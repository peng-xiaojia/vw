use vw_core::viewer::Viewer;
use vw_plugin_dir::DirViewer;
use vw_protocol::{OpenOptions, PageRange, Source};

fn make_tmp(name: &str) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("b.txt"), b"2222").unwrap();
    std::fs::write(dir.join("a.txt"), b"1").unwrap();
    std::fs::write(dir.join("sub").join("nested.md"), b"# n").unwrap();
    dir
}

#[test]
fn dir_source_lists_entries_sorted() {
    let dir = make_tmp("vw-dir-test");
    let v = DirViewer::new(60);
    assert!(v.probe(&Source::Dir { path: dir.clone() }).is_some());
    let doc = v.open(&Source::Dir { path: dir.clone() }, &OpenOptions::default()).unwrap();
    assert_eq!(doc.meta().mime.as_deref(), Some("inode/directory"));
    let page = doc.page(PageRange { start: 0, count: 100 }).unwrap();
    let text = page.blocks.iter().filter_map(vw_protocol::block_text).collect::<Vec<_>>().join("\n");
    // 目录在前、文件按名称排序
    let sub_pos = text.find("sub").unwrap();
    let a_pos = text.find("a.txt").unwrap();
    let b_pos = text.find("b.txt").unwrap();
    assert!(sub_pos < a_pos && a_pos < b_pos, "{text}");
}

#[test]
fn dir_is_readonly_and_path_dir_scores_lower() {
    let dir = make_tmp("vw-dir-test2");
    let v = DirViewer::new(60);
    let doc = v.open(&Source::Dir { path: dir.clone() }, &OpenOptions::default()).unwrap();
    assert!(doc.as_editable().is_none(), "目录永远只读");
    // 路径变体指向目录：可处理但低分
    let s = v.probe(&Source::path(&dir)).unwrap();
    assert_eq!(s.score, 30);
    let _ = std::fs::remove_dir_all(&dir);
}
