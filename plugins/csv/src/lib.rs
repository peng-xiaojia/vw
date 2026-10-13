//! vw 内置插件：CSV / TSV。

use vw_core::viewer::{Doc, EditableDoc, Viewer};
use vw_protocol::{
    search_blocks, Align, Block, BlockList, Cursor, DocMeta, EditCapability, EditSnapshot, Inline, Manifest,
    OpenOptions, PageRange, ProbeScore, PluginError, SearchHits, Source, TextFile,
};

pub const EDIT_MAX_BYTES: u64 = 64 * 1024 * 1024;
const SNIFF_BYTES: usize = 8192;
/// 每个表格块容纳的数据行数。
const ROWS_PER_BLOCK: usize = 500;

pub struct CsvViewer {
    manifest: Manifest,
    viewer_id: String,
    extensions: Vec<String>,
}

impl CsvViewer {
    pub fn new(extensions: &[&str], priority: i64) -> Self {
        Self {
            manifest: Manifest::simple_in_process("vw.csv", "csv", extensions, &["view", "edit"], priority),
            viewer_id: "csv".into(),
            extensions: extensions.iter().map(|s| s.to_lowercase()).collect(),
        }
    }

    fn delimiter_for(&self, path: &std::path::Path, head: &[u8]) -> u8 {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext.eq_ignore_ascii_case("tsv") {
                return b'\t';
            }
        }
        let first_line = head.split(|&b| b == b'\n').next().unwrap_or(head);
        let tabs = first_line.iter().filter(|&&b| b == b'\t').count();
        let commas = first_line.iter().filter(|&&b| b == b',').count();
        if tabs > commas {
            b'\t'
        } else {
            b','
        }
    }
}

fn parse_records(body: &[u8], delimiter: u8) -> Result<Vec<Vec<String>>, PluginError> {
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(body);
    let mut out = Vec::new();
    for rec in rdr.records() {
        let rec = rec.map_err(|e| PluginError::malformed(format!("CSV 解析失败：{e}")))?;
        out.push(rec.iter().map(|s| s.to_string()).collect());
    }
    Ok(out)
}

fn build_blocks(records: &[Vec<String>]) -> (Vec<Block>, u64) {
    if records.is_empty() {
        let block = Block::Table { head: None, rows: vec![], align: vec![] };
        return (vec![block], 1);
    }
    let head: Vec<Inline> = records[0].iter().map(|s| Inline::text(s.clone())).collect();
    let data = &records[1..];
    let mut blocks = Vec::new();
    for chunk in data.chunks(ROWS_PER_BLOCK) {
        let rows: Vec<Vec<Inline>> =
            chunk.iter().map(|r| r.iter().map(|s| Inline::text(s.clone())).collect()).collect();
        blocks.push(Block::Table { head: Some(head.clone()), rows, align: vec![] });
    }
    if blocks.is_empty() {
        blocks.push(Block::Table { head: Some(head), rows: vec![], align: vec![] });
    }
    let total = blocks.len() as u64;
    (blocks, total)
}

struct CsvDoc {
    viewer_id: String,
    file: TextFile,
    blocks: Vec<Block>,
    mime: &'static str,
    title: Option<String>,
    size: u64,
}

impl Doc for CsvDoc {
    fn meta(&self) -> DocMeta {
        DocMeta {
            viewer: self.viewer_id.clone(),
            mime: Some(self.mime.into()),
            size: Some(self.size),
            title: self.title.clone(),
            pages: Some(self.blocks.len() as u64),
        }
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

impl EditableDoc for CsvDoc {
    fn edit_capability(&self) -> EditCapability {
        self.file.capability()
    }

    fn snapshot(&self) -> Result<EditSnapshot, PluginError> {
        self.file.snapshot()
    }

    /// CSV 编辑按原始文本写回（不重排版）；解析校验宽松，无法判无效即放行。
    fn serialize(&self, edited: &EditSnapshot) -> Result<Vec<u8>, PluginError> {
        self.file.serialize(edited)
    }
}

impl Viewer for CsvViewer {
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
        let n = std::io::Read::read(&mut f, &mut head).ok()?;
        head.truncate(n);
        if head.contains(&0) {
            return None;
        }
        let first_line = head.split(|&b| b == b'\n').next().unwrap_or(&head);
        let has_comma = first_line.contains(&b',') || first_line.contains(&b'\t');
        if has_comma {
            Some(ProbeScore { score: 30, reason: "内容嗅探为分隔符文本".into() })
        } else {
            None
        }
    }

    fn open(&self, source: &Source, _opts: &OpenOptions) -> Result<Box<dyn Doc>, PluginError> {
        let path = source
            .as_fs_path()
            .ok_or_else(|| PluginError::malformed("CSV 插件只支持本地路径源"))?;
        let bytes = std::fs::read(path)?;
        let size = bytes.len() as u64;
        let title = path.file_name().map(|n| n.to_string_lossy().to_string());

        let body: &[u8] = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) { &bytes[3..] } else { &bytes };
        let head_end = body.split(|&b| b == b'\n').next().map(|l| l.len()).unwrap_or(0);
        let delimiter = self.delimiter_for(path, &body[..head_end.min(body.len())]);
        let records = parse_records(body, delimiter)?;
        let is_tsv = delimiter == b'\t';
        let (blocks, _) = build_blocks(&records);
        Ok(Box::new(CsvDoc {
            viewer_id: self.viewer_id.clone(),
            file: TextFile::from_bytes(&bytes, EDIT_MAX_BYTES),
            blocks,
            mime: if is_tsv { "text/tab-separated-values" } else { "text/csv" },
            title,
            size,
        }))
    }
}

/// 供测试与文档断言的列对齐默认值。
pub const DEFAULT_ALIGN: Align = Align::Left;
