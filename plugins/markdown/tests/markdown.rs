use vw_core::viewer::Viewer;
use vw_plugin_markdown::MarkdownViewer;
use vw_protocol::{Block, OpenOptions, PageRange, Source};

fn write_tmp(name: &str, content: &str) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, content).unwrap();
    p
}

fn open_md(name: &str, content: &str) -> Box<dyn vw_core::viewer::Doc> {
    let f = write_tmp(name, content);
    MarkdownViewer::new(&["md"], 50)
        .open(&Source::path(&f), &OpenOptions::default())
        .unwrap()
}

#[test]
fn headings_paragraphs_and_outline() {
    let doc = open_md("a.md", "# 标题一\n\n普通段落。\n\n## 小节\n\n第二段。\n");
    let page = doc.page(PageRange { start: 0, count: 100 }).unwrap();
    assert!(matches!(page.blocks[0], Block::Heading { level: 1, .. }));
    assert!(matches!(page.blocks[1], Block::Para { .. }));
    assert!(matches!(page.blocks[2], Block::Heading { level: 2, .. }));
    let outline = doc.outline().unwrap();
    assert_eq!(outline.len(), 2);
    assert_eq!(outline[0].text, "标题一");
    assert_eq!(outline[1].block, 2);
}

#[test]
fn styled_runs_and_inline_code() {
    let doc = open_md("b.md", "前 **粗体** 中 `code` 后。\n");
    let page = doc.page(PageRange { start: 0, count: 10 }).unwrap();
    match &page.blocks[0] {
        Block::Para { text } => {
            assert_eq!(text.len(), 5, "文本/粗体/文本/行内码/文本 五个 run，实际 {text:?}");
            assert!(text.iter().any(para_is_bold));
            assert!(text.iter().any(|i| matches!(i, vw_protocol::Inline::Code { text } if text == "code")));
        }
        other => panic!("应为 Para，实际 {other:?}"),
    }
}

// 局部断言辅助，避免测试里写长路径
use vw_protocol::Inline;
fn para_is_bold(i: &Inline) -> bool {
    matches!(i, Inline::Bold { inner } if run(inner) == "粗体")
}
fn run(v: &[Inline]) -> String {
    v.iter().map(|i| vw_protocol::inline_text(i)).collect()
}

#[test]
fn list_and_code_block() {
    let doc = open_md("c.md", "- one\n- two\n\n```rust\nfn main() {}\n```\n");
    let page = doc.page(PageRange { start: 0, count: 10 }).unwrap();
    assert!(matches!(&page.blocks[0], Block::List { ordered: false, items } if items.len() == 2));
    match &page.blocks[1] {
        Block::Code { lang, text } => {
            assert_eq!(lang.as_deref(), Some("rust"));
            assert_eq!(text, "fn main() {}\n");
        }
        other => panic!("应为 Code，实际 {other:?}"),
    }
}

#[test]
fn table_maps_to_table_block() {
    let doc = open_md("d.md", "| a | b |\n|---|--:|\n| 1 | 2 |\n");
    let page = doc.page(PageRange { start: 0, count: 10 }).unwrap();
    match &page.blocks[0] {
        Block::Table { head, rows, align } => {
            assert_eq!(head.as_ref().unwrap().len(), 2);
            assert_eq!(rows.len(), 1);
            assert_eq!(align[1], vw_protocol::Align::Right);
        }
        other => panic!("应为 Table，实际 {other:?}"),
    }
}

#[test]
fn edit_round_trip_and_sniff() {
    let f = write_tmp("e.md", "# t\n\n正文\n");
    let v = MarkdownViewer::new(&["md"], 50);
    let doc = v.open(&Source::path(&f), &OpenOptions::default()).unwrap();
    let ed = doc.as_editable().unwrap();
    let bytes = ed.serialize(&ed.snapshot().unwrap()).unwrap();
    assert_eq!(bytes, "# t\n\n正文\n".as_bytes().to_vec());
    // 无扩展名但首行是标题 → 嗅探命中
    let f2 = write_tmp("f", "# 标题\n");
    assert!(v.probe(&Source::path(&f2)).is_some());
}
