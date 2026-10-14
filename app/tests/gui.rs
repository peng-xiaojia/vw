//! GUI 命令层测试：`*_impl` 函数与 CLI/MCP 共享同一 Session 行为。

use vw_app::AppState;
use vw_core::Registry;
use vw_plugin_text::TextViewer;
use std::sync::Arc;

fn state() -> AppState {
    vw_core::set_tmp_override(Some(std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("app-tmp")));
    let mut reg = Registry::new();
    reg.register(Arc::new(TextViewer::new(&["txt"], 50)));
    reg.register(Arc::new(vw_plugin_archive::ArchiveViewer::new(55)));
    AppState::with_registry(reg)
}

fn write(name: &str, content: &[u8]) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("app");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, content).unwrap();
    p
}

#[test]
fn gui_open_page_search_save_roundtrip() {
    let s = state();
    let f = write("g.txt", b"hello gui\nsecond line\n");

    let info = s.open_impl(f.to_str().unwrap(), None, None).unwrap();
    assert_eq!(info.viewer_id, "text");
    assert!(info.editable);

    let page = s.page_impl(&info.handle, 0, 100).unwrap();
    assert_eq!(page["total"], 1);

    let hits = s.search_impl(&info.handle, "gui").unwrap();
    assert_eq!(hits.len(), 1);

    // 编辑保存（GUI 编辑器的保存按钮路径）
    let snap = s.snapshot_impl(&info.handle).unwrap();
    let r = s.save_impl(&info.handle, &format!("{snap}appended\n")).unwrap();
    assert!(r.bytes > 0);
    assert_eq!(std::fs::read(&f).unwrap(), b"hello gui\nsecond line\nappended\n");

    s.close_impl(&info.handle).unwrap();
    // 关闭后句柄失效
    assert!(s.page_impl(&info.handle, 0, 1).is_err());
}

#[test]
fn gui_open_member_via_zip() {
    // 归档成员打开（GUI 双击成员行路径）
    use std::io::Write;
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("app-zip");
    std::fs::create_dir_all(&dir).unwrap();
    let zip_path = dir.join("g.zip");
    let f = std::fs::File::create(&zip_path).unwrap();
    let mut zw = zip::ZipWriter::new(f);
    zw.start_file("note.txt", zip::write::SimpleFileOptions::default()).unwrap();
    zw.write_all(b"inside zip").unwrap();
    zw.finish().unwrap();

    let s = state();
    let info = s.open_impl(zip_path.to_str().unwrap(), None, Some("note.txt".into())).unwrap();
    assert_eq!(info.viewer_id, "text");
    assert!(info.source_display.contains("note.txt"), "{}", info.source_display);
    let page = s.page_impl(&info.handle, 0, 10).unwrap();
    let text = serde_json::to_string(&page).unwrap();
    assert!(text.contains("inside zip"), "{text}");
}
