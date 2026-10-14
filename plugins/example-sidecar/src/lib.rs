//! 最小 sidecar 示例插件的实现：处理 `.log`（纯文本）。
//!
//! 与内置 text 插件共用 `vw_plugin_text::logic::TextCore`，
//! 因此 conformance 能对「同一段逻辑、两种 transport」做等价断言。

use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use vw_plugin_sdk::{CancelFlag, RpcDoc, RpcViewer};
use vw_plugin_text::logic::{sniff_is_text, TextCore, SNIFF_BYTES};
use vw_protocol::{
    BlockList, Cursor, DocMeta, EditCapability, EditSnapshot, OpenOptions, Outline, PageRange, ProbeScore,
    PluginError, SearchHits, Source,
};

pub struct SidecarText;

pub struct SidecarTextDoc {
    core: TextCore,
    viewer_id: String,
}

impl RpcViewer for SidecarText {
    fn probe(&self, source: &Source) -> Option<ProbeScore> {
        let p = source.as_fs_path()?;
        if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
            if ext.eq_ignore_ascii_case("log") {
                return Some(ProbeScore { score: 55, reason: "扩展名匹配（sidecar 示例）".into() });
            }
        }
        let mut f = std::fs::File::open(p).ok()?;
        let mut head = vec![0u8; SNIFF_BYTES];
        let n = f.read(&mut head).ok()?;
        head.truncate(n);
        if sniff_is_text(&head) {
            // 分数略低于内置 text 的嗅探（35），让内置插件优先
            Some(ProbeScore { score: 30, reason: "内容嗅探为文本（sidecar 示例）".into() })
        } else {
            None
        }
    }

    fn open(
        &self,
        source: &Source,
        _opts: &OpenOptions,
        _tmp_dir: &Path,
        _cancel: CancelFlag,
    ) -> Result<Box<dyn RpcDoc>, PluginError> {
        let p = source
            .as_fs_path()
            .ok_or_else(|| PluginError::malformed("sidecar 示例只支持本地路径源"))?;
        let bytes = std::fs::read(p)?;
        let title = p.file_name().map(|n| n.to_string_lossy().to_string());
        Ok(Box::new(SidecarTextDoc {
            core: TextCore::from_bytes(&bytes, title),
            viewer_id: "textlog".into(),
        }))
    }
}

impl RpcDoc for SidecarTextDoc {
    fn meta(&self) -> DocMeta {
        DocMeta {
            viewer: self.viewer_id.clone(),
            mime: Some("text/plain".into()),
            size: Some(self.core.size),
            title: self.core.title.clone(),
            pages: Some(self.core.total),
        }
    }

    fn outline(&self) -> Option<Outline> {
        None
    }

    fn page(&self, range: PageRange, _cancel: CancelFlag) -> Result<BlockList, PluginError> {
        self.core.page(range)
    }

    fn search(&self, query: &str, from: Cursor, _cancel: CancelFlag) -> Result<SearchHits, PluginError> {
        self.core.search(query, from)
    }

    fn edit_capability(&self) -> EditCapability {
        self.core.edit_capability()
    }

    fn snapshot(&self, _cancel: CancelFlag) -> Result<EditSnapshot, PluginError> {
        self.core.snapshot()
    }

    fn serialize(&self, edited: &EditSnapshot, _cancel: CancelFlag) -> Result<Vec<u8>, PluginError> {
        self.core.serialize(edited)
    }
}

/// sidecar 入口：接管 stdin/stdout 直到 EOF。
pub fn run() {
    vw_plugin_sdk::enable_idle_exit_from_env();
    // 测试钩子：验证宿主的 stderr 落盘（§9）
    if std::env::var("VW_EMIT_STDERR").ok().as_deref() == Some("1") {
        eprintln!("stderr-boom-marker");
    }
    // Stdin/Stdout 句柄本身 Send + 'static；serve 内部自建缓冲
    vw_plugin_sdk::serve(std::io::stdin(), std::io::stdout(), Arc::new(SidecarText));
}
