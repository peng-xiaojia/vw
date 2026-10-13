use vw_core::viewer::Viewer;
use vw_plugin_csv::CsvViewer;
use vw_protocol::{Block, OpenOptions, PageRange, Source};

fn write_tmp(name: &str, content: &[u8]) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, content).unwrap();
    p
}

#[test]
fn csv_builds_table_with_head() {
    let f = write_tmp("a.csv", b"name,age\nalice,30\nbob,25\n");
    let v = CsvViewer::new(&["csv"], 50);
    let doc = v.open(&Source::path(&f), &OpenOptions::default()).unwrap();
    assert_eq!(doc.meta().mime.as_deref(), Some("text/csv"));
    let page = doc.page(PageRange { start: 0, count: 10 }).unwrap();
    assert_eq!(page.total, Some(1));
    match &page.blocks[0] {
        Block::Table { head, rows, .. } => {
            assert_eq!(head.as_ref().unwrap().len(), 2);
            assert_eq!(rows.len(), 2);
        }
        other => panic!("应为 Table 块，实际 {other:?}"),
    }
}

#[test]
fn tsv_delimiter_sniffed_by_extension() {
    let f = write_tmp("b.tsv", "名字\t年龄\n张三\t30\n".as_bytes());
    let v = CsvViewer::new(&["csv", "tsv"], 50);
    let doc = v.open(&Source::path(&f), &OpenOptions::default()).unwrap();
    let page = doc.page(PageRange { start: 0, count: 10 }).unwrap();
    match &page.blocks[0] {
        Block::Table { rows, .. } => assert_eq!(rows.len(), 1),
        other => panic!("应为 Table 块，实际 {other:?}"),
    }
}

#[test]
fn quoted_csv_round_trips_through_edit() {
    let raw = b"name,note\nalice,\"a, b\"\"c\"\n";
    let f = write_tmp("c.csv", raw);
    let v = CsvViewer::new(&["csv"], 50);
    let doc = v.open(&Source::path(&f), &OpenOptions::default()).unwrap();
    let ed = doc.as_editable().unwrap();
    let snap = ed.snapshot().unwrap();
    let bytes = ed.serialize(&snap).unwrap();
    assert_eq!(bytes, raw.to_vec(), "CSV 编辑按原始文本逐字节往返");
}
