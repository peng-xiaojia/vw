//! vw 重格式参考插件：Apache Parquet 数据文件查看（P5 模板）。
//!
//! 设计要点（对应「重格式插件」的模板形态，设计规格 §13 P5）：
//! - **重依赖由插件自带**：`parquet`（Apache Arrow 生态，约数百个 crate）
//!   只出现在本插件，不进内核——内核零格式知识的直接体现；
//! - stdio JSON-RPC sidecar，用 vw-plugin-sdk 实现；
//! - 只读：查看是统一的，编辑语义是插件的可选能力，这里选择不做；
//! - 正式发布时移入独立仓库、独立版本、按 §8 分发；本仓库内仅作参考模板。
//!   （SQLite/PDF/Office/媒体等带 C 依赖的插件同构，只是构建链不同。）
//!
//! 产出形态：文件元信息 `KeyValue` 块 + 数据行 `Table` 块（前 200 行）。

use std::path::Path;
use std::sync::Arc;

use parquet::file::reader::{FileReader, SerializedFileReader};
use vw_plugin_sdk::{CancelFlag, RpcDoc, RpcViewer};
use vw_protocol::{
    search_blocks, Align, Block, BlockList, Cursor, DocMeta, EditCapability, EditSnapshot, Inline, OpenOptions,
    Outline, PageRange, ProbeScore, PluginError, SearchHits, Source,
};

const MAGIC: &[u8] = b"PAR1";
const ROWS_CAP: usize = 200;
const EXTENSIONS: [&str; 2] = ["parquet", "pq"];

pub struct ParquetViewer;

pub struct ParquetDoc {
    blocks: Vec<Block>,
    total: u64,
    title: Option<String>,
    size: u64,
}

impl RpcViewer for ParquetViewer {
    fn probe(&self, source: &Source) -> Option<ProbeScore> {
        let p = source.as_fs_path()?;
        if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
            if EXTENSIONS.contains(&ext.to_lowercase().as_str()) {
                return Some(ProbeScore { score: 85, reason: "扩展名匹配".into() });
            }
        }
        let mut f = std::fs::File::open(p).ok()?;
        let mut head = [0u8; 4];
        let n = std::io::Read::read(&mut f, &mut head).ok()?;
        if &head[..n] == MAGIC {
            Some(ProbeScore { score: 90, reason: "魔数 PAR1".into() })
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
            .ok_or_else(|| PluginError::malformed("parquet 插件只支持本地路径源"))?;
        let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        let title = p.file_name().map(|n| n.to_string_lossy().to_string());

        let f = std::fs::File::open(p).map_err(PluginError::from)?;
        let reader = SerializedFileReader::new(f)
            .map_err(|e| PluginError::malformed(format!("无法读取 Parquet：{e}")))?;
        let meta = reader.metadata();
        let schema = meta.file_metadata().schema_descr();
        let columns: Vec<String> = (0..schema.num_columns())
            .map(|i| schema.column(i).name().to_string())
            .collect();

        let mut blocks = Vec::new();
        blocks.push(Block::KeyValue {
            pairs: vec![
                ("行数".into(), Inline::text(meta.file_metadata().num_rows().to_string())),
                ("行组".into(), Inline::text(meta.num_row_groups().to_string())),
                ("列".into(), Inline::text(columns.join(", "))),
                (
                    "写入器".into(),
                    Inline::text(meta.file_metadata().created_by().unwrap_or("未知").to_string()),
                ),
            ],
        });

        // 数据行（前 ROWS_CAP 行）：行迭代器逐组读取
        let head: Vec<Inline> = columns.iter().map(|c| Inline::text(c.clone())).collect();
        let mut rows: Vec<Vec<Inline>> = Vec::new();
        let iter = reader.get_row_iter(None).map_err(|e| PluginError::malformed(format!("行读取失败：{e}")))?;
        for r in iter.take(ROWS_CAP) {
            let row = r.map_err(|e| PluginError::malformed(format!("行解析失败：{e}")))?;
            let mut cells = Vec::with_capacity(columns.len());
            for (_descr, field) in row.get_column_iter() {
                cells.push(Inline::text(field_to_text(field)));
            }
            rows.push(cells);
        }
        let total_rows = meta.file_metadata().num_rows() as usize;
        if total_rows > ROWS_CAP {
            blocks.push(Block::para(format!("（共 {total_rows} 行，仅显示前 {ROWS_CAP} 行）")));
        }
        blocks.push(Block::Table {
            head: Some(head),
            rows,
            align: vec![Align::Left],
        });

        let total = blocks.len() as u64;
        Ok(Box::new(ParquetDoc { blocks, total, title, size }))
    }
}

impl RpcDoc for ParquetDoc {
    fn meta(&self) -> DocMeta {
        DocMeta {
            viewer: "parquet".into(),
            mime: Some("application/x-parquet".into()),
            size: Some(self.size),
            title: self.title.clone(),
            pages: Some(self.total),
        }
    }

    fn outline(&self) -> Option<Outline> {
        None
    }

    fn page(&self, range: PageRange, _cancel: CancelFlag) -> Result<BlockList, PluginError> {
        let start = range.start as usize;
        if start >= self.blocks.len() {
            return Ok(BlockList { blocks: vec![], total: Some(self.total) });
        }
        let end = (start + range.count as usize).min(self.blocks.len());
        Ok(BlockList { blocks: self.blocks[start..end].to_vec(), total: Some(self.total) })
    }

    fn search(&self, query: &str, from: Cursor, _cancel: CancelFlag) -> Result<SearchHits, PluginError> {
        Ok(search_blocks(&self.blocks, query, from))
    }

    fn edit_capability(&self) -> EditCapability {
        EditCapability::None
    }

    fn snapshot(&self, _cancel: CancelFlag) -> Result<EditSnapshot, PluginError> {
        Err(PluginError::unsupported("Parquet 查看为只读"))
    }
}

/// Field → 人类可读文本：字节数组按 UTF-8 解（Parquet 的字符串就是 BYTE_ARRAY）。
fn field_to_text(f: &parquet::record::Field) -> String {
    use parquet::record::Field;
    match f {
        Field::Str(s) => s.clone(),
        Field::Bytes(b) => String::from_utf8_lossy(b.data()).to_string(),
        Field::Null => "NULL".into(),
        other => format!("{other}"),
    }
}

fn main() {
    vw_plugin_sdk::enable_idle_exit_from_env();
    vw_plugin_sdk::serve(std::io::stdin(), std::io::stdout(), Arc::new(ParquetViewer));
}
