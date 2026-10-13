//! vw 内置插件：JSON / JSONL。

use std::io::Read;

use vw_core::viewer::{Doc, EditableDoc, Viewer};
use vw_protocol::{
    search_blocks, Block, BlockList, Cursor, DocMeta, EditCapability, EditSnapshot, Manifest, OpenOptions, Outline,
    OutlineItem, PageRange, ProbeScore, PluginError, SearchHits, Source, TextFile,
};

pub const EDIT_MAX_BYTES: u64 = 16 * 1024 * 1024;
const SNIFF_BYTES: usize = 8192;

const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

fn strip_bom(bytes: &[u8]) -> &[u8] {
    if bytes.starts_with(&BOM) {
        &bytes[3..]
    } else {
        bytes
    }
}

pub struct JsonViewer {
    manifest: Manifest,
    viewer_id: String,
    extensions: Vec<String>,
}

impl JsonViewer {
    pub fn new(extensions: &[&str], priority: i64) -> Self {
        Self {
            manifest: Manifest::simple_in_process("vw.json", "json", extensions, &["view", "edit"], priority),
            viewer_id: "json".into(),
            extensions: extensions.iter().map(|s| s.to_lowercase()).collect(),
        }
    }
}

/// 解析主体（剥 BOM 后）：整体成功 → 单块 pretty；失败则按 JSONL 逐行。
/// 返回 (blocks, mime, outline)。
fn build_blocks(bytes: &[u8]) -> Result<(Vec<Block>, &'static str, Option<Outline>), PluginError> {
    let body = strip_bom(bytes);

    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) {
        let pretty = serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string());
        let outline = v.as_object().map(|obj| {
            obj.keys()
                .take(1000)
                .map(|k| OutlineItem { level: 1, text: k.clone(), block: 0 })
                .collect()
        });
        return Ok((vec![Block::Code { lang: Some("json".into()), text: pretty }], "application/json", outline));
    }

    // JSONL：逐行解析，任何一行失败都视为不是我们的菜
    let text = std::str::from_utf8(body)
        .map_err(|_| PluginError::malformed("不是合法的 JSON / JSONL（且含无效 UTF-8）"))?;
    let mut blocks = Vec::new();
    for (idx, line) in text.split_inclusive('\n').enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(line.trim_end_matches(['\n', '\r']))
            .map_err(|e| PluginError::malformed(format!("第 {} 行不是合法 JSON：{e}", idx + 1)))?;
        let pretty = serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string());
        blocks.push(Block::Code { lang: Some("json".into()), text: pretty });
    }
    if blocks.is_empty() {
        return Err(PluginError::malformed("空文件或没有可解析的 JSON 行"));
    }
    let outline = Some(
        (0..blocks.len() as u64)
            .take(1000)
            .map(|i| OutlineItem { level: 1, text: format!("第 {} 行", i + 1), block: i })
            .collect(),
    );
    Ok((blocks, "application/x-ndjson", outline))
}

/// 编辑校验：整体 JSON 或逐行 JSONL 任一通过，否则拒绝保存。
fn validate_edited(bytes: &[u8]) -> Result<(), PluginError> {
    let body = strip_bom(bytes);
    if serde_json::from_slice::<serde_json::Value>(body).is_ok() {
        return Ok(());
    }
    let text = std::str::from_utf8(body).map_err(|_| PluginError::save_rejected("编辑后的内容不是有效 UTF-8"))?;
    let mut any = false;
    for (idx, line) in text.split_inclusive('\n').enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        serde_json::from_str::<serde_json::Value>(line.trim_end_matches(['\n', '\r']))
            .map_err(|e| PluginError::save_rejected(format!("第 {} 行不是合法 JSON：{e}", idx + 1)))?;
        any = true;
    }
    if any {
        Ok(())
    } else {
        Err(PluginError::save_rejected("编辑后的内容为空且不是合法 JSON"))
    }
}

struct JsonDoc {
    viewer_id: String,
    file: TextFile,
    blocks: Vec<Block>,
    mime: &'static str,
    outline: Option<Outline>,
    title: Option<String>,
    size: u64,
}

impl Doc for JsonDoc {
    fn meta(&self) -> DocMeta {
        DocMeta {
            viewer: self.viewer_id.clone(),
            mime: Some(self.mime.into()),
            size: Some(self.size),
            title: self.title.clone(),
            pages: Some(self.blocks.len() as u64),
        }
    }

    fn outline(&self) -> Option<Outline> {
        self.outline.clone()
    }

    fn page(&self, range: PageRange) -> Result<BlockList, PluginError> {
        let start = range.start as usize;
        let total = self.blocks.len() as u64;
        if start >= self.blocks.len() {
            return Ok(BlockList { blocks: vec![], total: Some(total) });
        }
        let end = (start + range.count as usize).min(self.blocks.len());
        Ok(BlockList { blocks: self.blocks[start..end].to_vec(), total: Some(total) })
    }

    fn search(&self, query: &str, from: Cursor) -> Result<SearchHits, PluginError> {
        Ok(search_blocks(&self.blocks, query, from))
    }

    fn as_editable(&self) -> Option<&dyn EditableDoc> {
        if self.file.editable() {
            Some(self)
        } else {
            None
        }
    }
}

impl EditableDoc for JsonDoc {
    fn edit_capability(&self) -> EditCapability {
        self.file.capability()
    }

    fn snapshot(&self) -> Result<EditSnapshot, PluginError> {
        self.file.snapshot()
    }

    fn serialize(&self, edited: &EditSnapshot) -> Result<Vec<u8>, PluginError> {
        let bytes = self.file.serialize(edited)?;
        validate_edited(&bytes)?;
        Ok(bytes)
    }
}

impl Viewer for JsonViewer {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn probe(&self, source: &Source) -> Option<ProbeScore> {
        let path = source.as_fs_path()?;
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if self.extensions.contains(&ext.to_lowercase()) {
                return Some(ProbeScore { score: 65, reason: "扩展名匹配".into() });
            }
        }
        let mut f = std::fs::File::open(path).ok()?;
        let mut head = vec![0u8; SNIFF_BYTES];
        let n = f.read(&mut head).ok()?;
        head.truncate(n);
        let start = head.iter().position(|&b| !b.is_ascii_whitespace()).unwrap_or(head.len());
        match head.get(start) {
            Some(b'{') | Some(b'[') => Some(ProbeScore { score: 40, reason: "内容嗅探为 JSON 结构".into() }),
            _ => None,
        }
    }

    fn open(&self, source: &Source, _opts: &OpenOptions) -> Result<Box<dyn Doc>, PluginError> {
        let path = source
            .as_fs_path()
            .ok_or_else(|| PluginError::malformed("JSON 插件只支持本地路径源"))?;
        let bytes = std::fs::read(path)?;
        let size = bytes.len() as u64;
        let title = path.file_name().map(|n| n.to_string_lossy().to_string());
        let (blocks, mime, outline) = build_blocks(&bytes)?;
        Ok(Box::new(JsonDoc {
            viewer_id: self.viewer_id.clone(),
            file: TextFile::from_bytes(&bytes, EDIT_MAX_BYTES),
            blocks,
            mime,
            outline,
            title,
            size,
        }))
    }
}
