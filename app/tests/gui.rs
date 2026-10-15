//! GUI 命令层测试：`*_impl` 函数与 CLI/MCP 共享同一 Session 行为。

use std::sync::Arc;
use vw_app::AppState;
use vw_core::Registry;
use vw_plugin_text::TextViewer;

fn state() -> AppState {
    vw_core::set_tmp_override(Some(
        std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("app-tmp"),
    ));
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
    let r = s
        .save_impl(&info.handle, &format!("{snap}appended\n"))
        .unwrap();
    assert!(r.bytes > 0);
    assert_eq!(
        std::fs::read(&f).unwrap(),
        b"hello gui\nsecond line\nappended\n"
    );
    assert!(s.snapshot_impl(&info.handle).unwrap().contains("appended"));
    assert!(!s.search_impl(&info.handle, "appended").unwrap().is_empty());
    assert!(
        serde_json::to_string(&s.page_impl(&info.handle, 0, 100).unwrap())
            .unwrap()
            .contains("appended")
    );
    s.save_impl(&info.handle, "second save\n").unwrap();
    assert_eq!(s.snapshot_impl(&info.handle).unwrap(), "second save\n");

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
    zw.start_file("note.txt", zip::write::SimpleFileOptions::default())
        .unwrap();
    zw.write_all(b"inside zip").unwrap();
    zw.finish().unwrap();

    let s = state();
    let info = s
        .open_impl(zip_path.to_str().unwrap(), None, Some("note.txt".into()))
        .unwrap();
    assert_eq!(info.viewer_id, "text");
    assert!(
        info.source_display.contains("note.txt"),
        "{}",
        info.source_display
    );
    assert!(!info.editable);
    assert!(s
        .save_impl(&info.handle, "must not modify archive")
        .is_err());
    let page = s.page_impl(&info.handle, 0, 10).unwrap();
    let text = serde_json::to_string(&page).unwrap();
    assert!(text.contains("inside zip"), "{text}");
    let export = dir.join("exported.txt");
    let _ = std::fs::remove_file(&export);
    assert_eq!(
        s.export_impl(&info.handle, export.to_str().unwrap())
            .unwrap(),
        10
    );
    assert_eq!(std::fs::read(&export).unwrap(), b"inside zip");
    assert!(s
        .export_impl(&info.handle, export.to_str().unwrap())
        .is_err());
}

#[test]
fn gui_path_errors_and_native_launch_routing() {
    let s = state();
    assert!(s
        .open_impl("this-file-does-not-exist-vw.txt", None, None)
        .is_err());
    assert!(s.blob_impl("unknown-handle", "../../outside").is_err());
    assert!(s.startup_paths_impl().is_empty());
    assert!(s.startup_paths_impl().is_empty());
    let cwd = std::path::Path::new(r"C:\launch folder");
    let paths = vw_app::launch_paths(
        vec!["vw-app.exe".into(), "中文 文件.txt".into(), "--".into()],
        cwd,
    );
    assert_eq!(paths, vec![cwd.join("中文 文件.txt").display().to_string()]);
    for denied in [
        "javascript:alert(1)",
        "file:///C:/secret",
        "data:text/html,hello",
        "ms-settings:",
        "https://example.com/\n",
    ] {
        assert!(vw_app::validate_external_url(denied).is_err(), "{denied}");
    }
    for allowed in [
        "https://example.com/page",
        "http://example.com",
        "mailto:a@example.com",
    ] {
        assert!(vw_app::validate_external_url(allowed).is_ok(), "{allowed}");
    }
}

#[test]
fn directory_listing_has_stable_paths_and_directories_first() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("directory-navigation");
    std::fs::create_dir_all(dir.join("z-directory")).unwrap();
    std::fs::write(dir.join("a-file.txt"), b"abc").unwrap();
    let listing = vw_plugin_dir::list_directory(&dir).unwrap();
    assert!(listing.entries[0].is_dir);
    assert_eq!(listing.entries[1].size, Some(3));
    assert_eq!(listing.entries[1].name, "a-file.txt");
    assert!(std::path::Path::new(&listing.entries[1].path).is_absolute());
    assert!(listing.parent.is_some());
    assert!(vw_plugin_dir::list_directory(&dir.join("a-file.txt")).is_err());
}

#[test]
fn office_preview_is_available_through_the_default_gui_registry() {
    use std::io::Write;
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("gui-office");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("中文 内容.docx");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    zip.start_file(
        "word/document.xml",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(br#"<w:document xmlns:w="urn:w"><w:body><w:p><w:r><w:t>Office preview</w:t></w:r></w:p></w:body></w:document>"#).unwrap();
    zip.finish().unwrap();
    let s = AppState::new().unwrap();
    let info = s.open_impl(path.to_str().unwrap(), None, None).unwrap();
    assert_eq!(info.viewer_id, "office");
    assert!(!info.editable);
    assert!(
        serde_json::to_string(&s.page_impl(&info.handle, 0, 10).unwrap())
            .unwrap()
            .contains("Office preview")
    );
    assert!(!s.search_impl(&info.handle, "preview").unwrap().is_empty());
    assert!(s.save_impl(&info.handle, "change").is_err());
    s.close_impl(&info.handle).unwrap();
}
