//! vw 内置插件：纯文本与代码。
//!
//! 两层结构：
//! - [`logic`]：与传输无关的「字节 → IR / 编辑」逻辑，sidecar 示例插件复用
//!   同一份逻辑参与 conformance（同插件、双 transport）；
//! - [`TextViewer`]：进程内 Viewer 适配。

pub mod logic;

use std::fs::File;
use std::io::Read;

use vw_core::viewer::{Doc, EditableDoc, Viewer};
use vw_protocol::{
    BlockList, Cursor, DocMeta, EditCapability, EditSnapshot, Manifest, OpenOptions, Outline,
    PageRange, PluginError, ProbeScore, SearchHits, Source,
};

use logic::TextCore;

pub use logic::{sniff_is_text, EDIT_MAX_BYTES, LINES_PER_BLOCK};

pub struct TextViewer {
    manifest: Manifest,
    viewer_id: String,
    extensions: Vec<String>,
}

impl TextViewer {
    pub fn new(extensions: &[&str], priority: i64) -> Self {
        Self::named("vw.text", "text", extensions, priority)
    }

    pub fn html() -> Self {
        Self::named("vw.html", "html", &["html", "htm"], 50)
    }

    fn named(id: &str, viewer_id: &str, extensions: &[&str], priority: i64) -> Self {
        Self {
            manifest: Manifest::simple_in_process(
                id,
                viewer_id,
                extensions,
                &["view", "edit", "format"],
                priority,
            ),
            viewer_id: viewer_id.into(),
            extensions: extensions.iter().map(|s| s.to_lowercase()).collect(),
        }
    }

    fn probe_score(&self, source: &Source) -> Option<ProbeScore> {
        let path = source.as_fs_path()?;
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if self.extensions.contains(&ext.to_lowercase()) {
                return Some(ProbeScore {
                    score: 55,
                    reason: "扩展名匹配".into(),
                });
            }
        }
        // 内容嗅探（无扩展名命中时）
        let mut f = File::open(path).ok()?;
        let mut head = vec![0u8; logic::SNIFF_BYTES];
        let n = f.read(&mut head).ok()?;
        head.truncate(n);
        if sniff_is_text(&head) {
            Some(ProbeScore {
                score: 35,
                reason: "内容嗅探为文本".into(),
            })
        } else {
            None
        }
    }
}

impl Viewer for TextViewer {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn probe(&self, source: &Source) -> Option<ProbeScore> {
        self.probe_score(source)
    }

    fn open(&self, source: &Source, opts: &OpenOptions) -> Result<Box<dyn Doc>, PluginError> {
        let path = source
            .as_fs_path()
            .ok_or_else(|| PluginError::malformed("文本插件只支持本地路径源"))?;
        let title = path.file_name().map(|n| n.to_string_lossy().to_string());
        let core = TextCore::from_path_encoding(path, title, opts.encoding.as_deref())?;
        Ok(Box::new(TextDocView {
            viewer_id: self.viewer_id.clone(),
            core,
            extension: path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase(),
        }))
    }
}

struct TextDocView {
    viewer_id: String,
    core: TextCore,
    extension: String,
}

impl Doc for TextDocView {
    fn format_support(&self) -> Option<vw_protocol::FormatSupport> {
        let parser = match self.extension.as_str() {
            "js" | "mjs" | "cjs" | "jsx" => "babel",
            "ts" | "tsx" | "mts" | "cts" => "typescript",
            "html" | "htm" => "html",
            "css" | "scss" | "less" => "css",
            "yaml" | "yml" => "yaml",
            _ => return None,
        };
        Some(vw_protocol::FormatSupport { provider: "prettier".into(), parser: Some(parser.into()) })
    }
    fn meta(&self) -> DocMeta {
        DocMeta {
            viewer: self.viewer_id.clone(),
            mime: Some(if self.viewer_id == "html" { "text/html" } else { "text/plain" }.into()),
            size: Some(self.core.size),
            title: self.core.title.clone(),
            pages: self.core.total_pages(),
        }
    }

    fn page(&self, range: PageRange) -> Result<BlockList, PluginError> {
        self.core.page(range)
    }

    fn search(&self, query: &str, from: Cursor) -> Result<SearchHits, PluginError> {
        self.core.search(query, from)
    }

    fn as_editable(&self) -> Option<&dyn EditableDoc> {
        if self.core.editable() {
            Some(self)
        } else {
            None
        }
    }
}

impl EditableDoc for TextDocView {
    fn serialize_with_encoding(&self, edited: &EditSnapshot, encoding: Option<&str>) -> Result<Vec<u8>, PluginError> {
        self.core.serialize_with_encoding(edited, encoding)
    }
    fn edit_capability(&self) -> EditCapability {
        self.core.edit_capability()
    }

    fn snapshot(&self) -> Result<EditSnapshot, PluginError> {
        self.core.snapshot()
    }

    fn serialize(&self, edited: &EditSnapshot) -> Result<Vec<u8>, PluginError> {
        self.core.serialize(edited)
    }
}

impl TextDocView {
    /// 预留：P2 的 dir 插件需要 outline 时参考。
    pub fn _outline(&self) -> Option<Outline> {
        None
    }
}
