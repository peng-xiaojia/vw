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
pub use viewer::{Doc, EditableDoc, Viewer};

pub use vw_protocol::PluginError;
