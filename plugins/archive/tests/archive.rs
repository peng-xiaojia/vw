use std::io::Write;

use vw_core::viewer::{ArchiveSource, Viewer};
use vw_plugin_archive::ArchiveViewer;
use vw_protocol::{ErrorKind, Manifest, OpenOptions, PageRange, Source};
use vw_core::{Registry, Session};

fn make_zip(name: &str, entries: &[(&str, &[u8])]) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("vw-archive-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let f = std::fs::File::create(&path).unwrap();
    let mut zw = zip::ZipWriter::new(f);
    for (name, content) in entries {
        zw.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        zw.write_all(content).unwrap();
    }
    zw.finish().unwrap();
    path
}

fn registry_with(priority: i64) -> Arc<Session> {
    let mut reg = Registry::new();
    reg.register(std::sync::Arc::new(ArchiveViewer::new(priority)));
    std::sync::Arc::new(Session::new(std::sync::Arc::new(reg)))
}
use std::sync::Arc;

#[test]
fn zip_lists_members_with_caps() {
    let z = make_zip("a.zip", &[("hello.txt", b"hi".as_ref()), ("sub/inner.md", b"# inner".as_ref())]);
    let v = ArchiveViewer::new(55);
    let doc = v.open(&Source::path(&z), &OpenOptions::default()).unwrap();
    assert_eq!(doc.meta().mime.as_deref(), Some("application/zip"));
    let page = doc.page(PageRange { start: 0, count: 10 }).unwrap();
    let text = page.blocks.iter().filter_map(vw_protocol::block_text).collect::<Vec<_>>().join("\n");
    assert!(text.contains("hello.txt") && text.contains("sub/inner.md"), "{text}");
    // 魔数嗅探
    assert!(v.probe(&Source::path(&z)).is_some());
}

#[test]
fn member_materialization_reprobes_and_opens() {
    let z = make_zip("b.zip", &[("hello.txt", b"hello from inside\n".as_ref()), ("sub/inner.md", b"# inner doc\n".as_ref())]);
    // 注册 archive + text + markdown：物化后按扩展名重新 probe
    let mut reg = Registry::new();
    reg.register(Arc::new(ArchiveViewer::new(55)));
    reg.register(Arc::new(vw_plugin_text::TextViewer::new(&["txt"], 50)));
    reg.register(Arc::new(vw_plugin_markdown::MarkdownViewer::new(&["md"], 50)));
    let session = Arc::new(Session::new(Arc::new(reg)));
    vw_core::set_tmp_override(Some(std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("tmp-archive")));

    let doc = session.open_member(&z, "hello.txt", OpenOptions::default()).unwrap();
    assert_eq!(doc.viewer_id, "text", "物化后应被 text 插件认领");
    let all = doc.page_all(100).unwrap();
    let text = all.blocks.iter().filter_map(vw_protocol::block_text).collect::<String>();
    assert!(text.contains("hello from inside"), "{text}");

    let doc2 = session.open_member(&z, "sub/inner.md", OpenOptions::default()).unwrap();
    assert_eq!(doc2.viewer_id, "markdown");
    // 溯源信息保留
    assert!(matches!(doc2.source, Source::Archive { .. }));
}

#[test]
fn missing_member_errors_clearly() {
    let z = make_zip("c.zip", &[("hello.txt", b"hi".as_ref())]);
    let session = registry_with(55);
    vw_core::set_tmp_override(Some(std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("tmp-archive")));
    let err = session.open_member(&z, "nope.txt", OpenOptions::default()).unwrap_err();
    assert_eq!(err.kind, ErrorKind::MalformedSource);
    assert!(err.message.contains("nope.txt"));
}

#[test]
fn zip_bomb_guards() {
    // 成员数上限
    let entries: Vec<(&str, &[u8])> = (0..5).map(|i| (format!("f{i}.txt").leak() as &str, b"x".as_ref())).collect();
    let z1 = make_zip("d.zip", &entries);
    let v = ArchiveViewer::with_caps(55, 3, u64::MAX, u64::MAX);
    let err = match v.open(&Source::path(&z1), &OpenOptions::default()) {
        Err(e) => e,
        Ok(_) => panic!("超成员数上限应失败"),
    };
    assert!(err.message.contains("成员数"), "{err}");

    // 单成员解压上限（写入端声明 17 字节 > 上限 4）：在物化时检查
    let z2 = make_zip("e.zip", &[("hello.txt", b"hello from inside".as_ref())]);
    let v2 = ArchiveViewer::with_caps(55, 100, u64::MAX, 4);
    let dest = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("vw-archive-test").join("dest");
    std::fs::create_dir_all(&dest).unwrap();
    let err2 = match v2.materialize(&z2, "hello.txt", &dest) {
        Err(e) => e,
        Ok(_) => panic!("超单成员解压上限应失败"),
    };
    assert!(err2.message.contains("成员解压") || err2.message.contains("解压量"), "{err2}");
}
