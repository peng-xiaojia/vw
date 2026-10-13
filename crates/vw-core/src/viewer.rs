//! 内核契约：`Viewer` / `Doc` / `EditableDoc`（设计规格 §5.1、§5.4）。
//!
//! 内置插件与外置 sidecar 在内核侧都呈现为同一个 `Viewer`，
//! 「内置 vs 外置」的差异只剩传输介质，不出现在内核分支里。

use vw_protocol::{
    BlobRef, BlockList, Cursor, DocMeta, EditCapability, EditSnapshot, Manifest, OpenOptions, Outline, PageRange,
    PluginError, ProbeScore, SearchHits, Source,
};

/// 插件对外的唯一入口。
pub trait Viewer: Send + Sync {
    fn manifest(&self) -> &Manifest;

    /// 0..=100。返回 `None` 表示明确不处理。
    fn probe(&self, source: &Source) -> Option<ProbeScore>;

    fn open(&self, source: &Source, opts: &OpenOptions) -> Result<Box<dyn Doc>, PluginError>;
}

/// 一个已打开的文档会话。按需取内容，不要求一次读完。
pub trait Doc: Send + Sync {
    fn meta(&self) -> DocMeta;

    fn outline(&self) -> Option<Outline> {
        None
    }

    fn page(&self, range: PageRange) -> Result<BlockList, PluginError>;

    fn search(&self, query: &str, from: Cursor) -> Result<SearchHits, PluginError>;

    /// blob 引用相对会话临时目录；宿主负责校验路径边界（设计规格 §5.3）。
    fn blob(&self, _id: &str) -> Result<BlobRef, PluginError> {
        Err(vw_protocol::PluginError::unsupported("该插件不提供 blob 资源"))
    }

    fn close(&self) {}

    /// 声明编辑能力时返回 `Some`；这是 `EditableDoc: Doc` 在 trait object 下的桥。
    fn as_editable(&self) -> Option<&dyn EditableDoc> {
        None
    }
}

/// 插件可选实现。未实现即视为只读。
pub trait EditableDoc: Doc {
    fn edit_capability(&self) -> EditCapability;
    fn snapshot(&self) -> Result<EditSnapshot, PluginError>;
    fn serialize(&self, edited: &EditSnapshot) -> Result<Vec<u8>, PluginError>;
}
