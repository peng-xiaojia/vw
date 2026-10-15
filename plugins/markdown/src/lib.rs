//! vw 内置插件：Markdown（pulldown-cmark → Document IR）。

use std::io::Read;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag};
use vw_core::viewer::{Doc, EditableDoc, Viewer};
use vw_protocol::{
    run_text, search_blocks, Align, Block, BlockList, Cursor, DocMeta, EditCapability, EditSnapshot, Inline, Manifest,
    OpenOptions, Outline, OutlineItem, PageRange, ProbeScore, PluginError, SearchHits, Source, TextFile,
};

pub const EDIT_MAX_BYTES: u64 = 16 * 1024 * 1024;
const SNIFF_BYTES: usize = 8192;

pub struct MarkdownViewer {
    manifest: Manifest,
    viewer_id: String,
    extensions: Vec<String>,
}

impl MarkdownViewer {
    pub fn new(extensions: &[&str], priority: i64) -> Self {
        Self {
            manifest: Manifest::simple_in_process("vw.markdown", "markdown", extensions, &["view", "edit", "outline", "format"], priority),
            viewer_id: "markdown".into(),
            extensions: extensions.iter().map(|s| s.to_lowercase()).collect(),
        }
    }
}

fn heading_level(h: HeadingLevel) -> u8 {
    match h {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn alignment(a: pulldown_cmark::Alignment) -> Align {
    match a {
        pulldown_cmark::Alignment::Left => Align::Left,
        pulldown_cmark::Alignment::Center => Align::Center,
        pulldown_cmark::Alignment::Right => Align::Right,
        pulldown_cmark::Alignment::None => Align::Left,
    }
}

enum Ctx {
    Para(Vec<Inline>),
    Heading { level: u8, runs: Vec<Inline> },
    Quote(Vec<Block>),
    List { ordered: bool, items: Vec<Block> },
    Item { runs: Vec<Inline>, blocks: Vec<Block> },
    Code { lang: Option<String>, text: String },
    Table {
        aligns: Vec<Align>,
        head: Option<Vec<Inline>>,
        rows: Vec<Vec<Inline>>,
        cur_row: Vec<Inline>,
        cur_cell: Option<Vec<Inline>>,
        in_head: bool,
        expect_row_end: bool,
    },
    Styled { wrap: fn(Vec<Inline>) -> Inline, runs: Vec<Inline> },
    Link { href: String, runs: Vec<Inline> },
    Image { dest: String, runs: Vec<Inline> },
}

fn push_block(stack: &mut [Ctx], root: &mut Vec<Block>, b: Block) {
    for ctx in stack.iter_mut().rev() {
        match ctx {
            Ctx::Quote(v) | Ctx::List { items: v, .. } | Ctx::Item { blocks: v, runs: _ } => {
                v.push(b);
                return;
            }
            _ => {}
        }
    }
    root.push(b);
}

fn push_inline(stack: &mut [Ctx], inline: Inline) {
    for ctx in stack.iter_mut().rev() {
        match ctx {
            Ctx::Para(runs)
            | Ctx::Heading { runs, .. }
            | Ctx::Styled { runs, .. }
            | Ctx::Link { runs, .. }
            | Ctx::Image { runs, .. } => {
                merge_inline(runs, inline);
                return;
            }
            Ctx::Item { runs, blocks } if blocks.is_empty() => {
                merge_inline(runs, inline);
                return;
            }
            Ctx::Table { cur_cell: Some(runs), .. } => {
                merge_inline(runs, inline);
                return;
            }
            _ => {}
        }
    }
}

/// 相邻纯文本合并成一个 run，减少 Inline 数量。
fn merge_inline(runs: &mut Vec<Inline>, inline: Inline) {
    if let (Some(Inline::Text { text: t }), Inline::Text { text: s }) = (runs.last_mut(), &inline) {
        t.push_str(s);
        return;
    }
    runs.push(inline);
}

pub fn parse_markdown(src: &str) -> Vec<Block> {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    let mut blocks: Vec<Block> = Vec::new();
    let mut stack: Vec<Ctx> = Vec::new();

    for event in Parser::new_ext(src, opts) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => stack.push(Ctx::Para(vec![])),
                Tag::Heading { level, .. } => stack.push(Ctx::Heading { level: heading_level(level), runs: vec![] }),
                Tag::BlockQuote => stack.push(Ctx::Quote(vec![])),
                Tag::CodeBlock(kind) => {
                    let lang = match kind {
                        CodeBlockKind::Fenced(info) => {
                            let w = info.split_whitespace().next().unwrap_or("");
                            if w.is_empty() {
                                None
                            } else {
                                Some(w.to_string())
                            }
                        }
                        CodeBlockKind::Indented => None,
                    };
                    stack.push(Ctx::Code { lang, text: String::new() });
                }
                Tag::List(start) => stack.push(Ctx::List { ordered: start.is_some(), items: vec![] }),
                Tag::Item => stack.push(Ctx::Item { runs: vec![], blocks: vec![] }),
                Tag::Table(aligns) => stack.push(Ctx::Table {
                    aligns: aligns.iter().map(|a| alignment(*a)).collect(),
                    head: None,
                    rows: vec![],
                    cur_row: vec![],
                    cur_cell: None,
                    in_head: false,
                    expect_row_end: false,
                }),
                Tag::TableHead => {
                    if let Some(Ctx::Table { in_head, .. }) = stack.last_mut() {
                        *in_head = true;
                    }
                }
                Tag::TableRow => {
                    if let Some(Ctx::Table { expect_row_end, .. }) = stack.last_mut() {
                        *expect_row_end = true;
                    }
                }
                Tag::TableCell => {
                    if let Some(Ctx::Table { cur_cell, .. }) = stack.last_mut() {
                        *cur_cell = Some(vec![]);
                    }
                }
                Tag::Emphasis => stack.push(Ctx::Styled { wrap: |v| Inline::Italic { inner: v }, runs: vec![] }),
                Tag::Strong => stack.push(Ctx::Styled { wrap: |v| Inline::Bold { inner: v }, runs: vec![] }),
                // IR 无删除线：降级为斜体
                Tag::Strikethrough => stack.push(Ctx::Styled { wrap: |v| Inline::Italic { inner: v }, runs: vec![] }),
                Tag::Link { dest_url, .. } => {
                    stack.push(Ctx::Link { href: dest_url.to_string(), runs: vec![] })
                }
                Tag::Image { dest_url, .. } => stack.push(Ctx::Image { dest: dest_url.to_string(), runs: vec![] }),
                _ => {}
            },
            Event::End(_) => {
                // 表格内部的结束符在栈顶消化，不弹出 Table
                let mut handled = false;
                if let Some(Ctx::Table { head, rows, cur_row, cur_cell, in_head, expect_row_end, .. }) =
                    stack.last_mut()
                {
                    if cur_cell.is_some() {
                        let runs = cur_cell.take().unwrap();
                        cur_row.push(Inline::Text { text: run_text(&runs) });
                        handled = true;
                    } else if *in_head {
                        *head = Some(std::mem::take(cur_row));
                        *in_head = false;
                        handled = true;
                    } else if *expect_row_end {
                        *expect_row_end = false;
                        let row = std::mem::take(cur_row);
                        rows.push(row);
                        handled = true;
                    }
                }
                if handled {
                    continue;
                }
                match stack.pop() {
                    Some(Ctx::Para(runs)) => push_block(&mut stack, &mut blocks, Block::para_runs(runs)),
                    Some(Ctx::Heading { level, runs }) => {
                        push_block(&mut stack, &mut blocks, Block::Heading { level, text: runs })
                    }
                    Some(Ctx::Quote(v)) => push_block(&mut stack, &mut blocks, Block::Quote { blocks: v }),
                    Some(Ctx::List { ordered, items }) => {
                        push_block(&mut stack, &mut blocks, Block::List { ordered, items })
                    }
                    Some(Ctx::Item { mut runs, blocks: mut content }) => {
                        if !runs.is_empty() {
                            content.insert(0, Block::para_runs(std::mem::take(&mut runs)));
                        }
                        if content.is_empty() {
                            content.push(Block::para_runs(vec![]));
                        }
                        // Item 的内容块逐个上交（常规情况只有一个）
                        for b in content {
                            push_block(&mut stack, &mut blocks, b);
                        }
                    }
                    Some(Ctx::Code { lang, text }) => {
                        push_block(&mut stack, &mut blocks, Block::Code { lang, text })
                    }
                    Some(Ctx::Table { head, rows, aligns, .. }) => push_block(
                        &mut stack,
                        &mut blocks,
                        Block::Table { head, rows, align: aligns },
                    ),
                    Some(Ctx::Styled { wrap, runs }) => push_inline(&mut stack, wrap(runs)),
                    Some(Ctx::Link { href, runs }) => {
                        push_inline(&mut stack, Inline::Link { text: runs, href })
                    }
                    Some(Ctx::Image { dest, runs }) => {
                        let alt = run_text(&runs);
                        push_inline(&mut stack, Inline::text(format!("[图片 {alt}]({dest})")));
                    }
                    None => {}
                }
            }
            Event::Text(t) => {
                if let Some(Ctx::Code { text, .. }) = stack.last_mut() {
                    text.push_str(&t);
                } else {
                    push_inline(&mut stack, Inline::text(t.to_string()));
                }
            }
            Event::Code(c) => push_inline(&mut stack, Inline::Code { text: c.to_string() }),
            Event::SoftBreak | Event::HardBreak => push_inline(&mut stack, Inline::Break),
            Event::TaskListMarker(checked) => {
                push_inline(&mut stack, Inline::text(if checked { "☑ " } else { "☐ " }));
            }
            // P1 不渲染 HTML 与脚注引用
            Event::Html(_) | Event::InlineHtml(_) | Event::FootnoteReference(_) => {}
            Event::Rule => push_block(&mut stack, &mut blocks, Block::para("―――――")),
        }
    }
    blocks
}

pub fn outline_of(blocks: &[Block]) -> Outline {
    blocks
        .iter()
        .enumerate()
        .filter_map(|(i, b)| match b {
            Block::Heading { level, text } => Some(OutlineItem {
                level: *level,
                text: run_text(text),
                block: i as u64,
            }),
            _ => None,
        })
        .collect()
}

struct MarkdownDoc {
    viewer_id: String,
    file: TextFile,
    blocks: Vec<Block>,
    outline: Outline,
    title: Option<String>,
    size: u64,
}

impl Doc for MarkdownDoc {
    fn format_support(&self) -> Option<vw_protocol::FormatSupport> {
        Some(vw_protocol::FormatSupport { provider: "prettier".into(), parser: Some("markdown".into()) })
    }

    fn meta(&self) -> DocMeta {
        DocMeta {
            viewer: self.viewer_id.clone(),
            mime: Some("text/markdown".into()),
            size: Some(self.size),
            title: self.title.clone(),
            pages: Some(self.blocks.len() as u64),
        }
    }

    fn outline(&self) -> Option<Outline> {
        Some(self.outline.clone())
    }

    fn page(&self, range: PageRange) -> Result<BlockList, PluginError> {
        let start = range.start as usize;
        let total = self.blocks.len() as u64;
        if start >= self.blocks.len() {
            return Ok(BlockList { blocks: vec![], total: Some(total) });
        }
        let end = (start + range.count as usize).min(self.blocks.len());
        Ok(BlockList { blocks: self.blocks[start..end].to_vec(), total: Some(total) })
    }

    fn search(&self, query: &str, from: Cursor) -> Result<SearchHits, PluginError> {
        Ok(search_blocks(&self.blocks, query, from))
    }

    fn as_editable(&self) -> Option<&dyn EditableDoc> {
        if self.file.editable() {
            Some(self)
        } else {
            None
        }
    }
}

impl EditableDoc for MarkdownDoc {
    fn serialize_with_encoding(&self, edited: &EditSnapshot, encoding: Option<&str>) -> Result<Vec<u8>, PluginError> {
        self.file.serialize_as(edited, encoding)
    }
    fn edit_capability(&self) -> EditCapability {
        self.file.capability()
    }

    fn snapshot(&self) -> Result<EditSnapshot, PluginError> {
        self.file.snapshot()
    }

    fn serialize(&self, edited: &EditSnapshot) -> Result<Vec<u8>, PluginError> {
        self.file.serialize(edited)
    }
}

impl Viewer for MarkdownViewer {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn probe(&self, source: &Source) -> Option<ProbeScore> {
        let path = source.as_fs_path()?;
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if self.extensions.contains(&ext.to_lowercase()) {
                return Some(ProbeScore { score: 60, reason: "扩展名匹配".into() });
            }
        }
        let mut f = std::fs::File::open(path).ok()?;
        let mut head = vec![0u8; SNIFF_BYTES];
        let n = f.read(&mut head).ok()?;
        head.truncate(n);
        let text = String::from_utf8_lossy(&head);
        let first_line = text.lines().next().unwrap_or("");
        if first_line.starts_with("# ") || first_line == "---" {
            Some(ProbeScore { score: 30, reason: "内容嗅探为 Markdown".into() })
        } else {
            None
        }
    }

    fn open(&self, source: &Source, opts: &OpenOptions) -> Result<Box<dyn Doc>, PluginError> {
        let path = source
            .as_fs_path()
            .ok_or_else(|| PluginError::malformed("Markdown 插件只支持本地路径源"))?;
        let bytes = std::fs::read(path)?;
        let size = bytes.len() as u64;
        let title = path.file_name().map(|n| n.to_string_lossy().to_string());
        let file = TextFile::decode(&bytes, EDIT_MAX_BYTES, opts.encoding.as_deref())?;
        let blocks = parse_markdown(&file.text);
        let outline = outline_of(&blocks);
        Ok(Box::new(MarkdownDoc { viewer_id: self.viewer_id.clone(), file, blocks, outline, title, size }))
    }
}
