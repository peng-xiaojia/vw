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
pub use session::{OpenedDoc, Session, SessionTempDir};
pub use viewer::{ArchiveSource, Doc, EditableDoc, Viewer};

pub use vw_protocol::PluginError;

/// 进程内日志目录覆盖（测试用；优先于 `VW_LOG_DIR`）。
static LOG_OVERRIDE: std::sync::RwLock<Option<std::path::PathBuf>> = std::sync::RwLock::new(None);

pub fn set_log_override(p: Option<std::path::PathBuf>) {
    *LOG_OVERRIDE.write().unwrap() = p;
}

/// 插件 stderr 日志根目录（设计规格 §9）：
/// 进程内 override > `VW_LOG_DIR` > 平台数据目录 `vw/logs/plugins`。
pub fn logs_root() -> std::path::PathBuf {
    if let Some(p) = LOG_OVERRIDE.read().unwrap().clone() {
        return p;
    }
    if let Ok(d) = std::env::var("VW_LOG_DIR") {
        if !d.is_empty() {
            return std::path::PathBuf::from(d);
        }
    }
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let mut p = std::path::PathBuf::from(home);
        if cfg!(windows) {
            p.push("AppData");
            p.push("Roaming");
        } else {
            p.push(".local");
            p.push("share");
        }
        p.push("vw");
        p.push("logs");
        p.push("plugins");
        return p;
    }
    std::path::PathBuf::from(".vw/logs/plugins")
}

/// `YYYYMMDD`（民用历换算，无外部依赖），用于按日期分文件。
pub fn date_stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}{m:02}{d:02}")
}

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
