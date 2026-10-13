//! 会话：打开文件/目录/归档成员（探测 → 仲裁 → 打开 → 记录基线）与保存管线（设计规格 §6）。

use std::path::{Path, PathBuf};
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
    /// 展示用的源（归档成员保留溯源信息，§5.1）。
    pub source: Source,
    /// 实际 IO 路径（归档成员为物化后的临时路径）。
    pub path: PathBuf,
    pub viewer_id: String,
    pub meta: DocMeta,
    pub backup: Backup,
    doc: Box<dyn Doc>,
    /// 目录源无基线（不可保存）。
    baseline: Mutex<Option<Baseline>>,
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

    /// 拉取全部块（每次 1000 块），用于 CLI/GUI 的整体渲染。
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
        let (report, bytes) = {
            let guard = self.baseline.lock().unwrap();
            let Some(baseline) = guard.as_ref() else {
                return Err(PluginError::unsupported("该源（目录）不支持保存"));
            };
            // 1. 插件校验并产出新字节（可拒绝）
            let bytes = ed.serialize(&EditSnapshot::text(text))?;
            // 2. 冲突检测
            baseline.check_unchanged()?;
            // 3. 原子写 + 备份
            (policy::atomic_write(&self.path, &bytes, self.backup)?, bytes)
        };
        // 4. 更新基线（锁已释放，避免重入死锁）
        *self.baseline.lock().unwrap() =
            Some(Baseline::after_write(self.path.clone(), &bytes).map_err(PluginError::from)?);
        Ok(report)
    }

    /// 诊断用：当前基线哈希（目录源为 None）。
    pub fn baseline_hash(&self) -> Result<Option<String>, PluginError> {
        match self.baseline.lock().unwrap().as_ref() {
            Some(b) => Ok(Some(b.hash_hex()?)),
            None => Ok(None),
        }
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
        self.open_doc(&source, source.clone(), path, opts)
    }

    /// 打开归档内的成员（设计规格 §5.1 的物化机制）：
    /// 归档插件物化到会话临时目录 → 核心以物化路径**重新探测仲裁**并按
    /// 普通文件打开；展示源保留归档溯源信息。
    pub fn open_member(&self, outer: &Path, inner: &str, opts: OpenOptions) -> Result<OpenedDoc, PluginError> {
        let src = Source::Archive { outer: Box::new(Source::path(outer)), inner: inner.to_string() };
        let viewer = self.registry.select(&src, opts.as_viewer.as_deref())?;
        let arch = viewer.as_ref().as_archive().ok_or_else(|| {
            PluginError::unsupported("选中的插件不支持解包归档成员")
        })?;
        let root = crate::tmp_root();
        sweep_stale_sessions(&root);
        let dir = root.join(format!("sess-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).map_err(PluginError::from)?;
        let materialized = arch.materialize(outer, inner, &dir)?;
        // 对其他插件而言，物化后的成员就是一个普通文件（§5.1）
        self.open_doc(&Source::path(&materialized), src, materialized, opts)
    }

    fn open_doc(
        &self,
        probe_source: &Source,
        display_source: Source,
        path: PathBuf,
        opts: OpenOptions,
    ) -> Result<OpenedDoc, PluginError> {
        let viewer = self.registry.select(probe_source, opts.as_viewer.as_deref())?;
        let doc = viewer.open(probe_source, &opts)?;
        let meta = doc.meta();
        let is_dir = path.is_dir();
        let baseline = if is_dir {
            None
        } else {
            let b = policy::capture_baseline(path.clone()).map_err(PluginError::from)?;
            // 小文件：open 返回前等待基线就绪，确定性排除「打开瞬间被篡改」的竞态
            if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) <= policy::BASELINE_WAIT_LIMIT {
                b.snapshot()?;
            }
            Some(b)
        };
        Ok(OpenedDoc {
            id: uuid::Uuid::new_v4().to_string(),
            viewer_id: viewer_id_of(&viewer),
            source: display_source,
            path,
            meta,
            backup: Backup::Bak,
            doc,
            baseline: Mutex::new(baseline),
        })
    }
}

/// 清扫超过 1 小时的历史会话临时目录（§5.3：孤儿文件按目录时间清扫）。
fn sweep_stale_sessions(root: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    let now = std::time::SystemTime::now();
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.starts_with("sess-") {
            continue;
        }
        if let Ok(md) = e.metadata() {
            if let Ok(mtime) = md.modified() {
                if now.duration_since(mtime).map(|d| d.as_secs() > 3600).unwrap_or(false) {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
    }
}
