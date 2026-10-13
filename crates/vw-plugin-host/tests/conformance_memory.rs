//! conformance（stdio over 内存管道）：同一插件逻辑经宿主消息层。
//! 另含安全契约测试：blob 路径越界必须被拒绝；超时必须 cancel + 强杀。

use std::sync::Arc;
use std::time::Duration;

use vw_core::{Registry, Session};
use vw_example_sidecar::SidecarText;
use vw_plugin_host::{HostConfig, HostViewer};
use vw_protocol::{
    BlobRef, Block, BlockList, Cursor, DocMeta, EditCapability, EditSnapshot, ErrorKind, Manifest, OpenOptions,
    Outline, PageRange, PluginError, ProbeScore, SearchHits, Source,
};
use vw_plugin_sdk::{CancelFlag, RpcDoc, RpcViewer};

fn registry_with_host(host: HostViewer) -> Arc<Session> {
    let mut reg = Registry::new();
    reg.register(Arc::new(host));
    Arc::new(Session::new(Arc::new(reg)))
}

#[test]
fn sidecar_logic_passes_conformance_over_memory_pipe() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cf-mem");
    let _ = std::fs::remove_dir_all(&dir);
    vw_core::set_tmp_override(Some(std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("tmp-mem")));

    let manifest = Manifest::simple_stdio("vw.example", "textlog", &["log"], &["view", "edit"], 40, "unused");
    let host = HostViewer::from_memory(manifest, Arc::new(SidecarText), HostConfig::default());
    let session = registry_with_host(host);
    let open = |p: &std::path::Path| session.open(Source::path(p), OpenOptions::default());
    vw_conformance::run_text_suite(&open, &dir, "log").unwrap();
}

// ---------- 安全：blob 路径越界 ----------

struct RogueBlob;

impl RpcViewer for RogueBlob {
    fn probe(&self, _source: &Source) -> Option<ProbeScore> {
        Some(ProbeScore { score: 50, reason: "rogue".into() })
    }

    fn open(
        &self,
        _source: &Source,
        _opts: &OpenOptions,
        _tmp_dir: &std::path::Path,
        _cancel: CancelFlag,
    ) -> Result<Box<dyn RpcDoc>, PluginError> {
        Ok(Box::new(RogueBlobDoc))
    }
}

struct RogueBlobDoc;

impl RpcDoc for RogueBlobDoc {
    fn meta(&self) -> DocMeta {
        DocMeta { viewer: "rogue".into(), ..Default::default() }
    }

    fn page(&self, _range: PageRange, _cancel: CancelFlag) -> Result<BlockList, PluginError> {
        Ok(BlockList { blocks: vec![Block::para("hi")], total: Some(1) })
    }

    fn search(&self, _q: &str, _from: Cursor, _cancel: CancelFlag) -> Result<SearchHits, PluginError> {
        Ok(vw_protocol::SearchHits { hits: vec![], next: None })
    }

    fn blob(&self, _id: &str) -> Result<BlobRef, PluginError> {
        // 恶意/被劫持插件尝试借 blob 通道让核心读任意文件
        Ok(BlobRef { path: "../secret.txt".into(), mime: None, size: None })
    }
}

#[test]
fn blob_path_escape_is_rejected() {
    vw_core::set_tmp_override(Some(std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("tmp-rogue")));
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("rogue");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("r.log");
    std::fs::write(&f, b"x").unwrap();

    let manifest = Manifest::simple_stdio("vw.rogue", "rogue", &["log"], &["view"], 90, "unused");
    let host = HostViewer::from_memory(manifest, Arc::new(RogueBlob), HostConfig::default());
    let session = registry_with_host(host);
    let doc = session.open(Source::path(&f), OpenOptions::default()).unwrap();
    let err = match doc.blob("anything") {
        Err(e) => e,
        Ok(_) => panic!("越界 blob 路径必须被拒绝"),
    };
    assert_eq!(err.kind, ErrorKind::PermissionDenied);
}

// ---------- 健壮性：挂起请求超时 → cancel → 强杀 ----------

struct SlowViewer;

impl RpcViewer for SlowViewer {
    fn probe(&self, _source: &Source) -> Option<ProbeScore> {
        Some(ProbeScore { score: 50, reason: "slow".into() })
    }

    fn open(
        &self,
        _source: &Source,
        _opts: &OpenOptions,
        _tmp_dir: &std::path::Path,
        _cancel: CancelFlag,
    ) -> Result<Box<dyn RpcDoc>, PluginError> {
        Ok(Box::new(SlowDoc))
    }
}

struct SlowDoc;

impl RpcDoc for SlowDoc {
    fn meta(&self) -> DocMeta {
        DocMeta { viewer: "slow".into(), ..Default::default() }
    }

    fn page(&self, _range: PageRange, cancel: CancelFlag) -> Result<BlockList, PluginError> {
        // 忽略取消标志，模拟卡死
        for _ in 0..40 {
            if cancel.load(std::sync::atomic::Ordering::SeqCst) {
                // 依然不返回：验证宿主的强杀路径
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(BlockList { blocks: vec![], total: Some(0) })
    }

    fn search(&self, _q: &str, _from: Cursor, _cancel: CancelFlag) -> Result<SearchHits, PluginError> {
        Ok(vw_protocol::SearchHits { hits: vec![], next: None })
    }
}

#[test]
fn hung_request_times_out_and_process_gets_killed() {
    vw_core::set_tmp_override(Some(std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("tmp-slow")));
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("slow");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("s.log");
    std::fs::write(&f, b"x").unwrap();

    let manifest = Manifest::simple_stdio("vw.slow", "slow", &["log"], &["view"], 90, "unused");
    let cfg = HostConfig { request_timeout: Duration::from_millis(200), ..Default::default() };
    let host = HostViewer::from_memory(manifest, Arc::new(SlowViewer), cfg);
    let session = registry_with_host(host);
    let doc = session.open(Source::path(&f), OpenOptions::default()).unwrap();

    let start = std::time::Instant::now();
    let err = match doc.page(PageRange { start: 0, count: 10 }) {
        Err(e) => e,
        Ok(_) => panic!("挂起请求应超时"),
    };
    assert_eq!(err.kind, ErrorKind::Timeout);
    // 超时 = 请求超时 + 1s cancel 宽限，而非等满 SlowDoc 的 4s
    assert!(start.elapsed() < Duration::from_secs(3), "应主动强杀而不是干等：{:?}", start.elapsed());
}
