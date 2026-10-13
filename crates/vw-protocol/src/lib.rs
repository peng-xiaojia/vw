//! vw 文件查看器 —— 协议 crate。
//!
//! 独立于 vw-core：第三方插件 SDK 只需依赖本 crate 即可产出/消费协议数据。
//! 这里的每一个类型都是「契约」的一部分，改动需过设计评审。

pub mod blob;
pub mod edit;
pub mod error;
pub mod ir;
pub mod manifest;
pub mod rpc;
pub mod search;
pub mod source;
pub mod types;

pub use blob::BlobRef;
pub use edit::{EditCapability, EditSnapshot, LineEnding};
pub use error::{ErrorKind, PluginError};
pub use ir::{Align, Block, DrawOp, Inline, block_from_value, block_list_from_value};
pub use manifest::{Contributes, EnginesSection, Manifest, ProtocolSection, Transport, TransportKind, ViewerDecl};
pub use rpc::{Request, Response};
pub use search::{block_text, inline_text, run_text, search_blocks};
pub use source::Source;
pub use types::{
    BlockList, Cursor, DocMeta, OpenOptions, Outline, OutlineItem, PageRange, ProbeScore, SearchHit, SearchHits,
};
