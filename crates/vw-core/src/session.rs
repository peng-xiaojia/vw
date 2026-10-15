//! 会话：打开文件/目录/归档成员（探测 → 仲裁 → 打开 → 记录基线）与保存管线（设计规格 §6）。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use vw_protocol::{
    BlobRef, BlockList, Cursor, DocMeta, EditCapability, EditSnapshot, OpenOptions, Outline,
    PageRange, PluginError, SearchHits, Source,
};

use crate::policy::{self, Backup, Baseline, SaveReport};
use crate::registry::{viewer_id_of, Registry};
use crate::viewer::{Doc, Viewer};

/// Owned temporary resources; only the owner removes its directory, never an age sweep.
pub struct SessionTempDir {
    path: PathBuf,
}
impl SessionTempDir {
    pub fn new() -> Result<Self, PluginError> {
        Self::in_root(&crate::tmp_root())
    }
    pub fn in_root(root: &Path) -> Result<Self, PluginError> {
        let path = root.join(format!("sess-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
}
impl Drop for SessionTempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// 一次打开的文档会话。
pub struct OpenedDoc {
    pub id: String,
    /// 展示用的源（归档成员保留溯源信息，§5.1）。
    pub source: Source,
    /// 实际 IO 路径（归档成员为物化后的临时路径）。
    pub path: PathBuf,
    pub viewer_id: String,
    /// Metadata captured at open. Use `current_meta()` after saving/paging.
    pub meta: DocMeta,
    pub backup: Backup,
    doc: RwLock<Box<dyn Doc>>,
    viewer: Arc<dyn Viewer>,
    io_source: Source,
    opts: Mutex<OpenOptions>,
    _temp_dir: Option<SessionTempDir>,
    closed: AtomicBool,
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
    fn ensure_open(&self) -> Result<(), PluginError> {
        if self.closed.load(Ordering::SeqCst) {
            Err(PluginError::unsupported("文档会话已关闭"))
        } else {
            Ok(())
        }
    }
    pub fn page(&self, range: PageRange) -> Result<BlockList, PluginError> {
        let doc = self.doc.read().unwrap();
        self.ensure_open()?;
        doc.page(range)
    }

    /// 拉取全部块（每次 1000 块），用于 CLI/GUI 的整体渲染。
    pub fn page_all(&self, max_blocks: usize) -> Result<BlockList, PluginError> {
        let doc = self.doc.read().unwrap();
        self.ensure_open()?;
        let mut all = Vec::new();
        let mut total = None;
        loop {
            let remaining = max_blocks.saturating_sub(all.len());
            if remaining == 0 {
                break;
            }
            let list = doc.page(PageRange {
                start: all.len() as u64,
                count: remaining.min(1000) as u32,
            })?;
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
        let doc = self.doc.read().unwrap();
        self.ensure_open().ok()?;
        doc.outline()
    }

    pub fn search(&self, query: &str, from: Cursor) -> Result<SearchHits, PluginError> {
        let doc = self.doc.read().unwrap();
        self.ensure_open()?;
        doc.search(query, from)
    }

    pub fn blob(&self, id: &str) -> Result<BlobRef, PluginError> {
        let doc = self.doc.read().unwrap();
        self.ensure_open()?;
        doc.blob(id)
    }

    pub fn close(&self) {
        let doc = self.doc.write().unwrap();
        if !self.closed.swap(true, Ordering::SeqCst) {
            doc.close();
        }
    }

    /// 编辑能力；`None` 表示该文档只读。
    pub fn edit_capability(&self) -> Option<EditCapability> {
        if matches!(self.source, Source::Archive { .. }) {
            return None;
        }
        let doc = self.doc.read().unwrap();
        self.ensure_open().ok()?;
        doc.as_editable().map(|e| e.edit_capability())
    }

    /// 取当前可编辑内容的快照（v1 仅文本）。
    pub fn snapshot_text(&self) -> Result<String, PluginError> {
        if matches!(self.source, Source::Archive { .. }) {
            return Err(PluginError::unsupported("归档成员只读，请导出后编辑"));
        }
        let doc = self.doc.read().unwrap();
        self.ensure_open()?;
        let ed = doc.as_editable().ok_or_else(|| {
            PluginError::unsupported(format!("插件 {} 未声明可编辑", self.viewer_id))
        })?;
        match ed.snapshot()? {
            EditSnapshot::Text { text } => Ok(text),
        }
    }

    /// 保存管线（设计规格 §5.4）：serialize → 冲突检测 → 原子写 → 备份 → 更新基线。
    pub fn save_text(&self, text: String) -> Result<SaveReport, PluginError> {
        self.save_text_with_encoding(text, None)
    }

    pub fn format_support(&self) -> Option<vw_protocol::FormatSupport> {
        self.doc.read().unwrap().format_support()
    }

    pub fn format_text(&self, text: &str) -> Result<String, PluginError> {
        let doc = self.doc.read().unwrap();
        self.ensure_open()?;
        if !self.viewer.manifest().capabilities_of(&self.viewer_id).iter().any(|c| c == "format") {
            return Err(PluginError::unsupported("插件未注册格式化能力"));
        }
        doc.format_text(text)
    }

    pub fn reopen_encoding(&self, encoding: &str) -> Result<(), PluginError> {
        let mut doc = self.doc.write().unwrap();
        self.ensure_open()?;
        if matches!(self.source, Source::Archive { .. }) {
            return Err(PluginError::unsupported("归档成员请导出后重新解码"));
        }
        let mut opts = self.opts.lock().unwrap();
        let mut next = opts.clone(); next.encoding = Some(encoding.into());
        let fresh = self.viewer.open(&self.io_source, &next)?;
        if fresh.as_editable().is_none() { return Err(PluginError::unsupported("该文件不支持文本重新解码")); }
        let baseline = policy::capture_baseline(self.path.clone())?;
        baseline.snapshot()?;
        *self.baseline.lock().unwrap() = Some(baseline);
        doc.close(); *doc = fresh; *opts = next;
        Ok(())
    }

    pub fn save_text_with_encoding(&self, text: String, encoding: Option<&str>) -> Result<SaveReport, PluginError> {
        if matches!(self.source, Source::Archive { .. }) {
            return Err(PluginError::unsupported("归档成员只读，请导出后编辑"));
        }
        let mut doc = self.doc.write().unwrap();
        self.ensure_open()?;
        let ed = doc.as_editable().ok_or_else(|| {
            PluginError::unsupported(format!("插件 {} 未声明可编辑", self.viewer_id))
        })?;
        let mut guard = self.baseline.lock().unwrap();
        let Some(baseline) = guard.as_ref() else {
            return Err(PluginError::unsupported("该源（目录）不支持保存"));
        };
        // 1. 插件校验并产出新字节（可拒绝）
        let bytes = ed.serialize_with_encoding(&EditSnapshot::text(text), encoding)?;
        if let EditCapability::Text {
            max_bytes: Some(limit),
            ..
        } = ed.edit_capability()
        {
            if bytes.len() as u64 > limit {
                return Err(PluginError::unsupported("编辑结果超出插件大小上限"));
            }
        }
        // 2. 冲突检测
        baseline.check_unchanged()?;
        // 3. 原子写 + 备份
        let report = policy::atomic_write(&self.path, &bytes, self.backup)?;
        // Keep the write lifecycle exclusive through baseline publication and plugin refresh.
        *guard = match Baseline::after_write(self.path.clone(), &bytes) {
            Ok(baseline) => Some(baseline),
            Err(err) => {
                doc.close();
                let mut meta = doc.meta();
                meta.size = Some(bytes.len() as u64);
                *doc = Box::new(UnavailableDoc {
                    meta,
                    message: format!("文件已保存，但基线更新失败：{err}"),
                });
                return Err(PluginError::internal(format!(
                    "文件已保存，但基线更新失败：{err}"
                )));
            }
        };
        let mut opts = self.opts.lock().unwrap();
        if let Some(label) = encoding { opts.encoding = Some(label.into()); }
        match self.viewer.open(&self.io_source, &opts) {
            Ok(fresh) => {
                doc.close();
                *doc = fresh;
            }
            Err(err) => {
                doc.close();
                *doc = Box::new(UnavailableDoc {
                    meta: doc.meta(),
                    message: err.message.clone(),
                });
                return Err(PluginError::internal(format!(
                    "文件已保存，但重新打开失败：{}",
                    err.message
                )));
            }
        }
        Ok(report)
    }

    pub fn current_meta(&self) -> DocMeta {
        self.doc.read().unwrap().meta()
    }

    pub fn resolve_blob(&self, id: &str) -> Result<(BlobRef, PathBuf), PluginError> {
        let doc = self.doc.read().unwrap();
        self.ensure_open()?;
        let blob = doc.blob(id)?;
        let root = doc
            .blob_root()
            .ok_or_else(|| PluginError::unsupported("插件未声明资源目录"))?;
        let rel = Path::new(&blob.path);
        if blob.path.is_empty()
            || rel.is_absolute()
            || blob.path.split(['/', '\\']).any(|s| s == "..")
        {
            return Err(PluginError::permission("blob 路径越界"));
        }
        let root = std::fs::canonicalize(root)?;
        let trusted_root = std::fs::canonicalize(crate::tmp_root())?;
        if !root.starts_with(&trusted_root) || root == trusted_root {
            return Err(PluginError::permission(
                "blob 资源目录不在受信任的会话临时目录内",
            ));
        }
        let path = std::fs::canonicalize(root.join(rel))?;
        if !path.starts_with(&root) || !path.is_file() {
            return Err(PluginError::permission("blob 路径越界或不是文件"));
        }
        Ok((blob, path))
    }

    /// 诊断用：当前基线哈希（目录源为 None）。
    pub fn baseline_hash(&self) -> Result<Option<String>, PluginError> {
        match self.baseline.lock().unwrap().as_ref() {
            Some(b) => Ok(Some(b.hash_hex()?)),
            None => Ok(None),
        }
    }
}

impl Drop for OpenedDoc {
    fn drop(&mut self) {
        if !self.closed.swap(true, Ordering::SeqCst) {
            self.doc.get_mut().unwrap().close();
        }
    }
}

struct UnavailableDoc {
    meta: DocMeta,
    message: String,
}
impl Doc for UnavailableDoc {
    fn meta(&self) -> DocMeta {
        self.meta.clone()
    }
    fn page(&self, _: PageRange) -> Result<BlockList, PluginError> {
        Err(PluginError::internal(&self.message))
    }
    fn search(&self, _: &str, _: Cursor) -> Result<SearchHits, PluginError> {
        Err(PluginError::internal(&self.message))
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
        let io_source = if matches!(source, Source::Dir { .. }) {
            Source::Dir { path: path.clone() }
        } else {
            Source::path(&path)
        };
        self.open_doc(&io_source, source, path, opts, None)
    }

    /// 打开归档内的成员（设计规格 §5.1 的物化机制）：
    /// 归档插件物化到会话临时目录 → 核心以物化路径**重新探测仲裁**并按
    /// 普通文件打开；展示源保留归档溯源信息。
    pub fn open_member(
        &self,
        outer: &Path,
        inner: &str,
        opts: OpenOptions,
    ) -> Result<OpenedDoc, PluginError> {
        let src = Source::Archive {
            outer: Box::new(Source::path(outer)),
            inner: inner.to_string(),
        };
        let viewer = self.registry.select(&src, None)?;
        let arch = viewer
            .as_ref()
            .as_archive()
            .ok_or_else(|| PluginError::unsupported("选中的插件不支持解包归档成员"))?;
        let dir = SessionTempDir::new()?;
        let materialized = arch.materialize(outer, inner, dir.path())?;
        let canonical = std::fs::canonicalize(&materialized)?;
        if !canonical.starts_with(std::fs::canonicalize(dir.path())?) || !canonical.is_file() {
            return Err(PluginError::permission("归档成员物化路径越界"));
        }
        // 对其他插件而言，物化后的成员就是一个普通文件（§5.1）
        self.open_doc(&Source::path(&canonical), src, canonical, opts, Some(dir))
    }

    fn open_doc(
        &self,
        probe_source: &Source,
        display_source: Source,
        path: PathBuf,
        opts: OpenOptions,
        temp_dir: Option<SessionTempDir>,
    ) -> Result<OpenedDoc, PluginError> {
        let viewer = self
            .registry
            .select(probe_source, opts.as_viewer.as_deref())?;
        let doc = viewer.open(probe_source, &opts)?;
        let meta = doc.meta();
        let is_dir = path.is_dir();
        let baseline = if is_dir
            || matches!(display_source, Source::Archive { .. })
            || doc.as_editable().is_none()
        {
            None
        } else {
            let b = policy::capture_baseline(path.clone()).map_err(PluginError::from)?;
            // 小文件：open 返回前等待基线就绪，确定性排除「打开瞬间被篡改」的竞态
            if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) <= policy::BASELINE_WAIT_LIMIT
            {
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
            doc: RwLock::new(doc),
            viewer,
            io_source: probe_source.clone(),
            opts: Mutex::new(opts),
            _temp_dir: temp_dir,
            closed: AtomicBool::new(false),
            baseline: Mutex::new(baseline),
        })
    }
}
