//! §9 诊断：插件 stderr 必须按插件 id 与日期落盘。

use std::sync::Arc;

use vw_core::{Registry, Session};
use vw_plugin_host::{HostConfig, HostViewer};
use vw_protocol::{Manifest, OpenOptions, Source};

#[test]
fn sidecar_stderr_is_logged_to_disk() {
    let logs = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("stderr-logs");
    let _ = std::fs::remove_dir_all(&logs);
    vw_core::set_log_override(Some(logs.clone()));
    vw_core::set_tmp_override(Some(std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("stderr-tmp")));

    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("stderr");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("x.log");
    std::fs::write(&f, b"hello\n").unwrap();

    std::env::set_var("VW_EMIT_STDERR", "1");
    let exe = env!("CARGO_BIN_EXE_vw-example-sidecar");
    let manifest = Manifest::simple_stdio("vw.example", "textlog", &["log"], &["view", "edit"], 40, exe);
    let host = HostViewer::from_manifest(manifest, HostConfig::default()).unwrap();
    let mut reg = Registry::new();
    reg.register(Arc::new(host));
    let session = Session::new(Arc::new(reg));
    let _doc = session.open(Source::path(&f), OpenOptions::default()).unwrap();

    // stderr 在 sidecar 启动即打印；等宿主捕获线程落盘
    std::thread::sleep(std::time::Duration::from_millis(500));

    let today = vw_core::date_stamp();
    let log_path = logs.join(format!("vw.example-{today}.log"));
    assert!(log_path.is_file(), "日志文件应存在：{}", log_path.display());
    let content = std::fs::read_to_string(&log_path).unwrap();
    assert!(content.contains("stderr-boom-marker"), "{content}");
    assert!(content.contains("会话开始"), "{content}");
}
