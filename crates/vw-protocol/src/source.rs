//! `Source`：可组合的查看目标（设计规格 §5.1）。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    Path { path: PathBuf },
    Dir { path: PathBuf },
    /// 归档内的成员：外层源 + 内部路径（P2 前需细化物化机制，见规格 §5.1）。
    Archive { outer: Box<Source>, inner: String },
}

impl Source {
    pub fn path(p: impl Into<PathBuf>) -> Self {
        Source::Path { path: p.into() }
    }

    pub fn as_fs_path(&self) -> Option<&Path> {
        match self {
            Source::Path { path } | Source::Dir { path } => Some(path),
            Source::Archive { .. } => None,
        }
    }

    pub fn display(&self) -> String {
        match self {
            Source::Path { path } => path.display().to_string(),
            Source::Dir { path } => format!("{}/", path.display()),
            Source::Archive { outer, inner } => format!("{}!{inner}", outer.display()),
        }
    }
}
