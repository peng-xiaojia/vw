use std::fs;
use std::time::Duration;

use vw_core::policy::{atomic_write, capture_baseline, validate_source, Backup};
use vw_core::registry::{viewer_id_of, Registry};
use vw_core::session::Session;
use vw_core::viewer::{Doc, EditableDoc, Viewer};
use vw_protocol::{
    Block, Cursor, DocMeta, EditCapability, ErrorKind, Manifest, OpenOptions, PageRange,
    PluginError, ProbeScore, SearchHits, Source,
};

/// 测试用最小文本 viewer：模拟一个「内置插件」的完整行为。
struct FakeText {
    manifest: Manifest,
}

fn fake_manifest(id: &str, priority: i64) -> Manifest {
    serde_json::from_value(serde_json::json!({
        "manifest_version": 1,
        "id": id,
        "name": id,
        "version": "0.1.0",
        "transport": { "kind": "in-process" },
        "contributes": { "viewers": [ { "id": id, "extensions": ["txt"], "capabilities": ["view", "edit"] } ] },
        "priority": priority,
    }))
    .unwrap()
}

impl Viewer for FakeText {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn probe(&self, _source: &Source) -> Option<ProbeScore> {
        Some(ProbeScore {
            score: 50,
            reason: "扩展名匹配".into(),
        })
    }

    fn open(&self, source: &Source, _opts: &OpenOptions) -> Result<Box<dyn Doc>, PluginError> {
        let path = source.as_fs_path().unwrap();
        let bytes = fs::read(path)?;
        let text = String::from_utf8_lossy(&bytes).to_string();
        Ok(Box::new(FakeDoc {
            text,
            root: path.parent().unwrap().to_path_buf(),
        }))
    }
}

struct FakeDoc {
    text: String,
    root: std::path::PathBuf,
}

impl Doc for FakeDoc {
    fn meta(&self) -> DocMeta {
        DocMeta {
            viewer: "fake".into(),
            size: Some(self.text.len() as u64),
            ..Default::default()
        }
    }

    fn page(&self, _range: PageRange) -> Result<vw_protocol::BlockList, PluginError> {
        let blocks = vec![Block::preformatted(self.text.clone())];
        Ok(vw_protocol::BlockList {
            blocks,
            total: Some(1),
        })
    }

    fn search(&self, query: &str, from: Cursor) -> Result<SearchHits, PluginError> {
        Ok(vw_protocol::search_blocks(
            &[Block::preformatted(self.text.clone())],
            query,
            from,
        ))
    }

    fn as_editable(&self) -> Option<&dyn EditableDoc> {
        Some(self)
    }
    fn blob_root(&self) -> Option<std::path::PathBuf> {
        Some(self.root.clone())
    }
    fn blob(&self, id: &str) -> Result<vw_protocol::BlobRef, PluginError> {
        Ok(vw_protocol::BlobRef {
            path: id.to_string(),
            mime: None,
            size: None,
        })
    }
}

impl EditableDoc for FakeDoc {
    fn edit_capability(&self) -> EditCapability {
        EditCapability::Text {
            encoding: "utf-8".into(),
            line_ending: vw_protocol::LineEnding::Preserve,
            has_bom: false,
            max_bytes: None,
        }
    }

    fn snapshot(&self) -> Result<vw_protocol::EditSnapshot, PluginError> {
        Ok(vw_protocol::EditSnapshot::text(self.text.clone()))
    }

    fn serialize(&self, edited: &vw_protocol::EditSnapshot) -> Result<Vec<u8>, PluginError> {
        match edited {
            vw_protocol::EditSnapshot::Text { text } => Ok(text.clone().into_bytes()),
        }
    }
}

fn write_file(dir: &std::path::Path, name: &str, content: &[u8]) -> std::path::PathBuf {
    let p = dir.join(name);
    fs::write(&p, content).unwrap();
    p
}

fn registry_with(id: &str, priority: i64) -> Registry {
    let mut r = Registry::new();
    r.register(std::sync::Arc::new(FakeText {
        manifest: fake_manifest(id, priority),
    }));
    r
}

#[test]
fn validate_source_rejects_missing_and_accepts_dir() {
    let tmp = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("vw-core-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    assert_eq!(
        validate_source(&Source::path(tmp.join("nope.txt")))
            .unwrap_err()
            .kind,
        ErrorKind::MalformedSource
    );
    // P2：目录源合法
    assert!(validate_source(&Source::Dir { path: tmp.clone() }).is_ok());
    let f = write_file(&tmp, "ok.txt", b"hi");
    assert!(validate_source(&Source::path(&f)).is_ok());
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn atomic_write_is_atomic_and_backs_up() {
    let tmp = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("vw-core-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let f = write_file(&tmp, "a.txt", b"v1");
    let report = atomic_write(&f, b"v2-longer", Backup::Bak).unwrap();
    assert_eq!(fs::read(&f).unwrap(), b"v2-longer");
    assert_eq!(fs::read(f.with_extension("txt.bak")).unwrap(), b"v1");
    assert!(report.backup.is_some());
    // 临时文件不留残留
    let leftovers: Vec<_> = fs::read_dir(&tmp)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.contains(".tmp-"))
        .collect();
    assert!(leftovers.is_empty(), "残留临时文件：{leftovers:?}");
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn baseline_detects_external_modification() {
    let tmp = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("vw-core-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let f = write_file(&tmp, "b.txt", b"original");
    let baseline = capture_baseline(f.clone()).unwrap();
    assert!(baseline.check_unchanged().is_ok());

    // 外部篡改 → 冲突
    std::thread::sleep(Duration::from_millis(10));
    fs::write(&f, b"tampered").unwrap();
    let err = baseline.check_unchanged().unwrap_err();
    assert_eq!(err.kind, ErrorKind::Conflict);

    // 仅 touch（内容相同）→ 放行
    let f2 = write_file(&tmp, "c.txt", b"same");
    let bl2 = capture_baseline(f2.clone()).unwrap();
    let h1 = bl2.hash_hex().unwrap();
    // 重写相同内容并修改 mtime
    std::thread::sleep(Duration::from_millis(10));
    fs::write(&f2, b"same").unwrap();
    assert!(bl2.check_unchanged().is_ok());
    assert_eq!(h1.len(), 64, "BLAKE3 hex 长度");
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn baseline_rejects_same_size_change_even_when_modified_time_is_preserved() {
    let path = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("preserved-mtime-{}.txt", uuid::Uuid::new_v4()));
    fs::write(&path, b"original").unwrap();
    let baseline = capture_baseline(path.clone()).unwrap();
    let snap = baseline.snapshot().unwrap();
    fs::write(&path, b"tampered").unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(snap.mtime))
        .unwrap();
    assert_eq!(
        baseline.check_unchanged().unwrap_err().kind,
        ErrorKind::Conflict
    );
    fs::remove_file(path).unwrap();
}

#[test]
fn registry_arbitration_and_override() {
    let tmp = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("vw-core-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let f = write_file(&tmp, "d.txt", b"x");

    // 单插件命中
    let r = registry_with("a", 50);
    let s = Session::new(std::sync::Arc::new(r));
    let opened = s.open(Source::path(&f), OpenOptions::default()).unwrap();
    assert_eq!(opened.viewer_id, "a");

    // 并列 → AmbiguousViewer
    let mut r2 = Registry::new();
    r2.register(std::sync::Arc::new(FakeText {
        manifest: fake_manifest("p1", 50),
    }));
    r2.register(std::sync::Arc::new(FakeText {
        manifest: fake_manifest("p2", 50),
    }));
    let s2 = Session::new(std::sync::Arc::new(r2));
    let err = s2
        .open(Source::path(&f), OpenOptions::default())
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::AmbiguousViewer);
    assert!(err.message.contains("p1") && err.message.contains("p2"));

    // priority 打破平手
    let mut r3 = Registry::new();
    r3.register(std::sync::Arc::new(FakeText {
        manifest: fake_manifest("p1", 50),
    }));
    r3.register(std::sync::Arc::new(FakeText {
        manifest: fake_manifest("p2", 60),
    }));
    let s3 = Session::new(std::sync::Arc::new(r3));
    let opened3 = s3.open(Source::path(&f), OpenOptions::default()).unwrap();
    assert_eq!(opened3.viewer_id, "p2");

    // --as 覆盖
    let opened4 = s3
        .open(
            Source::path(&f),
            OpenOptions {
                as_viewer: Some("p1".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(opened4.viewer_id, "p1");
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn save_pipeline_conflict_and_success() {
    let tmp = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("vw-core-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let f = write_file(&tmp, "e.txt", b"hello");

    let s = Session::new(std::sync::Arc::new(registry_with("a", 50)));
    let opened = s.open(Source::path(&f), OpenOptions::default()).unwrap();

    // 正常保存
    let report = opened.save_text("hello edited\n".into()).unwrap();
    assert_eq!(fs::read(&f).unwrap(), b"hello edited\n");
    assert_eq!(report.bytes, 13);
    // 基线已更新：再次保存不冲突
    opened.save_text("again".into()).unwrap();
    assert_eq!(fs::read(&f).unwrap(), b"again");

    // 外部篡改后保存 → Conflict，原文件保持被篡改的内容（不被覆盖）
    std::thread::sleep(Duration::from_millis(10));
    fs::write(&f, b"external").unwrap();
    let err = opened.save_text("should fail".into()).unwrap_err();
    assert_eq!(err.kind, ErrorKind::Conflict);
    assert_eq!(fs::read(&f).unwrap(), b"external");
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn viewer_ids_sort_stable() {
    let mut r = Registry::new();
    r.register(std::sync::Arc::new(FakeText {
        manifest: fake_manifest("zz", 50),
    }));
    r.register(std::sync::Arc::new(FakeText {
        manifest: fake_manifest("aa", 50),
    }));
    assert_eq!(viewer_id_of(&r.viewers()[0]), "zz");
}

#[test]
fn save_refreshes_snapshot_pages_and_search() {
    let tmp = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("save-refresh-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let path = write_file(&tmp, "fresh.txt", b"old");
    let session = Session::new(std::sync::Arc::new(registry_with("a", 50)));
    let doc = session
        .open(Source::path(&path), OpenOptions::default())
        .unwrap();
    doc.save_text("new content".into()).unwrap();
    assert_eq!(doc.snapshot_text().unwrap(), "new content");
    assert_eq!(
        vw_protocol::block_text(&doc.page(PageRange { start: 0, count: 1 }).unwrap().blocks[0])
            .unwrap(),
        "new content"
    );
    assert!(doc.search("old", Cursor::start()).unwrap().hits.is_empty());
    assert_eq!(doc.search("new", Cursor::start()).unwrap().hits.len(), 1);
    assert_eq!(doc.current_meta().size, Some(11));
    drop(doc);
    fs::remove_dir_all(tmp).unwrap();
}

#[test]
fn concurrent_saves_publish_matching_snapshot_and_baseline() {
    let tmp = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("concurrent-save-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let path = write_file(&tmp, "shared.txt", b"initial");
    let session = Session::new(std::sync::Arc::new(registry_with("a", 50)));
    let doc = std::sync::Arc::new(
        session
            .open(Source::path(&path), OpenOptions::default())
            .unwrap(),
    );
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let doc = doc.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                for revision in 0..10 {
                    doc.save_text(format!("{i}:{revision}")).unwrap();
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    let bytes = fs::read(&path).unwrap();
    assert_eq!(doc.snapshot_text().unwrap().as_bytes(), bytes);
    assert_eq!(
        doc.baseline_hash().unwrap().unwrap(),
        blake3::hash(&bytes).to_string()
    );
    assert!(path.with_extension("txt.bak").is_file());
    drop(doc);
    fs::remove_dir_all(tmp).unwrap();
}

#[test]
fn core_blob_resolution_checks_relative_paths_and_declared_root() {
    let trusted = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("core-resource-root");
    vw_core::set_tmp_override(Some(trusted.clone()));
    let tmp = trusted.join(format!("core-blob-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let path = write_file(&tmp, "content.txt", b"blob bytes");
    let session = Session::new(std::sync::Arc::new(registry_with("a", 50)));
    let doc = session
        .open(Source::path(&path), OpenOptions::default())
        .unwrap();
    assert_eq!(
        fs::read(doc.resolve_blob("content.txt").unwrap().1).unwrap(),
        b"blob bytes"
    );
    for invalid in [
        "../content.txt",
        "..\\content.txt",
        "",
        path.to_str().unwrap(),
    ] {
        assert_eq!(
            doc.resolve_blob(invalid).unwrap_err().kind,
            ErrorKind::PermissionDenied
        );
    }
    let rogue_root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("outside-resources-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&rogue_root).unwrap();
    let rogue_path = write_file(&rogue_root, "secret.txt", b"private");
    let rogue = session
        .open(Source::path(&rogue_path), OpenOptions::default())
        .unwrap();
    assert_eq!(
        rogue.resolve_blob("secret.txt").unwrap_err().kind,
        ErrorKind::PermissionDenied,
        "plugin must not declare an arbitrary local resource root"
    );
    drop(rogue);
    fs::remove_dir_all(rogue_root).unwrap();
    drop(doc);
    fs::remove_dir_all(tmp).unwrap();
}

struct CloseViewer {
    manifest: Manifest,
    closes: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}
struct CloseDoc(std::sync::Arc<std::sync::atomic::AtomicUsize>);
impl Viewer for CloseViewer {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    fn probe(&self, _: &Source) -> Option<ProbeScore> {
        Some(ProbeScore {
            score: 100,
            reason: "close".into(),
        })
    }
    fn open(&self, _: &Source, _: &OpenOptions) -> Result<Box<dyn Doc>, PluginError> {
        Ok(Box::new(CloseDoc(self.closes.clone())))
    }
}
impl Doc for CloseDoc {
    fn meta(&self) -> DocMeta {
        DocMeta::default()
    }
    fn page(&self, _: PageRange) -> Result<vw_protocol::BlockList, PluginError> {
        Ok(vw_protocol::BlockList {
            blocks: vec![],
            total: Some(0),
        })
    }
    fn search(&self, _: &str, _: Cursor) -> Result<SearchHits, PluginError> {
        Ok(SearchHits {
            hits: vec![],
            next: None,
        })
    }
    fn close(&self) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

#[test]
fn explicit_close_and_drop_close_plugin_exactly_once() {
    let path = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("close-once-{}.txt", uuid::Uuid::new_v4()));
    fs::write(&path, b"source").unwrap();
    let closes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut reg = Registry::new();
    reg.register(std::sync::Arc::new(CloseViewer {
        manifest: fake_manifest("close", 100),
        closes: closes.clone(),
    }));
    let session = Session::new(std::sync::Arc::new(reg));
    let doc = session
        .open(Source::path(&path), OpenOptions::default())
        .unwrap();
    doc.close();
    doc.close();
    drop(doc);
    assert_eq!(closes.load(std::sync::atomic::Ordering::SeqCst), 1);
    fs::remove_file(path).unwrap();
}

struct FakeArchive {
    manifest: Manifest,
}
impl Viewer for FakeArchive {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    fn probe(&self, source: &Source) -> Option<ProbeScore> {
        matches!(source, Source::Archive { .. }).then(|| ProbeScore {
            score: 100,
            reason: "archive".into(),
        })
    }
    fn open(&self, _: &Source, _: &OpenOptions) -> Result<Box<dyn Doc>, PluginError> {
        unreachable!()
    }
    fn as_archive(&self) -> Option<&dyn vw_core::ArchiveSource> {
        Some(self)
    }
}
impl vw_core::ArchiveSource for FakeArchive {
    fn materialize(
        &self,
        _: &std::path::Path,
        _: &str,
        dest: &std::path::Path,
    ) -> Result<std::path::PathBuf, PluginError> {
        let path = dest.join("member.txt");
        fs::write(&path, b"archive bytes")?;
        Ok(path)
    }
}

#[test]
fn archive_members_are_readonly_and_owned_temp_is_removed() {
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("core-resource-root");
    vw_core::set_tmp_override(Some(root.clone()));
    let mut reg = registry_with("a", 50);
    reg.register(std::sync::Arc::new(FakeArchive {
        manifest: fake_manifest("archive", 100),
    }));
    let session = Session::new(std::sync::Arc::new(reg));
    let doc = session
        .open_member(
            std::path::Path::new("outer.zip"),
            "member.txt",
            OpenOptions::default(),
        )
        .unwrap();
    let materialized = doc.path.clone();
    assert!(
        doc.edit_capability().is_none(),
        "archive member must not advertise edits"
    );
    assert!(doc.save_text("fake save".into()).is_err());
    assert_eq!(fs::read(&materialized).unwrap(), b"archive bytes");
    assert!(!doc
        .page(PageRange { start: 0, count: 1 })
        .unwrap()
        .blocks
        .is_empty());
    drop(doc);
    assert!(
        !materialized.exists(),
        "member lifetime must own its temp directory"
    );
}
