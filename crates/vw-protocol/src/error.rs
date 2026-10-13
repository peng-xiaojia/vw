//! 统一错误模型（设计规格 §9）。两种 transport 的错误形状完全一致：
//! `{"kind": "...", "message": "..."}`。

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// 没有插件认领该文件。
    NoViewer,
    /// 多个插件打分与优先级并列，需要用户 `--as` 仲裁（设计规格 §7.2）。
    AmbiguousViewer,
    PluginCrashed,
    Timeout,
    Cancelled,
    Unsupported,
    MalformedSource,
    PermissionDenied,
    /// 保存前检测到文件已被外部修改。
    Conflict,
    /// 插件在校验阶段拒绝保存。
    SaveRejected,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginError {
    pub kind: ErrorKind,
    pub message: String,
}

impl PluginError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into() }
    }

    pub fn no_viewer(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::NoViewer, message)
    }

    pub fn ambiguous(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::AmbiguousViewer, message)
    }

    pub fn crashed(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::PluginCrashed, message)
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Timeout, message)
    }

    pub fn cancelled() -> Self {
        Self::new(ErrorKind::Cancelled, "请求已取消")
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unsupported, message)
    }

    pub fn malformed(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::MalformedSource, message)
    }

    pub fn permission(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::PermissionDenied, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Conflict, message)
    }

    pub fn save_rejected(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::SaveRejected, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message)
    }
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{:?}] {}", self.kind, self.message)
    }
}

impl std::error::Error for PluginError {}

impl From<std::io::Error> for PluginError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::NotFound => Self::malformed(format!("文件不存在：{e}")),
            std::io::ErrorKind::PermissionDenied => Self::permission(format!("无权访问：{e}")),
            _ => Self::internal(format!("IO 错误：{e}")),
        }
    }
}

impl From<serde_json::Error> for PluginError {
    fn from(e: serde_json::Error) -> Self {
        Self::internal(format!("JSON 编解码错误：{e}"))
    }
}
