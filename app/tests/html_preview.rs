use std::path::PathBuf;

#[test]
fn html_preview_preserves_resources_and_revokes_access_on_close() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("html-preview-resources");
    std::fs::create_dir_all(root.join("assets")).unwrap();
    std::fs::write(root.join("index.html"), "<!doctype html><link rel=stylesheet href=assets/site.css><h1>Original</h1>").unwrap();
    std::fs::write(root.join("assets/site.css"), "h1 { color: red }").unwrap();
    std::fs::write(root.join("assets/logo.svg"), "<svg xmlns='http://www.w3.org/2000/svg'/>").unwrap();
    std::fs::write(root.join("secret.key"), "private").unwrap();
    std::fs::write(root.join("../outside.css"), "private").unwrap();
    let state = vw_app::AppState::new().unwrap();
    let info = state.open_impl(root.join("index.html").to_str().unwrap(), None, None).unwrap();
    let url = state.html_preview_impl(&info.handle, Some("<!doctype html><h1>Draft</h1>".into()), 1).unwrap();
    let parsed = tauri::Url::parse(&url).unwrap();
    let (mime, bytes) = state.html_resource_impl(parsed.path()).unwrap();
    assert_eq!(mime, "text/html; charset=utf-8");
    let html = String::from_utf8(bytes).unwrap();
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains("<h1>Draft</h1>"));
    let token = parsed.path().trim_start_matches('/').split('/').next().unwrap();
    assert_eq!(state.html_resource_impl(&format!("/{token}/assets/site.css")).unwrap().1, b"h1 { color: red }");
    assert_eq!(state.html_resource_impl(&format!("/{token}/assets/logo.svg")).unwrap().0, "image/svg+xml");
    for path in ["../outside.css", "%2e%2e/outside.css", "assets/%2e%2e/%2e%2e/outside.css", "C:/private.css", "assets%5c..%5c..%5coutside.css", "secret.key", "index.html:stream"] {
        assert!(state.html_resource_impl(&format!("/{token}/{path}")).is_err(), "{path}");
    }
    let next = state.html_preview_impl(&info.handle, Some("<h1>Updated</h1>".into()), 3).unwrap();
    state.html_preview_impl(&info.handle, Some("<h1>Stale request</h1>".into()), 2).unwrap();
    assert_eq!(url, next);
    assert!(String::from_utf8(state.html_resource_impl(parsed.path()).unwrap().1).unwrap().contains("Updated"));
    state.close_impl(&info.handle).unwrap();
    assert!(state.html_resource_impl(parsed.path()).is_err());
    std::fs::write(root.join("note.txt"), "plain").unwrap();
    let text = state.open_impl(root.join("note.txt").to_str().unwrap(), None, None).unwrap();
    assert!(state.html_preview_impl(&text.handle, None, 1).is_err());
}

#[test]
fn raw_html_keeps_legacy_encoding_detection_and_decodes_utf16_for_the_bridge() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("html-preview-encoding");
    std::fs::create_dir_all(&root).unwrap();
    let gbk = b"<meta charset=gbk><h1>\xd6\xd0\xce\xc4</h1>";
    std::fs::write(root.join("index.html"), "<h1>Entry</h1>").unwrap();
    std::fs::write(root.join("gbk.html"), gbk).unwrap();
    let mut utf16 = vec![0xff, 0xfe];
    for unit in "<h1>中文</h1>".encode_utf16() { utf16.extend_from_slice(&unit.to_le_bytes()); }
    std::fs::write(root.join("utf16.html"), utf16).unwrap();
    let state = vw_app::AppState::new().unwrap();
    let info = state.open_impl(root.join("index.html").to_str().unwrap(), None, None).unwrap();
    let url = state.html_preview_impl(&info.handle, None, 1).unwrap();
    let parsed = tauri::Url::parse(&url).unwrap();
    let token = parsed.path().trim_start_matches('/').split('/').next().unwrap();
    let (mime, bytes) = state.html_resource_impl(&format!("/{token}/gbk.html")).unwrap();
    assert_eq!(mime, "text/html"); assert!(bytes.starts_with(gbk));
    let (mime, bytes) = state.html_resource_impl(&format!("/{token}/utf16.html")).unwrap();
    assert_eq!(mime, "text/html; charset=utf-8");
    assert!(String::from_utf8(bytes).unwrap().starts_with("<h1>中文</h1>"));
}
