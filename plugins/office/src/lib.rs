//! Read-only OOXML content previews. No macros, formulas or external resources run.
mod package;
mod presentation;
mod word;
mod workbook;
mod xml;
mod legacy;

use std::path::Path;
use vw_core::viewer::{Doc, Viewer};
use vw_protocol::{
    run_text, search_blocks, Block, BlockList, Cursor, DocMeta, Manifest, OpenOptions, Outline,
    OutlineItem, PageRange, PluginError, ProbeScore, SearchHits, Source,
};

#[derive(Clone, Copy)]
pub struct OfficeLimits {
    pub members: usize,
    pub total_bytes: u64,
    pub xml_bytes: u64,
}
impl Default for OfficeLimits {
    fn default() -> Self {
        Self {
            members: 4096,
            total_bytes: 256 * 1024 * 1024,
            xml_bytes: 32 * 1024 * 1024,
        }
    }
}
pub struct OfficeViewer {
    manifest: Manifest,
    limits: OfficeLimits,
}
impl OfficeViewer {
    pub fn new(priority: i64) -> Self {
        Self::with_limits(priority, OfficeLimits::default())
    }
    pub fn with_limits(priority: i64, limits: OfficeLimits) -> Self {
        Self {
            manifest: Manifest::simple_in_process(
                "vw.office",
                "office",
                &["doc", "xls", "ppt", "docx", "xlsx", "pptx"],
                &["view", "outline"],
                priority,
            ),
            limits,
        }
    }
}
fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}
pub(crate) fn damaged(message: impl std::fmt::Display) -> PluginError {
    PluginError::malformed(format!("Office 文档损坏或超出预览限制：{message}"))
}
impl Viewer for OfficeViewer {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    fn probe(&self, source: &Source) -> Option<ProbeScore> {
        let path = source.as_fs_path()?;
        (path.is_file() && matches!(extension(path).as_str(), "doc" | "xls" | "ppt" | "docx" | "xlsx" | "pptx")).then(
            || ProbeScore {
                score: 95,
                reason: "OOXML Office 扩展名".into(),
            },
        )
    }
    fn open(&self, source: &Source, _opts: &OpenOptions) -> Result<Box<dyn Doc>, PluginError> {
        let path = source
            .as_fs_path()
            .ok_or_else(|| damaged("只支持本地 OOXML 文件"))?;
        if matches!(extension(path).as_str(), "doc" | "xls" | "ppt") {
            return Ok(Box::new(OfficeDoc {
                blocks: legacy::parse(path, self.limits)?,
                title: path.file_name().map(|s| s.to_string_lossy().into()),
                size: std::fs::metadata(path)?.len(),
                mime: match extension(path).as_str() { "doc" => "application/msword", "xls" => "application/vnd.ms-excel", _ => "application/vnd.ms-powerpoint" }.into(),
            }));
        }
        let mut package = package::Package::open(path, self.limits)?;
        let (blocks, mime) = match extension(path).as_str() {
            "docx" => (
                word::parse(&mut package)?,
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            ),
            "xlsx" => (
                workbook::parse(&mut package)?,
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            ),
            "pptx" => (
                presentation::parse(&mut package)?,
                "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            ),
            _ => return Err(PluginError::unsupported("Office 预览支持 DOCX、XLSX、PPTX")),
        };
        Ok(Box::new(OfficeDoc {
            blocks,
            title: path.file_name().map(|s| s.to_string_lossy().into()),
            size: std::fs::metadata(path)?.len(),
            mime: mime.into(),
        }))
    }
}
struct OfficeDoc {
    blocks: Vec<Block>,
    title: Option<String>,
    size: u64,
    mime: String,
}
impl Doc for OfficeDoc {
    fn meta(&self) -> DocMeta {
        DocMeta {
            viewer: "office".into(),
            title: self.title.clone(),
            size: Some(self.size),
            mime: Some(self.mime.clone()),
            pages: Some(self.blocks.len() as u64),
        }
    }
    fn outline(&self) -> Option<Outline> {
        Some(
            self.blocks
                .iter()
                .enumerate()
                .filter_map(|(index, block)| {
                    if let Block::Heading { level, text } = block {
                        Some(OutlineItem {
                            level: *level,
                            text: run_text(text),
                            block: index as u64,
                        })
                    } else {
                        None
                    }
                })
                .collect(),
        )
    }
    fn page(&self, range: PageRange) -> Result<BlockList, PluginError> {
        let start = range.start.min(self.blocks.len() as u64) as usize;
        let end = start
            .saturating_add(range.count as usize)
            .min(self.blocks.len());
        Ok(BlockList {
            blocks: self.blocks[start..end].to_vec(),
            total: Some(self.blocks.len() as u64),
        })
    }
    fn search(&self, query: &str, from: Cursor) -> Result<SearchHits, PluginError> {
        Ok(search_blocks(&self.blocks, query, from))
    }
}
