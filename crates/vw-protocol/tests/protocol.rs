use vw_protocol::{
    block_from_value, search_blocks, Align, Block, Cursor, EditCapability, Inline, LineEnding, Manifest, Source,
};

#[test]
fn ir_serde_round_trip() {
    let blocks = vec![
        Block::heading(1, "标题"),
        Block::Para { text: Inline::Bold { inner: Box::new(Inline::text("粗体")) } },
        Block::Table {
            head: Some(vec![Inline::text("a"), Inline::text("b")]),
            rows: vec![vec![Inline::text("1"), Inline::text("2")]],
            align: vec![Align::Left, Align::Right],
        },
        Block::Code { lang: Some("rust".into()), text: "fn main() {}".into() },
        Block::Preformatted { text: "原样\n文本".into() },
        Block::Canvas {
            w: 10,
            h: 20,
            ops: vec![vw_protocol::DrawOp::Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0, color: "#fff".into() }],
        },
    ];
    let json = serde_json::to_string(&blocks).unwrap();
    let back: Vec<Block> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, blocks);
}

#[test]
fn unknown_block_kind_degrades_to_error() {
    let v: serde_json::Value =
        serde_json::from_str(r#"{"kind":"hologram","x":1}"#).unwrap();
    match block_from_value(&v) {
        Block::Error { message } => assert!(message.contains("hologram") || message.contains("未知")),
        other => panic!("应降级为 Error 块，实际是 {other:?}"),
    }
}

#[test]
fn unknown_block_kind_inside_list_does_not_fail_whole_list() {
    let v: serde_json::Value = serde_json::from_str(
        r#"{"blocks":[{"kind":"para","text":{"kind":"text","text":"ok"}},{"kind":"fancy"}],"total":2}"#,
    )
    .unwrap();
    let list = vw_protocol::block_list_from_value(&v);
    assert_eq!(list.blocks.len(), 2);
    assert!(matches!(list.blocks[0], Block::Para { .. }));
    assert!(matches!(list.blocks[1], Block::Error { .. }));
}

#[test]
fn manifest_from_spec_example_parses() {
    // 设计规格 §7.3 的 manifest 草案
    let json = r#"{
      "manifest_version": 1,
      "id": "io.example.pdf",
      "name": "PDF Viewer",
      "version": "0.1.0",
      "protocol": { "contract": 1 },
      "engines": { "vw": ">=0.1.0" },
      "transport": {
        "kind": "stdio-jsonrpc",
        "framing": "jsonl",
        "executable": "bin/{platform}/vw-pdf{ext}"
      },
      "contributes": {
        "viewers": [
          { "id": "pdf", "extensions": ["pdf"], "mime": ["application/pdf"], "capabilities": ["view"] }
        ]
      },
      "permissions": ["fs.read", "tmp.write"],
      "priority": 50,
      "signature": null
    }"#;
    let m: Manifest = serde_json::from_str(json).unwrap();
    m.validate().unwrap();
    assert_eq!(m.transport.kind, vw_protocol::TransportKind::StdioJsonrpc);
    assert!(!m.declares_edit("pdf"));
}

#[test]
fn in_process_manifest_validates() {
    let json = r#"{
      "manifest_version": 1,
      "id": "vw.text",
      "name": "Text",
      "version": "0.1.0",
      "transport": { "kind": "in-process" },
      "contributes": { "viewers": [ { "id": "text", "capabilities": ["view", "edit"] } ] }
    }"#;
    let m: Manifest = serde_json::from_str(json).unwrap();
    m.validate().unwrap();
    assert!(m.declares_edit("text"));
}

#[test]
fn manifest_rejects_bad_capability() {
    let json = r#"{
      "manifest_version": 1,
      "id": "x", "name": "x", "version": "0.1.0",
      "transport": { "kind": "in-process" },
      "contributes": { "viewers": [ { "id": "x", "capabilities": ["god-mode"] } ] }
    }"#;
    let m: Manifest = serde_json::from_str(json).unwrap();
    assert!(m.validate().is_err());
}

#[test]
fn edit_capability_serde_matches_wire_shape() {
    let c = EditCapability::Text {
        encoding: "utf-8".into(),
        line_ending: LineEnding::Lf,
        has_bom: false,
        max_bytes: Some(1024),
    };
    let json = serde_json::to_value(&c).unwrap();
    assert_eq!(json["kind"], "text");
    let back: EditCapability = serde_json::from_value(json).unwrap();
    assert_eq!(back, c);
    // 未知取值 → 只读（规格 §5.4）
    let unknown: Result<EditCapability, _> =
        serde_json::from_value(serde_json::json!({"kind": "quantum"}));
    assert!(unknown.is_err(), "未知 kind 必须解析失败，由调用方降级为只读");
}

#[test]
fn source_serde_matches_wire_shape() {
    let s = Source::path("/a/b.pdf");
    let json = serde_json::to_value(&s).unwrap();
    assert_eq!(json["kind"], "path");
    assert_eq!(json["path"], "/a/b.pdf");
    let back: Source = serde_json::from_value(json).unwrap();
    assert_eq!(back, s);
}

#[test]
fn search_is_case_insensitive_and_resumes_from_cursor() {
    let blocks = vec![Block::preformatted("hello World\nworld peace"), Block::preformatted("no match here")];
    let hits = search_blocks(&blocks, "world", Cursor::start());
    assert_eq!(hits.hits.len(), 2);
    assert_eq!(hits.hits[0].block, 0);
    assert_eq!(hits.hits[1].start, 12);

    let hits2 = search_blocks(&blocks, "world", Cursor { block: 0, offset: hits.hits[0].start + 1 });
    assert_eq!(hits2.hits.len(), 1);
    assert_eq!(hits2.hits[0].start, 12);

    let hits3 = search_blocks(&blocks, "world", Cursor { block: 1, offset: 0 });
    assert!(hits3.hits.is_empty());
    assert!(hits3.next.is_none());
}
