use vw_plugin_text::logic::TextCore;
use vw_protocol::{EditSnapshot, PageRange};

#[test]
fn chunks_preserve_lines_and_page() {
    let lines: String = (0..250).map(|i| format!("line-{i}\n")).collect();
    let core = TextCore::from_bytes(lines.as_bytes(), Some("t".into()));
    assert_eq!(core.total, 3); // 100 + 100 + 50
    let p = core.page(PageRange { start: 0, count: 2 }).unwrap();
    assert_eq!(p.blocks.len(), 2);
    assert_eq!(p.total, Some(3));
    let p3 = core.page(PageRange { start: 2, count: 10 }).unwrap();
    assert_eq!(p3.blocks.len(), 1);
    let beyond = core.page(PageRange { start: 9, count: 10 }).unwrap();
    assert!(beyond.blocks.is_empty());
}

#[test]
fn bom_crlf_no_trailing_newline_round_trip() {
    // BOM + CRLF + 无尾随换行：snapshot→serialize 必须逐字节还原
    let raw = b"\xEF\xBB\xBFname,value\r\na,1\r\nb,2";
    let core = TextCore::from_bytes(raw, None);
    assert!(core.editable());
    let snap = core.snapshot().unwrap();
    let out = core.serialize(&snap).unwrap();
    assert_eq!(out, raw.to_vec());
    assert!(matches!(&snap, EditSnapshot::Text { text } if text.starts_with("name,value")));

    // LF 无 BOM 同样逐字节还原
    let core2 = TextCore::from_bytes(b"x\ny\n", None);
    let out2 = core2.serialize(&core2.snapshot().unwrap()).unwrap();
    assert_eq!(out2, b"x\ny\n".to_vec());
}

#[test]
fn invalid_utf8_degrades_to_readonly_but_still_viewable() {
    let raw = b"ok\xe4\xb8\xad\xff\xfebad"; // 含无效序列
    let core = TextCore::from_bytes(raw, None);
    assert!(!core.editable());
    assert!(matches!(core.edit_capability(), vw_protocol::EditCapability::None));
    assert!(core.snapshot().is_err());
    // 仍可分页查看（lossy）
    assert_eq!(core.total, 1);
    assert!(vw_protocol::block_text(&core.blocks[0]).unwrap().contains("ok中"));
}

#[test]
fn search_finds_across_blocks() {
    let lines: String = (0..150).map(|i| if i == 130 { "needle here\n".to_string() } else { format!("filler {i}\n") }).collect();
    let core = TextCore::from_bytes(lines.as_bytes(), None);
    let hits = core.search("needle", vw_protocol::Cursor::start()).unwrap();
    assert_eq!(hits.hits.len(), 1);
    assert_eq!(hits.hits[0].block, 1, "第 131 行落在第 2 块");
}
