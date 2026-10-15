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
    let p3 = core
        .page(PageRange {
            start: 2,
            count: 10,
        })
        .unwrap();
    assert_eq!(p3.blocks.len(), 1);
    let beyond = core
        .page(PageRange {
            start: 9,
            count: 10,
        })
        .unwrap();
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
    assert!(matches!(
        core.edit_capability(),
        vw_protocol::EditCapability::None
    ));
    assert!(core.snapshot().is_err());
    // 仍可分页查看（lossy）
    assert_eq!(core.total, 1);
    assert!(vw_protocol::block_text(&core.blocks[0])
        .unwrap()
        .contains("ok中"));
}

#[test]
fn search_finds_across_blocks() {
    let lines: String = (0..150)
        .map(|i| {
            if i == 130 {
                "needle here\n".to_string()
            } else {
                format!("filler {i}\n")
            }
        })
        .collect();
    let core = TextCore::from_bytes(lines.as_bytes(), None);
    let hits = core.search("needle", vw_protocol::Cursor::start()).unwrap();
    assert_eq!(hits.hits.len(), 1);
    assert_eq!(hits.hits[0].block, 1, "第 131 行落在第 2 块");
}

#[test]
fn serialize_rejects_edit_that_exceeds_capability_limit() {
    let core = TextCore::from_bytes(b"small", None);
    let text = "x".repeat(vw_plugin_text::EDIT_MAX_BYTES as usize + 1);
    assert!(core.serialize(&EditSnapshot::text(text)).is_err());
}

#[test]
fn sniff_rejects_invalid_trailing_byte() {
    assert!(!vw_plugin_text::sniff_is_text(b"abc\xff"));
    assert!(vw_plugin_text::sniff_is_text(b"abc\xe4\xb8"));
}

#[test]
fn large_text_pages_lazily_with_bounded_utf8_blocks_and_search_cursors() {
    use vw_core::Viewer;
    use vw_protocol::{Cursor, OpenOptions, Source};
    let path = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("lazy-text.txt");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let text = "中".repeat(400_000) + "\nneedle\nneedle\n";
    std::fs::write(&path, text.as_bytes()).unwrap();
    let doc = vw_plugin_text::TextViewer::new(&["txt"], 50)
        .open(&Source::path(&path), &OpenOptions::default())
        .unwrap();
    assert_eq!(
        doc.meta().pages,
        None,
        "opening large text must not precompute its pages"
    );
    let mut restored = String::new();
    let mut block = 0;
    loop {
        let page = doc
            .page(PageRange {
                start: block,
                count: 1,
            })
            .unwrap();
        if page.blocks.is_empty() {
            break;
        }
        let part = vw_protocol::block_text(&page.blocks[0]).unwrap();
        assert!(
            part.len() <= 65536,
            "long lines must not create unbounded blocks"
        );
        assert!(
            !part.contains('\u{fffd}'),
            "UTF-8 split must retain complete characters"
        );
        restored.push_str(&part);
        block += 1;
    }
    assert_eq!(restored, text);
    let hits = doc
        .search(
            "needle",
            Cursor {
                block: block - 1,
                offset: 0,
            },
        )
        .unwrap();
    assert_eq!(hits.hits.len(), 2);
    let next = doc
        .search(
            "needle",
            Cursor {
                block: hits.hits[0].block,
                offset: hits.hits[0].start + 6,
            },
        )
        .unwrap();
    assert_eq!(next.hits.len(), 1);
    assert_eq!(next.hits[0], hits.hits[1]);
    assert!(
        doc.as_editable().is_none(),
        "large paged documents are readonly"
    );
}

#[test]
fn lazy_search_returns_continuation_before_scanning_entire_file() {
    use vw_core::Viewer;
    use vw_protocol::{Cursor, OpenOptions, Source};
    let path = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("bounded-search.txt");
    let text = "x".repeat(17 * 1024 * 1024) + "needle";
    std::fs::write(&path, text).unwrap();
    let doc = vw_plugin_text::TextViewer::new(&["txt"], 50)
        .open(&Source::path(&path), &OpenOptions::default())
        .unwrap();
    let first = doc.search("needle", Cursor::start()).unwrap();
    assert!(first.hits.is_empty());
    assert_eq!(
        first.next,
        Some(Cursor {
            block: 256,
            offset: 0
        })
    );
    let second = doc.search("needle", first.next.unwrap()).unwrap();
    assert_eq!(second.hits.len(), 1);
    assert_eq!(second.hits[0].block, 272);
    assert!(second.next.is_none());
}
