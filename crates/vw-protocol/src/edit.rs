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

/// 通用「纯文本文件编辑」支持：BOM 剥离与回填、无损编码校验、大小上限。
/// 文本类插件（text/json/csv/markdown）复用；不构成格式知识。
///
/// 无法无损往返的文件（解码失败、超限）一律降级只读（设计规格 §5.4）。
pub struct TextFile {
    pub encoding: String,
    pub has_bom: bool,
    /// 保留旧字段名；true 表示所选编码已成功无损解码，并不限于 UTF-8。
    pub valid_utf8: bool,
    pub max_bytes: u64,
    pub size: u64,
    /// 无 BOM 的解码内容；无效 UTF-8 时为 lossy 结果（仅供查看）。
    pub text: String,
}

const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

impl TextFile {
    pub fn from_bytes(bytes: &[u8], max_bytes: u64) -> Self {
        Self::decode(bytes, max_bytes, None).unwrap_or_else(|_| Self {
            encoding: "utf-8".into(), has_bom: bytes.starts_with(&BOM), valid_utf8: false,
            max_bytes, size: bytes.len() as u64, text: String::from_utf8_lossy(bytes).into_owned(),
        })
    }

    /// Explicit decoding is strict: never turn invalid bytes into editable replacement characters.
    pub fn decode(bytes: &[u8], max_bytes: u64, label: Option<&str>) -> Result<Self, PluginError> {
        let detected = if bytes.starts_with(&[0xff, 0xfe]) { "utf-16le" }
            else if bytes.starts_with(&[0xfe, 0xff]) { "utf-16be" } else { "utf-8" };
        let label = label.unwrap_or(detected).to_ascii_lowercase();
        let codec = encoding_rs::Encoding::for_label(if label == "utf-8-bom" { b"utf-8" } else { label.as_bytes() })
            .ok_or_else(|| PluginError::unsupported("不支持该编码"))?;
        let bom: &[u8] = if codec == encoding_rs::UTF_8 { &BOM }
            else if codec == encoding_rs::UTF_16LE { &[0xff, 0xfe] }
            else if codec == encoding_rs::UTF_16BE { &[0xfe, 0xff] } else { &[] };
        let has_bom = !bom.is_empty() && bytes.starts_with(bom);
        let body = if has_bom { &bytes[bom.len()..] } else { bytes };
        let text = codec.decode_without_bom_handling_and_without_replacement(body)
            .ok_or_else(|| PluginError::malformed(format!("字节不是有效的 {label}，请选择其他编码")))?.into_owned();
        Ok(Self { encoding: codec.name().to_ascii_lowercase(), has_bom, valid_utf8: true,
            max_bytes, size: bytes.len() as u64, text })
    }

    pub fn encode(text: &str, label: &str, has_bom: bool) -> Result<Vec<u8>, PluginError> {
        let label = label.to_ascii_lowercase();
        let codec = encoding_rs::Encoding::for_label(if label == "utf-8-bom" { b"utf-8" } else { label.as_bytes() })
            .ok_or_else(|| PluginError::unsupported("不支持该编码"))?;
        let mut out = Vec::new();
        let bom = has_bom || label == "utf-8-bom";
        if codec == encoding_rs::UTF_16LE || codec == encoding_rs::UTF_16BE {
            let le = codec == encoding_rs::UTF_16LE;
            if bom { out.extend_from_slice(if le { &[0xff, 0xfe] } else { &[0xfe, 0xff] }); }
            for unit in text.encode_utf16() {
                out.extend_from_slice(&if le { unit.to_le_bytes() } else { unit.to_be_bytes() });
            }
        } else {
            let (bytes, _, lossy) = codec.encode(text);
            if lossy { return Err(PluginError::save_rejected(format!("{label} 无法表示部分字符，请使用 UTF-8 或 UTF-16"))); }
            if bom && codec == encoding_rs::UTF_8 { out.extend_from_slice(&BOM); }
            out.extend_from_slice(&bytes);
        }
        Ok(out)
    }

    pub fn editable(&self) -> bool {
        self.valid_utf8 && self.size <= self.max_bytes
    }

    pub fn capability(&self) -> EditCapability {
        if !self.editable() {
            EditCapability::None
        } else {
            EditCapability::Text {
                encoding: self.encoding.clone(),
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
        self.serialize_as(edited, None)
    }

    pub fn serialize_as(&self, edited: &EditSnapshot, encoding: Option<&str>) -> Result<Vec<u8>, PluginError> {
        if !self.editable() {
            return Err(PluginError::unsupported("该文件只读"));
        }
        let text = match edited {
            EditSnapshot::Text { text } => text,
        };
        Self::encode(text, encoding.unwrap_or(&self.encoding), if let Some(label) = encoding { label.starts_with("utf-16") } else { self.has_bom })
    }
}
