use vw_plugin_json::JsonViewer;
use vw_protocol::{OpenOptions, PageRange, Source};
use vw_core::viewer::Viewer;

fn write_tmp(name: &str, content: &[u8]) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, content).unwrap();
    p
}

#[test]
fn whole_json_pretty_prints_and_outlines_top_keys() {
    let f = write_tmp("a.json", br#"{"b":1,"a":{"x":2}}"#);
    let v = JsonViewer::new(&["json"], 50);
    let doc = v.open(&Source::path(&f), &OpenOptions::default()).unwrap();
    assert_eq!(doc.meta().mime.as_deref(), Some("application/json"));
    let page = doc.page(PageRange { start: 0, count: 10 }).unwrap();
    assert_eq!(page.total, Some(1));
    let outline = doc.outline().unwrap();
    assert_eq!(outline[0].text, "b"); // serde pretty 保持插入序
}

#[test]
fn jsonl_splits_per_line() {
    let f = write_tmp("b.jsonl", b"{\"a\":1}\n{\"b\":2}\n\n{\"c\":3}\n");
    let v = JsonViewer::new(&["jsonl"], 50);
    let doc = v.open(&Source::path(&f), &OpenOptions::default()).unwrap();
    assert_eq!(doc.meta().mime.as_deref(), Some("application/x-ndjson"));
    let page = doc.page(PageRange { start: 0, count: 100 }).unwrap();
    assert_eq!(page.total, Some(3));
}

#[test]
fn broken_json_fails_open_with_clear_error() {
    let f = write_tmp("c.json", b"{oops}");
    let v = JsonViewer::new(&["json"], 50);
    let err = match v.open(&Source::path(&f), &OpenOptions::default()) {
        Err(e) => e,
        Ok(_) => panic!("损坏的 JSON 应打开失败"),
    };
    assert_eq!(err.kind, vw_protocol::ErrorKind::MalformedSource);
}

#[test]
fn edit_rejects_invalid_json_but_round_trips_valid() {
    let f = write_tmp("d.json", b"{\"k\": 1}\n");
    let v = JsonViewer::new(&["json"], 50);
    let doc = v.open(&Source::path(&f), &OpenOptions::default()).unwrap();
    let ed = doc.as_editable().unwrap();

    // 逐字节往返
    let snap = ed.snapshot().unwrap();
    let bytes = ed.serialize(&snap).unwrap();
    assert_eq!(bytes, b"{\"k\": 1}\n".to_vec());

    // 合法修改通过
    let ok = ed.serialize(&vw_protocol::EditSnapshot::text("{\"k\": 2}")).unwrap();
    assert_eq!(ok, b"{\"k\": 2}".to_vec());

    // 非法修改被拒绝
    let err = ed.serialize(&vw_protocol::EditSnapshot::text("{broken")).unwrap_err();
    assert_eq!(err.kind, vw_protocol::ErrorKind::SaveRejected);
    assert!(vw_protocol::Manifest::simple_in_process("vw.json", "json", &["json"], &["view", "edit"], 50)
        .declares_edit("json"));
}
