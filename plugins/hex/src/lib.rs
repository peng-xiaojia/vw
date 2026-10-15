//! vw 内置插件：十六进制兜底查看。`probe = 0` 认领一切文件（设计规格 §7.2），
//! 让「认不出的文件」不经过内核逻辑。

use std::io::{Read, Seek, SeekFrom};
use std::sync::Mutex;
use vw_core::viewer::{Doc, Viewer};
use vw_protocol::{
    Block, BlockList, Cursor, DocMeta, Manifest, OpenOptions, PageRange, PluginError, ProbeScore,
    SearchHits, Source,
};

const BYTES_PER_LINE: usize = 16;
const LINES_PER_BLOCK: usize = 512;
const BYTES_PER_BLOCK: u64 = (BYTES_PER_LINE * LINES_PER_BLOCK) as u64;

pub struct HexViewer {
    manifest: Manifest,
    viewer_id: String,
}

impl HexViewer {
    pub fn new(priority: i64) -> Self {
        Self {
            manifest: Manifest::simple_in_process("vw.hex", "hex", &[], &["view"], priority),
            viewer_id: "hex".into(),
        }
    }
}

fn hexdump_lines(bytes: &[u8], base: u64) -> Vec<String> {
    bytes
        .chunks(BYTES_PER_LINE)
        .enumerate()
        .map(|(i, chunk)| {
            let offset = base + (i * BYTES_PER_LINE) as u64;
            // 每 8 字节多补一个空格；整行 hex 栏固定 49 列（48 + 1）
            let mut hex = String::with_capacity(BYTES_PER_LINE * 3 + 1);
            for (j, b) in chunk.iter().enumerate() {
                hex.push_str(&format!("{b:02x} "));
                if j == 7 {
                    hex.push(' ');
                }
            }
            while hex.len() < BYTES_PER_LINE * 3 + 1 {
                hex.push(' ');
            }
            let ascii: String = chunk
                .iter()
                .map(|&b| {
                    if (0x20..0x7f).contains(&b) {
                        b as char
                    } else {
                        '.'
                    }
                })
                .chain(std::iter::repeat('.').take(BYTES_PER_LINE - chunk.len()))
                .collect();
            format!("{offset:08x}  {hex}|{ascii}|")
        })
        .collect()
}

struct HexDoc {
    viewer_id: String,
    file: Mutex<std::fs::File>,
    total: u64,
    title: Option<String>,
    size: u64,
}

impl Doc for HexDoc {
    fn meta(&self) -> DocMeta {
        DocMeta {
            viewer: self.viewer_id.clone(),
            mime: Some("application/octet-stream".into()),
            size: Some(self.size),
            title: self.title.clone(),
            pages: Some(self.total),
        }
    }

    fn page(&self, range: PageRange) -> Result<BlockList, PluginError> {
        if range.start >= self.total {
            return Ok(BlockList {
                blocks: vec![],
                total: Some(self.total),
            });
        }
        let mut file = self.file.lock().unwrap();
        let mut blocks = Vec::new();
        let end = range
            .start
            .saturating_add(u64::from(range.count.min(1000)))
            .min(self.total);
        for block in range.start..end {
            let offset = block * BYTES_PER_BLOCK;
            let len = self.size.saturating_sub(offset).min(BYTES_PER_BLOCK) as usize;
            let mut bytes = vec![0; len];
            file.seek(SeekFrom::Start(offset))?;
            file.read_exact(&mut bytes)?;
            blocks.push(Block::preformatted(if bytes.is_empty() {
                "(空文件)".into()
            } else {
                hexdump_lines(&bytes, offset).join("\n")
            }));
        }
        Ok(BlockList {
            blocks,
            total: Some(self.total),
        })
    }

    fn search(&self, query: &str, from: Cursor) -> Result<SearchHits, PluginError> {
        let mut result = SearchHits {
            hits: vec![],
            next: None,
        };
        if query.is_empty() {
            return Ok(result);
        }
        let end = from.block.saturating_add(256).min(self.total);
        for block in from.block..end {
            let list = self.page(PageRange {
                start: block,
                count: 1,
            })?;
            let matches = vw_protocol::search_blocks(
                &list.blocks,
                query,
                Cursor {
                    block: 0,
                    offset: if block == from.block { from.offset } else { 0 },
                },
            );
            for mut hit in matches.hits {
                hit.block = block;
                let offset = hit.start + query.chars().count() as u64;
                result.hits.push(hit);
                if result.hits.len() == 50 {
                    result.next = Some(Cursor { block, offset });
                    return Ok(result);
                }
            }
        }
        if end < self.total {
            result.next = Some(Cursor {
                block: end,
                offset: 0,
            });
        }
        Ok(result)
    }
}

impl Viewer for HexViewer {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// 兜底插件：永远认领，分数 0。
    fn probe(&self, _source: &Source) -> Option<ProbeScore> {
        Some(ProbeScore {
            score: 0,
            reason: "兜底：认领一切文件".into(),
        })
    }

    fn open(&self, source: &Source, _opts: &OpenOptions) -> Result<Box<dyn Doc>, PluginError> {
        let path = source
            .as_fs_path()
            .ok_or_else(|| PluginError::malformed("hex 插件只支持本地路径源"))?;
        let file = std::fs::File::open(path)?;
        let size = file.metadata()?.len();
        let total = size.div_ceil(BYTES_PER_BLOCK).max(1);
        let title = path.file_name().map(|n| n.to_string_lossy().to_string());
        Ok(Box::new(HexDoc {
            viewer_id: self.viewer_id.clone(),
            file: Mutex::new(file),
            total,
            title,
            size,
        }))
    }
}
