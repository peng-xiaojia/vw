#![cfg(windows)]
use std::sync::Arc;
use vw_core::{Registry, Session};
use vw_plugin_pdf::PdfViewer;
use vw_protocol::{Block, OpenOptions, PageRange, Source};

fn fixture() -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("pdf");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("one page.pdf");
    let objects = ["<< /Type /Catalog /Pages 2 0 R >>", "<< /Type /Pages /Kids [3 0 R] /Count 1 >>", "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << >> /Contents 4 0 R >>", "<< /Length 16 >>\nstream\n0 0 100 100 re f\nendstream"];
    let mut pdf = "%PDF-1.4\n".to_string();
    let mut offsets = vec![0];
    for (index, body) in objects.iter().enumerate() { offsets.push(pdf.len()); pdf.push_str(&format!("{} 0 obj\n{}\nendobj\n", index + 1, body)); }
    let xref = pdf.len();
    pdf.push_str("xref\n0 5\n0000000000 65535 f \n");
    for offset in offsets.iter().skip(1) { pdf.push_str(&format!("{offset:010} 00000 n \n")); }
    pdf.push_str(&format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"));
    std::fs::write(&path, pdf).unwrap();
    path
}

#[test]
fn pdf_page_renders_to_scoped_png_and_is_readonly() {
    vw_core::set_tmp_override(Some(std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("pdf-sessions")));
    let mut registry = Registry::new(); registry.register(Arc::new(PdfViewer::new(80)));
    let doc = Session::new(Arc::new(registry)).open(Source::path(fixture()), OpenOptions::default()).unwrap();
    assert_eq!(doc.meta.pages, Some(1)); assert!(doc.edit_capability().is_none());
    let page = doc.page(PageRange { start: 0, count: 1 }).unwrap();
    assert!(matches!(&page.blocks[0], Block::Image { blob, .. } if blob == "page-0"));
    let (blob, path) = doc.resolve_blob("page-0").unwrap();
    assert_eq!(blob.mime.as_deref(), Some("image/png"));
    assert!(std::fs::read(path).unwrap().starts_with(b"\x89PNG\r\n\x1a\n"));
    assert!(doc.resolve_blob("page-1").is_err()); assert!(doc.resolve_blob("../outside").is_err());
}
