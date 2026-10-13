//! conformance（in-process）：内置 text 插件直调路径。

use std::sync::Arc;

use vw_core::{Registry, Session};
use vw_plugin_text::TextViewer;
use vw_protocol::{OpenOptions, Source};

#[test]
fn builtin_text_passes_conformance_in_process() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cf-inproc");
    let _ = std::fs::remove_dir_all(&dir);

    let mut reg = Registry::new();
    reg.register(Arc::new(TextViewer::new(&["txt"], 50)));
    let session = Session::new(Arc::new(reg));
    let open = |p: &std::path::Path| session.open(Source::path(p), OpenOptions::default());
    vw_conformance::run_text_suite(&open, &dir, "txt").unwrap();
}
