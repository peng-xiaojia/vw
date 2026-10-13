//! 编辑契约（设计规格 §5.4）：编辑跟随插件，核心只提供保存管线。

use serde::{Deserialize, Serialize};

use crate::error::PluginError;

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

/// 通用「纯文本文件编辑」支持：BOM 剥离与回填、UTF-8 校验、大小上限。
/// 文本类插件（text/json/csv/markdown）复用；不构成格式知识。
///
/// 无法无损往返的文件（无效 UTF-8、超限）一律降级只读（设计规格 §5.4）。
pub struct TextFile {
    pub has_bom: bool,
    pub valid_utf8: bool,
    pub max_bytes: u64,
    pub size: u64,
    /// 无 BOM 的解码内容；无效 UTF-8 时为 lossy 结果（仅供查看）。
    pub text: String,
}

const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

impl TextFile {
    pub fn from_bytes(bytes: &[u8], max_bytes: u64) -> Self {
        let has_bom = bytes.starts_with(&BOM);
        let body = if has_bom { &bytes[3..] } else { bytes };
        let (valid_utf8, text) = match std::str::from_utf8(body) {
            Ok(s) => (true, s.to_string()),
            Err(_) => (false, String::from_utf8_lossy(body).into_owned()),
        };
        Self { has_bom, valid_utf8, max_bytes, size: bytes.len() as u64, text }
    }

    pub fn editable(&self) -> bool {
        self.valid_utf8 && self.size <= self.max_bytes
    }

    pub fn capability(&self) -> EditCapability {
        if !self.editable() {
            EditCapability::None
        } else {
            EditCapability::Text {
                encoding: "utf-8".into(),
                line_ending: LineEnding::Preserve,
                has_bom: self.has_bom,
                max_bytes: Some(self.max_bytes),
            }
        }
    }

    pub fn snapshot(&self) -> Result<EditSnapshot, PluginError> {
        if !self.editable() {
            return Err(PluginError::unsupported(
                "无法无损往返（无效 UTF-8 或超出大小上限），已降级为只读",
            ));
        }
        Ok(EditSnapshot::text(self.text.clone()))
    }

    pub fn serialize(&self, edited: &EditSnapshot) -> Result<Vec<u8>, PluginError> {
        if !self.editable() {
            return Err(PluginError::unsupported("该文件只读"));
        }
        let text = match edited {
            EditSnapshot::Text { text } => text,
        };
        let mut out = Vec::with_capacity(text.len() + BOM.len());
        if self.has_bom {
            out.extend_from_slice(&BOM);
        }
        out.extend_from_slice(text.as_bytes());
        Ok(out)
    }
}
