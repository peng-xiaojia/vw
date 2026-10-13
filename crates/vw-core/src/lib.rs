//! vw 内核：零格式知识。
//!
//! 分层（设计规格 §4.1）：registry / doc / session / policy / render。
//! 本 crate 不依赖任何 GUI 框架，不含任何具体格式的解析代码。

pub mod policy;
pub mod registry;
pub mod render;
pub mod session;
pub mod viewer;

pub use policy::{atomic_write, capture_baseline, validate_source, Backup, Baseline, SaveReport};
pub use registry::Registry;
pub use session::{OpenedDoc, Session};
pub use viewer::{ArchiveSource, Doc, EditableDoc, Viewer};

pub use vw_protocol::PluginError;

/// 进程内临时目录覆盖（测试与嵌入用；优先于 `VW_TMP_DIR`）。
/// 用显式 override 而非测试内 set_var：Windows 上并行改/读环境变量会死锁。
static TMP_OVERRIDE: std::sync::RwLock<Option<std::path::PathBuf>> = std::sync::RwLock::new(None);

pub fn set_tmp_override(p: Option<std::path::PathBuf>) {
    *TMP_OVERRIDE.write().unwrap() = p;
}

/// 会话临时根目录：进程内 override > `VW_TMP_DIR` > 系统临时目录下 `vw/`。
/// blob、归档成员物化等一切临时产物都必须落在其中（设计规格 §5.3）。
pub fn tmp_root() -> std::path::PathBuf {
    if let Some(p) = TMP_OVERRIDE.read().unwrap().clone() {
        return p;
    }
    if let Ok(d) = std::env::var("VW_TMP_DIR") {
        if !d.is_empty() {
            return std::path::PathBuf::from(d);
        }
    }
    std::env::temp_dir().join("vw")
}
