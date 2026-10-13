//! 编辑契约（设计规格 §5.4）：编辑跟随插件，核心只提供保存管线。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EditCapability {
    /// 只读。未知取值一律降级为只读，保证向前兼容。
    None,
    /// 纯文本编辑：核心提供通用文本编辑器。
    Text {
        encoding: String,
        line_ending: LineEnding,
        has_bom: bool,
        max_bytes: Option<u64>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineEnding {
    Lf,
    CrLf,
    Preserve,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EditSnapshot {
    Text { text: String },
}

impl EditSnapshot {
    pub fn text(s: impl Into<String>) -> Self {
        EditSnapshot::Text { text: s.into() }
    }
}
