//! 会话：打开文件（探测 → 仲裁 → 打开 → 记录基线）与保存管线（设计规格 §6）。

use std::sync::{Arc, Mutex};

use vw_protocol::{
    BlockList, BlobRef, Cursor, DocMeta, EditCapability, EditSnapshot, OpenOptions, Outline, PageRange, PluginError,
    SearchHits, Source,
};

use crate::policy::{self, Backup, Baseline, SaveReport};
use crate::registry::{viewer_id_of, Registry};
use crate::viewer::Doc;

/// 一次打开的文档会话。
pub struct OpenedDoc {
    pub id: String,
    pub source: Source,
    /// 规范化后的本地路径（P1 恒有值）。
    pub path: std::path::PathBuf,
    pub viewer_id: String,
    pub meta: DocMeta,
    pub backup: Backup,
    doc: Box<dyn Doc>,
    baseline: Mutex<Baseline>,
}

impl std::fmt::Debug for OpenedDoc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenedDoc")
            .field("id", &self.id)
            .field("viewer_id", &self.viewer_id)
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl OpenedDoc {
    pub fn page(&self, range: PageRange) -> Result<BlockList, PluginError> {
        self.doc.page(range)
    }

    /// 拉取全部块（每次 `page_bulk` 块），用于 CLI 的整体渲染。
    pub fn page_all(&self, max_blocks: usize) -> Result<BlockList, PluginError> {
        let mut all = Vec::new();
        let mut total = None;
        loop {
            let list = self.doc.page(PageRange { start: all.len() as u64, count: 1000 })?;
            if list.total.is_some() {
                total = list.total;
            }
            let done = list.blocks.is_empty()
                || all.len() + list.blocks.len() >= max_blocks
                || total.is_some_and(|t| all.len() + list.blocks.len() >= t as usize);
            all.extend(list.blocks);
            if done || all.len() >= max_blocks {
                break;
            }
        }
        Ok(BlockList { blocks: all, total })
    }

    pub fn outline(&self) -> Option<Outline> {
        self.doc.outline()
    }

    pub fn search(&self, query: &str, from: Cursor) -> Result<SearchHits, PluginError> {
        self.doc.search(query, from)
    }

    pub fn blob(&self, id: &str) -> Result<BlobRef, PluginError> {
        self.doc.blob(id)
    }

    pub fn close(&self) {
        self.doc.close();
    }

    /// 编辑能力；`None` 表示该文档只读。
    pub fn edit_capability(&self) -> Option<EditCapability> {
        self.doc.as_editable().map(|e| e.edit_capability())
    }

    /// 取当前可编辑内容的快照（v1 仅文本）。
    pub fn snapshot_text(&self) -> Result<String, PluginError> {
        let ed = self
            .doc
            .as_editable()
            .ok_or_else(|| PluginError::unsupported(format!("插件 {} 未声明可编辑", self.viewer_id)))?;
        match ed.snapshot()? {
            EditSnapshot::Text { text } => Ok(text),
        }
    }

    /// 保存管线（设计规格 §5.4）：serialize → 冲突检测 → 原子写 → 备份 → 更新基线。
    pub fn save_text(&self, text: String) -> Result<SaveReport, PluginError> {
        let ed = self
            .doc
            .as_editable()
            .ok_or_else(|| PluginError::unsupported(format!("插件 {} 未声明可编辑", self.viewer_id)))?;
        // 1. 插件校验并产出新字节（可拒绝）
        let bytes = ed.serialize(&EditSnapshot::text(text))?;
        // 2. 冲突检测
        self.baseline.lock().unwrap().check_unchanged()?;
        // 3. 原子写 + 备份
        let report = policy::atomic_write(&self.path, &bytes, self.backup)?;
        // 4. 更新基线
        *self.baseline.lock().unwrap() =
            Baseline::after_write(self.path.clone(), &bytes).map_err(PluginError::from)?;
        Ok(report)
    }

    /// 诊断用：当前基线哈希。
    pub fn baseline_hash(&self) -> Result<String, PluginError> {
        self.baseline.lock().unwrap().hash_hex()
    }
}

pub struct Session {
    registry: Arc<Registry>,
}

impl Session {
    pub fn new(registry: Arc<Registry>) -> Self {
        Self { registry }
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn open(&self, source: Source, opts: OpenOptions) -> Result<OpenedDoc, PluginError> {
        let path = policy::validate_source(&source)?;
        let viewer = self.registry.select(&source, opts.as_viewer.as_deref())?;
        let doc = viewer.open(&source, &opts)?;
        let meta = doc.meta();
        let baseline = policy::capture_baseline(path.clone()).map_err(PluginError::from)?;
        // 小文件：open 返回前等待基线就绪，确定性排除「打开瞬间被篡改」的竞态
        if meta.size.unwrap_or(0) <= policy::BASELINE_WAIT_LIMIT {
            baseline.snapshot()?;
        }
        Ok(OpenedDoc {
            id: uuid::Uuid::new_v4().to_string(),
            viewer_id: viewer_id_of(&viewer),
            source,
            path,
            meta,
            backup: Backup::Bak,
            doc,
            baseline: Mutex::new(baseline),
        })
    }
}
