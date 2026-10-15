use vw_core::viewer::Viewer;
use vw_plugin_hex::HexViewer;
use vw_protocol::{OpenOptions, PageRange, ProbeScore, Source};

fn write_tmp(name: &str, content: &[u8]) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, content).unwrap();
    p
}

#[test]
fn hex_probe_claims_everything_at_zero() {
    let v = HexViewer::new(10);
    let f = write_tmp("any.bin", b"\x00\x01\x02");
    let ProbeScore { score, reason } = v.probe(&Source::path(&f)).unwrap();
    assert_eq!(score, 0);
    assert!(!reason.is_empty());
}

#[test]
fn hexdump_lines_shape_and_readonly() {
    let payload: Vec<u8> = (0u8..=47).chain([0x00, 0xff]).collect(); // 49 字节 = 4 行
    let f = write_tmp("b.bin", &payload);
    let v = HexViewer::new(10);
    let doc = v.open(&Source::path(&f), &OpenOptions::default()).unwrap();
    assert_eq!(doc.meta().mime.as_deref(), Some("application/octet-stream"));
    assert!(doc.as_editable().is_none(), "hex 永远只读");
    let page = doc
        .page(PageRange {
            start: 0,
            count: 100,
        })
        .unwrap();
    let text = vw_protocol::block_text(&page.blocks[0]).unwrap();
    let first = text.lines().next().unwrap();
    assert!(first.starts_with("00000000  "), "首行偏移格式：{first}");
    // 布局：8 偏移 + 2 空格 + 49 hex 栏 + |................|
    assert_eq!(
        first.chars().count(),
        8 + 2 + 49 + 1 + 16 + 1,
        "整行列数：{first}"
    );
    // 第 5 行只有 2 个字节（0x00 0xff），ASCII 栏以 14 个点补齐
    let last = text.lines().last().unwrap();
    assert!(
        last.starts_with("00000030  00 ff "),
        "末行偏移与字节：{last}"
    );
    assert!(last.ends_with("|................|"), "ASCII 栏补齐：{last}");
}

#[test]
fn paging_reaches_beyond_old_view_cap_with_absolute_offsets() {
    use std::io::{Seek, SeekFrom, Write};
    let path = write_tmp("large-seek.bin", b"");
    let mut f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    f.set_len(20 * 1024 * 1024).unwrap();
    f.seek(SeekFrom::Start(17 * 1024 * 1024)).unwrap();
    f.write_all(b"MARKER").unwrap();
    drop(f);
    let doc = HexViewer::new(10)
        .open(&Source::path(&path), &OpenOptions::default())
        .unwrap();
    assert_eq!(doc.meta().pages, Some(2560));
    let page = doc
        .page(PageRange {
            start: 2176,
            count: 1,
        })
        .unwrap();
    let text = vw_protocol::block_text(&page.blocks[0]).unwrap();
    assert!(text.starts_with("01100000  4d 41 52 4b 45 52 "), "{text}");
    let hits = doc
        .search(
            "MARKER",
            vw_protocol::Cursor {
                block: 2176,
                offset: 0,
            },
        )
        .unwrap();
    assert_eq!(hits.hits[0].block, 2176);
    let bounded = doc.search("MARKER", vw_protocol::Cursor::start()).unwrap();
    assert!(bounded.hits.is_empty());
    assert_eq!(
        bounded.next,
        Some(vw_protocol::Cursor {
            block: 256,
            offset: 0
        })
    );
}
