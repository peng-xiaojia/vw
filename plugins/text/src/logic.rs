//! 与传输无关的文本查看/编辑逻辑：字节 → IR 块、快照与序列化。
//! 进程内 TextViewer 与 sidecar 示例插件共用这一份，保证双 transport 输出一致。

use vw_protocol::{
    search_blocks, Block, BlockList, Cursor, EditCapability, EditSnapshot, PageRange, PluginError, SearchHits,
};

pub const EDIT_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// 每 100 行一个 `Preformatted` 块：大文件按需分页的粒度。
pub const LINES_PER_BLOCK: usize = 100;
pub const SNIFF_BYTES: usize = 8192;

/// 内容嗅探：头部必须是合法 UTF-8、无 0 字节、控制字符占比低。
/// （非 UTF-8 编码的文本在 P1 由 hex 兜底查看；误判二进制为文本的代价更高。）
pub fn sniff_is_text(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return true;
    }
    if bytes.contains(&0) {
        return false;
    }
    let ctrl = bytes
        .iter()
        .filter(|&&c| c < 0x20 && c != b'\t' && c != b'\n' && c != b'\r')
        .count();
    if (ctrl as f64) / (bytes.len() as f64) > 0.02 {
        return false;
    }
    // 允许截断在多字节字符中间：剥掉尾部不完整序列再校验
    let valid = match std::str::from_utf8(bytes) {
        Ok(_) => true,
        Err(e) => e.valid_up_to() >= bytes.len() - 3,
    };
    valid
}

pub struct TextCore {
    pub title: Option<String>,
    pub size: u64,
    /// 解码后的文本（供编辑；BOM 已剥离）。
    pub text: String,
    pub blocks: Vec<Block>,
    pub total: u64,
    edit: vw_protocol::TextFile,
}

impl TextCore {
    pub fn from_bytes(bytes: &[u8], title: Option<String>) -> Self {
        let edit = vw_protocol::TextFile::from_bytes(bytes, EDIT_MAX_BYTES);
        let size = bytes.len() as u64;
        // split_inclusive 保留每行行尾（含 \r\n），块拼接即原文。
        let chunks: Vec<&str> = if edit.text.is_empty() {
            vec![]
        } else {
            edit.text.split_inclusive('\n').collect()
        };
        let mut blocks = Vec::new();
        for chunk in chunks.chunks(LINES_PER_BLOCK) {
            blocks.push(Block::preformatted(chunk.concat()));
        }
        if blocks.is_empty() {
            blocks.push(Block::preformatted(""));
        }
        let total = blocks.len() as u64;
        Self { title, size, text: edit.text.clone(), blocks, total, edit }
    }

    pub fn editable(&self) -> bool {
        self.edit.editable()
    }

    pub fn edit_capability(&self) -> EditCapability {
        self.edit.capability()
    }

    pub fn snapshot(&self) -> Result<EditSnapshot, PluginError> {
        self.edit.snapshot()
    }

    pub fn serialize(&self, edited: &EditSnapshot) -> Result<Vec<u8>, PluginError> {
        self.edit.serialize(edited)
    }

    pub fn page(&self, range: PageRange) -> Result<BlockList, PluginError> {
        let start = range.start as usize;
        if start >= self.blocks.len() {
            return Ok(BlockList { blocks: vec![], total: Some(self.total) });
        }
        let end = (start + range.count as usize).min(self.blocks.len());
        Ok(BlockList { blocks: self.blocks[start..end].to_vec(), total: Some(self.total) })
    }

    pub fn search(&self, query: &str, from: Cursor) -> Result<SearchHits, PluginError> {
        Ok(search_blocks(&self.blocks, query, from))
    }
}
