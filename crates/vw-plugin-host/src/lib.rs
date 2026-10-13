//! vw 外置插件宿主：sidecar 进程管理与 stdio JSON-RPC。

pub mod conn;
pub mod viewer;


pub use viewer::{platform_tag, tmp_root, HostConfig, HostDiagnostics, HostViewer};
