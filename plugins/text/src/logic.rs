//! 与传输无关的文本查看/编辑逻辑：字节 → IR 块、快照与序列化。
//! 进程内 TextViewer 与 sidecar 示例插件共用这一份，保证双 transport 输出一致。

use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Mutex;
use vw_protocol::{
    search_blocks, Block, BlockList, Cursor, EditCapability, EditSnapshot, PageRange, PluginError,
    SearchHits,
};

pub const EDIT_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// 每 100 行一个 `Preformatted` 块：大文件按需分页的粒度。
pub const LINES_PER_BLOCK: usize = 100;
pub const SNIFF_BYTES: usize = 8192;
pub const EAGER_MAX_BYTES: u64 = 1024 * 1024;
pub const PAGE_MAX_BYTES: usize = 64 * 1024;

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
        Err(e) => e.error_len().is_none() && bytes.len() - e.valid_up_to() <= 3,
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
    lazy: Option<Mutex<LazyText>>,
}

struct LazyText {
    file: std::fs::File,
    size: u64,
    offsets: BTreeMap<u64, u64>,
    total: Option<u64>,
}

impl LazyText {
    fn read_block(&mut self, offset: u64) -> Result<(String, u64), PluginError> {
        self.file.seek(SeekFrom::Start(offset))?;
        let mut bytes =
            vec![0; (self.size.saturating_sub(offset)).min(PAGE_MAX_BYTES as u64) as usize];
        self.file.read_exact(&mut bytes)?;
        let mut lines = 0;
        if let Some(end) = bytes.iter().enumerate().find_map(|(i, &b)| {
            if b == b'\n' {
                lines += 1;
            }
            (lines == LINES_PER_BLOCK).then_some(i + 1)
        }) {
            bytes.truncate(end);
        }
        // A read boundary may split a valid UTF-8 character. Leave its bytes for the next page.
        if offset + (bytes.len() as u64) < self.size {
            let mut valid = 0;
            while valid < bytes.len() {
                match std::str::from_utf8(&bytes[valid..]) {
                    Ok(_) => break,
                    Err(e) => match e.error_len() {
                        Some(n) => valid += e.valid_up_to() + n,
                        None => {
                            bytes.truncate(valid + e.valid_up_to());
                            break;
                        }
                    },
                }
            }
        }
        let next = offset + bytes.len() as u64;
        Ok((String::from_utf8_lossy(&bytes).into_owned(), next))
    }

    fn page(&mut self, range: PageRange) -> Result<BlockList, PluginError> {
        let mut blocks = Vec::new();
        if range.count == 0 || self.total.is_some_and(|t| range.start >= t) {
            return Ok(BlockList {
                blocks,
                total: self.total,
            });
        }
        let (&mut_block, &mut_offset) = self.offsets.range(..=range.start).next_back().unwrap();
        let (mut block, mut offset) = (mut_block, mut_offset);
        let end = range.start.saturating_add(u64::from(range.count.min(1000)));
        while block < end && offset < self.size {
            let (text, next) = self.read_block(offset)?;
            if next <= offset {
                break;
            }
            if block >= range.start {
                blocks.push(Block::preformatted(text));
            }
            block += 1;
            offset = next;
            if self.offsets.len() >= 1024 && !self.offsets.contains_key(&block) {
                if let Some(old) = self.offsets.keys().copied().find(|&key| key != 0) {
                    self.offsets.remove(&old);
                }
            }
            self.offsets.insert(block, offset);
        }
        if offset >= self.size {
            self.total = Some(block);
        }
        Ok(BlockList {
            blocks,
            total: self.total,
        })
    }
}

impl TextCore {
    pub fn from_bytes(bytes: &[u8], title: Option<String>) -> Self {
        let edit = vw_protocol::TextFile::from_bytes(bytes, EDIT_MAX_BYTES);
        Self::from_decoded(bytes.len() as u64, title, edit)
    }

    fn from_decoded(size: u64, title: Option<String>, edit: vw_protocol::TextFile) -> Self {
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
        Self {
            title,
            size,
            text: edit.text.clone(),
            blocks,
            total,
            edit,
            lazy: None,
        }
    }

    /// Small files retain lossless editing; larger files keep bounded, lazy read-only pages.
    pub fn from_path(path: &Path, title: Option<String>) -> Result<Self, PluginError> {
        Self::from_path_encoding(path, title, None)
    }

    pub fn from_path_encoding(path: &Path, title: Option<String>, encoding: Option<&str>) -> Result<Self, PluginError> {
        let mut file = std::fs::File::open(path)?;
        let size = file.metadata()?.len();
        let mut prefix = [0; 3];
        let prefix_len = file.read(&mut prefix)?;
        file.seek(SeekFrom::Start(0))?;
        let utf16 = prefix_len >= 2 && (prefix[..2] == [0xff, 0xfe] || prefix[..2] == [0xfe, 0xff]);
        if size <= EAGER_MAX_BYTES || (encoding.is_some() || utf16) && size <= EDIT_MAX_BYTES {
            let mut bytes = Vec::new();
            file.take(EDIT_MAX_BYTES + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > EDIT_MAX_BYTES {
                return Err(PluginError::conflict("文件在打开期间增长"));
            }
            let edit = match encoding {
                Some(label) => vw_protocol::TextFile::decode(&bytes, EDIT_MAX_BYTES, Some(label))?,
                None => vw_protocol::TextFile::from_bytes(&bytes, EDIT_MAX_BYTES),
            };
            let mut core = Self::from_decoded(bytes.len() as u64, title, edit);
            core.edit.max_bytes = EDIT_MAX_BYTES;
            return Ok(core);
        }
        if utf16 || encoding.is_some_and(|e| !e.eq_ignore_ascii_case("utf-8")) {
            return Err(PluginError::unsupported("大于 64 MiB 的文件仅提供 UTF-8 分页或二进制查看"));
        }
        let mut head = [0; 3];
        file.read_exact(&mut head)?;
        let start = if head == [0xef, 0xbb, 0xbf] { 3 } else { 0 };
        let mut core = Self::from_bytes(&[], title);
        core.size = size;
        core.blocks.clear();
        core.total = 0;
        core.lazy = Some(Mutex::new(LazyText {
            file,
            size,
            offsets: BTreeMap::from([(0, start)]),
            total: None,
        }));
        Ok(core)
    }

    pub fn total_pages(&self) -> Option<u64> {
        self.lazy
            .as_ref()
            .map_or(Some(self.total), |lazy| lazy.lock().unwrap().total)
    }

    pub fn editable(&self) -> bool {
        self.lazy.is_none() && self.edit.editable()
    }

    pub fn edit_capability(&self) -> EditCapability {
        if self.lazy.is_some() {
            EditCapability::None
        } else {
            self.edit.capability()
        }
    }

    pub fn snapshot(&self) -> Result<EditSnapshot, PluginError> {
        if self.lazy.is_some() {
            return Err(PluginError::unsupported("大文件只读，请使用分页浏览"));
        }
        self.edit.snapshot()
    }

    pub fn serialize(&self, edited: &EditSnapshot) -> Result<Vec<u8>, PluginError> {
        if !self.editable() {
            return Err(PluginError::unsupported("该文件只读"));
        }
        let EditSnapshot::Text { text } = edited;
        if text.len() as u64 + if self.edit.has_bom { 3 } else { 0 } > self.edit.max_bytes {
            return Err(PluginError::unsupported("编辑结果超出大小上限"));
        }
        self.edit.serialize(edited)
    }

    pub fn serialize_with_encoding(&self, edited: &EditSnapshot, encoding: Option<&str>) -> Result<Vec<u8>, PluginError> {
        if !self.editable() { return Err(PluginError::unsupported("该文件只读")); }
        let bytes = self.edit.serialize_as(edited, encoding)?;
        if bytes.len() as u64 > self.edit.max_bytes { return Err(PluginError::unsupported("编辑结果超出大小上限")); }
        Ok(bytes)
    }

    pub fn page(&self, range: PageRange) -> Result<BlockList, PluginError> {
        if let Some(lazy) = &self.lazy {
            return lazy.lock().unwrap().page(range);
        }
        let start = range.start as usize;
        if start >= self.blocks.len() {
            return Ok(BlockList {
                blocks: vec![],
                total: Some(self.total),
            });
        }
        let end = start
            .saturating_add(range.count as usize)
            .min(self.blocks.len());
        Ok(BlockList {
            blocks: self.blocks[start..end].to_vec(),
            total: Some(self.total),
        })
    }

    pub fn search(&self, query: &str, from: Cursor) -> Result<SearchHits, PluginError> {
        if self.lazy.is_some() {
            return search_pages(query, from, |block| {
                self.page(PageRange {
                    start: block,
                    count: 1,
                })
            });
        }
        Ok(search_blocks(&self.blocks, query, from))
    }
}

/// Search bounded pages with absolute block cursors and at most 50 matches per response.
pub fn search_pages(
    query: &str,
    from: Cursor,
    mut page: impl FnMut(u64) -> Result<BlockList, PluginError>,
) -> Result<SearchHits, PluginError> {
    let mut result = SearchHits {
        hits: vec![],
        next: None,
    };
    if query.is_empty() {
        return Ok(result);
    }
    let mut block = from.block;
    let end = from.block.saturating_add(256);
    loop {
        let list = page(block)?;
        if list.blocks.is_empty() {
            break;
        }
        let matches = search_blocks(
            &list.blocks,
            query,
            Cursor {
                block: 0,
                offset: if block == from.block { from.offset } else { 0 },
            },
        );
        for mut hit in matches.hits {
            hit.block = block;
            let end = hit.start + query.chars().count() as u64;
            result.hits.push(hit);
            if result.hits.len() == 50 {
                result.next = Some(Cursor { block, offset: end });
                return Ok(result);
            }
        }
        block = block.saturating_add(1);
        if list.total.is_some_and(|t| block >= t) || block == u64::MAX {
            break;
        }
        if block >= end {
            result.next = Some(Cursor { block, offset: 0 });
            break;
        }
    }
    Ok(result)
}
