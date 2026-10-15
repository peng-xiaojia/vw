use std::sync::Arc;
use vw_core::{Registry, Session};
use vw_plugin_image::ImageViewer;
use vw_protocol::{Block, Cursor, OpenOptions, PageRange, Source};

fn fixture(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("images");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

// Valid PNG header; the browser is responsible for decoding image pixels.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    bytes.extend(width.to_be_bytes());
    bytes.extend(height.to_be_bytes());
    bytes.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
    bytes
}

#[test]
fn png_is_readonly_and_blob_stays_within_document_session() {
    vw_core::set_tmp_override(Some(std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("image-sessions")));
    use image::ImageEncoder;
    let mut valid = Vec::new();
    image::codecs::png::PngEncoder::new(&mut valid).write_image(&[255; 24], 2, 3, image::ExtendedColorType::Rgba8).unwrap();
    let path = fixture("diagram.png", &valid);
    let mut registry = Registry::new();
    registry.register(Arc::new(ImageViewer::new(70)));
    let doc = Session::new(Arc::new(registry)).open(Source::path(&path), OpenOptions::default()).unwrap();
    assert_eq!(doc.viewer_id, "image");
    assert_eq!(doc.meta.mime.as_deref(), Some("image/png"));
    assert!(doc.edit_capability().is_none());
    assert!(doc.save_text("wrong".into()).is_err());
    let page = doc.page(PageRange { start: 0, count: 10 }).unwrap();
    assert_eq!(page.total, Some(1));
    assert!(matches!(&page.blocks[0], Block::Image { blob, w: Some(2), h: Some(3), .. } if blob == "image"));
    assert!(doc.page(PageRange { start: 1, count: 1 }).unwrap().blocks.is_empty());
    assert!(doc.search("x", Cursor::start()).unwrap().hits.is_empty());
    let blob = doc.blob("image").unwrap();
    assert_eq!(blob.mime.as_deref(), Some("image/png"));
    assert!(!std::path::Path::new(&blob.path).is_absolute());
    assert!(doc.blob("../escape").is_err());
}

#[test]
fn malicious_or_oversized_image_headers_are_rejected() {
    let mut registry = Registry::new();
    registry.register(Arc::new(ImageViewer::new(70)));
    let session = Session::new(Arc::new(registry));
    for (name, bytes) in [("script.png", b"<svg onload='alert(1)'/>".to_vec()), ("huge.png", png(100_000, 100_000)), ("zero.png", png(0, 2))] {
        let path = fixture(name, &bytes);
        assert!(session.open(Source::path(path), OpenOptions::default()).is_err(), "{name}");
    }
}
