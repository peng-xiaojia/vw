//! conformance（stdio over 真实子进程）：端到端进程生命周期。

use std::sync::Arc;

use vw_core::{Registry, Session};
use vw_plugin_host::{HostConfig, HostViewer};
use vw_protocol::{Manifest, OpenOptions, Source};

#[test]
fn sidecar_process_passes_conformance_end_to_end() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cf-sub");
    let _ = std::fs::remove_dir_all(&dir);
    vw_core::set_tmp_override(Some(std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("tmp-sub")));

    let exe = env!("CARGO_BIN_EXE_vw-example-sidecar");
    let manifest = Manifest::simple_stdio("vw.example", "textlog", &["log"], &["view", "edit"], 40, exe);
    let host = HostViewer::from_manifest(manifest, HostConfig::default()).unwrap();
    let mut reg = Registry::new();
    reg.register(Arc::new(host));
    let session = Session::new(Arc::new(reg));
    let open = |p: &std::path::Path| session.open(Source::path(p), OpenOptions::default());
    vw_conformance::run_text_suite(&open, &dir, "log").unwrap();
}
