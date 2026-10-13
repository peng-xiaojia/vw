//! vw 内置插件：十六进制兜底查看。`probe = 0` 认领一切文件（设计规格 §7.2），
//! 让「认不出的文件」不经过内核逻辑。

use vw_core::viewer::{Doc, Viewer};
use vw_protocol::{
    Block, BlockList, Cursor, DocMeta, Manifest, OpenOptions, PageRange, ProbeScore, PluginError, SearchHits, Source,
};

/// 查看窗口上限：超出部分不读入（超大二进制不卡）。
const VIEW_CAP: u64 = 16 * 1024 * 1024;
const BYTES_PER_LINE: usize = 16;
const LINES_PER_BLOCK: usize = 512;

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

fn hexdump_lines(bytes: &[u8]) -> Vec<String> {
    bytes
        .chunks(BYTES_PER_LINE)
        .enumerate()
        .map(|(i, chunk)| {
            let offset = i * BYTES_PER_LINE;
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
                .map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' })
                .chain(std::iter::repeat('.').take(BYTES_PER_LINE - chunk.len()))
                .collect();
            format!("{offset:08x}  {hex}|{ascii}|")
        })
        .collect()
}

struct HexDoc {
    viewer_id: String,
    blocks: Vec<Block>,
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
        let start = range.start as usize;
        if start >= self.blocks.len() {
            return Ok(BlockList { blocks: vec![], total: Some(self.total) });
        }
        let end = (start + range.count as usize).min(self.blocks.len());
        Ok(BlockList { blocks: self.blocks[start..end].to_vec(), total: Some(self.total) })
    }

    fn search(&self, query: &str, from: Cursor) -> Result<SearchHits, PluginError> {
        Ok(vw_protocol::search_blocks(&self.blocks, query, from))
    }
}

impl Viewer for HexViewer {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// 兜底插件：永远认领，分数 0。
    fn probe(&self, _source: &Source) -> Option<ProbeScore> {
        Some(ProbeScore { score: 0, reason: "兜底：认领一切文件".into() })
    }

    fn open(&self, source: &Source, _opts: &OpenOptions) -> Result<Box<dyn Doc>, PluginError> {
        let path = source
            .as_fs_path()
            .ok_or_else(|| PluginError::malformed("hex 插件只支持本地路径源"))?;
        let size = std::fs::metadata(path)?.len();
        let truncated = size > VIEW_CAP;
        let read_len = size.min(VIEW_CAP) as usize;
        let mut f = std::fs::File::open(path)?;
        use std::io::Read;
        let mut bytes = vec![0u8; read_len];
        f.read_exact(&mut bytes)?;

        let lines = hexdump_lines(&bytes);
        let mut blocks = Vec::new();
        for chunk in lines.chunks(LINES_PER_BLOCK) {
            blocks.push(Block::preformatted(chunk.join("\n")));
        }
        if blocks.is_empty() {
            blocks.push(Block::preformatted("(空文件)"));
        }
        if truncated {
            blocks.push(Block::error(format!(
                "文件共 {size} 字节，仅显示前 {VIEW_CAP} 字节"
            )));
        }
        let total = blocks.len() as u64;
        let title = path.file_name().map(|n| n.to_string_lossy().to_string());
        Ok(Box::new(HexDoc { viewer_id: self.viewer_id.clone(), blocks, total, title, size }))
    }
}
