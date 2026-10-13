//! `HostViewer`：把外置 sidecar 插件呈现为内核的 `Viewer`/`Doc`。
//! 内核侧看不到传输差异——这正是「契约统一」的落点（设计规格 §4.2）。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use vw_core::viewer::{Doc, EditableDoc, Viewer};
use vw_protocol::{
    block_list_from_value, BlockList, BlobRef, Cursor, DocMeta, EditCapability, EditSnapshot, Manifest, OpenOptions,
    Outline, PageRange, PluginError, ProbeScore, SearchHits, Source, TransportKind,
};
use vw_plugin_sdk::{serve, RpcViewer};

use crate::conn::Conn;

#[derive(Debug, Clone)]
pub struct HostConfig {
    pub probe_timeout: Duration,
    pub open_timeout: Duration,
    pub request_timeout: Duration,
}

impl Default for HostConfig {
    fn default() -> Self {
        // 设计规格 §9：probe 500ms / open 10s / page 5s
        Self { probe_timeout: Duration::from_millis(500), open_timeout: Duration::from_secs(10), request_timeout: Duration::from_secs(5) }
    }
}

enum Spawn {
    Process { executable: PathBuf },
    Memory { viewer: Arc<dyn RpcViewer> },
}

pub struct HostViewer {
    manifest: Manifest,
    viewer_id: String,
    spawn: Spawn,
    cfg: HostConfig,
    conn: Mutex<Option<Arc<Conn>>>,
    tmp_root: PathBuf,
}

/// 平台标记（manifest `executable` 的 `{platform}` 取值，设计规格 §7.3）。
pub fn platform_tag() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "win32-x64",
        ("windows", "aarch64") => "win32-arm64",
        ("macos", "x86_64") => "darwin-x64",
        ("macos", "aarch64") => "darwin-arm64",
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        _ => "unknown",
    }
}

fn resolve_executable(tpl: &str) -> PathBuf {
    let ext = if cfg!(windows) { ".exe" } else { "" };
    let s = tpl.replace("{platform}", platform_tag()).replace("{ext}", ext);
    PathBuf::from(s)
}

/// 会话临时根目录：`VW_TMP_DIR` 可覆盖（测试用），否则系统临时目录。
pub fn tmp_root() -> PathBuf {
    if let Ok(d) = std::env::var("VW_TMP_DIR") {
        if !d.is_empty() {
            return PathBuf::from(d);
        }
    }
    std::env::temp_dir().join("vw")
}

/// 校验 blob 相对路径必须落在会话临时目录内（设计规格 §5.3/§10）。
fn validate_blob_path(tmp_dir: &Path, rel: &str) -> Result<PathBuf, PluginError> {
    if rel.is_empty() {
        return Err(PluginError::permission("blob 路径为空"));
    }
    let p = Path::new(rel);
    if p.is_absolute() || rel.split(['/', '\\']).any(|seg| seg == "..") {
        return Err(PluginError::permission(format!("blob 路径越界：{rel}")));
    }
    let full = tmp_dir.join(p);
    let canon_dir = std::fs::canonicalize(tmp_dir).map_err(PluginError::from)?;
    let canon = match std::fs::canonicalize(&full) {
        Ok(c) => c,
        Err(_) => return Err(PluginError::malformed(format!("blob 不存在：{rel}"))),
    };
    if !canon.starts_with(&canon_dir) {
        return Err(PluginError::permission(format!("blob 路径越界：{rel}")));
    }
    Ok(canon)
}

impl HostViewer {
    /// 从 manifest 构造（stdio 插件）。executable 占位符在此解析。
    pub fn from_manifest(manifest: Manifest, cfg: HostConfig) -> Result<Self, PluginError> {
        let viewer_id = manifest
            .contributes
            .viewers
            .first()
            .map(|v| v.id.clone())
            .ok_or_else(|| PluginError::internal("manifest 未声明 viewer"))?;
        let tpl = match (&manifest.transport.kind, &manifest.transport.executable) {
            (TransportKind::StdioJsonrpc, Some(tpl)) => tpl.clone(),
            _ => return Err(PluginError::internal("stdio 插件缺少 executable")),
        };
        Ok(Self {
            manifest,
            viewer_id,
            spawn: Spawn::Process { executable: resolve_executable(&tpl) },
            cfg,
            conn: Mutex::new(None),
            tmp_root: tmp_root(),
        })
    }

    /// 测试/嵌入用：对端是同进程内的 `RpcViewer`，走内存管道。
    pub fn from_memory(manifest: Manifest, viewer: Arc<dyn RpcViewer>, cfg: HostConfig) -> Self {
        let viewer_id = manifest
            .contributes
            .viewers
            .first()
            .map(|v| v.id.clone())
            .unwrap_or_default();
        Self {
            manifest,
            viewer_id,
            spawn: Spawn::Memory { viewer },
            cfg,
            conn: Mutex::new(None),
            tmp_root: tmp_root(),
        }
    }

    /// 最近一次会话的插件 stderr（诊断用，设计规格 §9）。
    pub fn stderr_tail(&self) -> Vec<String> {
        match self.conn.lock().unwrap().as_ref() {
            Some(c) => c.stderr_tail.lock().unwrap().iter().cloned().collect(),
            None => vec![],
        }
    }

    fn connect(&self) -> Result<Arc<Conn>, PluginError> {
        let mut guard = self.conn.lock().unwrap();
        if let Some(c) = guard.as_ref() {
            if !c.is_crashed() {
                return Ok(c.clone());
            }
            // 崩溃后按需重启（设计规格 §9）
        }
        let conn = match &self.spawn {
            Spawn::Process { executable } => {
                if !executable.is_file() {
                    return Err(PluginError::crashed(format!(
                        "插件可执行文件不存在：{}",
                        executable.display()
                    )));
                }
                let child = Command::new(executable)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .map_err(|e| PluginError::crashed(format!("启动插件进程失败：{e}")))?;
                Conn::connect_process(child)?
            }
            Spawn::Memory { viewer } => {
                let (client, server) = vw_plugin_sdk::mempipe();
                let v = viewer.clone();
                std::thread::spawn(move || {
                    serve(server.clone(), server, v);
                });
                let closer = client.clone();
                let kill: Box<dyn Fn() + Send + Sync> = Box::new(move || closer.close_write());
                Conn::connect_memory(client, kill)
            }
        };
        *guard = Some(conn.clone());
        Ok(conn)
    }

    fn make_session_dir(&self) -> Result<PathBuf, PluginError> {
        let dir = self.tmp_root.join(format!("sess-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).map_err(PluginError::from)?;
        Ok(dir)
    }
}

impl Viewer for HostViewer {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn probe(&self, source: &Source) -> Option<ProbeScore> {
        let conn = self.connect().ok()?;
        let params = serde_json::json!({ "source": source });
        let result = conn
            .request("probe", params, self.cfg.probe_timeout)
            .ok()?;
        if result.is_null() {
            None
        } else {
            serde_json::from_value(result).ok()
        }
    }

    fn open(&self, source: &Source, opts: &OpenOptions) -> Result<Box<dyn Doc>, PluginError> {
        let conn = self.connect()?;
        let tmp_dir = self.make_session_dir()?;
        let params = serde_json::json!({
            "source": source,
            "opts": opts,
            "tmp_dir": tmp_dir.to_string_lossy(),
        });
        let result = conn.request("open", params, self.cfg.open_timeout)?;
        let handle = result
            .get("handle")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| PluginError::internal("open 响应缺少 handle"))?;
        let meta = result
            .get("meta")
            .cloned()
            .ok_or_else(|| PluginError::internal("open 响应缺少 meta"))?;
        let meta: DocMeta = serde_json::from_value(meta).map_err(PluginError::from)?;
        Ok(Box::new(HostDoc {
            conn,
            handle,
            tmp_dir,
            cfg: self.cfg.clone(),
            meta,
            capability: OnceLock::new(),
        }))
    }
}

struct HostDoc {
    conn: Arc<Conn>,
    handle: String,
    tmp_dir: PathBuf,
    cfg: HostConfig,
    meta: DocMeta,
    capability: OnceLock<EditCapability>,
}

impl HostDoc {
    fn request(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value, PluginError> {
        self.conn.request(method, params, self.cfg.request_timeout)
    }

    fn fetch_capability(&self) -> EditCapability {
        if let Some(c) = self.capability.get() {
            return c.clone();
        }
        let cap = self
            .request("editCapability", serde_json::json!({ "handle": self.handle }))
            .ok()
            .and_then(|v| serde_json::from_value::<EditCapability>(v).ok())
            .unwrap_or(EditCapability::None);
        let _ = self.capability.set(cap.clone());
        cap
    }

    /// 校验 blob 路径必须落在会话临时目录内（设计规格 §5.3/§10）。
    fn check_blob_path(&self, rel: &str) -> Result<PathBuf, PluginError> {
        validate_blob_path(&self.tmp_dir, rel)
    }
}

impl Doc for HostDoc {
    fn meta(&self) -> DocMeta {
        self.meta.clone()
    }

    fn outline(&self) -> Option<Outline> {
        let v = self
            .request("outline", serde_json::json!({ "handle": self.handle }))
            .ok()?;
        if v.is_null() {
            None
        } else {
            serde_json::from_value(v).ok()
        }
    }

    fn page(&self, range: PageRange) -> Result<BlockList, PluginError> {
        let v = self.request(
            "page",
            serde_json::json!({ "handle": self.handle, "range": range }),
        )?;
        // 走逐块前向兼容降级解析（设计规格 §5.2 纪律 4）
        Ok(block_list_from_value(&v))
    }

    fn search(&self, query: &str, from: Cursor) -> Result<SearchHits, PluginError> {
        let v = self.request(
            "search",
            serde_json::json!({ "handle": self.handle, "query": query, "from": from }),
        )?;
        serde_json::from_value(v).map_err(PluginError::from)
    }

    fn blob(&self, id: &str) -> Result<BlobRef, PluginError> {
        let v = self.request("blob", serde_json::json!({ "handle": self.handle, "id": id }))?;
        let r: BlobRef = serde_json::from_value(v).map_err(PluginError::from)?;
        // 核心侧路径校验：必须落在会话临时目录内（安全契约）
        self.check_blob_path(&r.path)?;
        Ok(r)
    }

    fn close(&self) {
        let _ = self.request("close", serde_json::json!({ "handle": self.handle }));
    }

    fn as_editable(&self) -> Option<&dyn EditableDoc> {
        match self.fetch_capability() {
            EditCapability::None => None,
            _ => Some(self),
        }
    }
}

impl EditableDoc for HostDoc {
    fn edit_capability(&self) -> EditCapability {
        self.fetch_capability()
    }

    fn snapshot(&self) -> Result<EditSnapshot, PluginError> {
        let v = self.request("snapshot", serde_json::json!({ "handle": self.handle }))?;
        serde_json::from_value(v).map_err(PluginError::from)
    }

    /// 按契约：插件把新字节写进会话临时目录并返回 blob 引用，
    /// **核心**读取后原子写（设计规格 §5.3/§5.4）。
    fn serialize(&self, edited: &EditSnapshot) -> Result<Vec<u8>, PluginError> {
        let v = self.request(
            "serialize",
            serde_json::json!({ "handle": self.handle, "edited": edited }),
        )?;
        let blob_id = v
            .get("blob")
            .and_then(|b| b.as_str())
            .map(String::from)
            .ok_or_else(|| PluginError::internal("serialize 响应缺少 blob"))?;
        let r = self.blob(&blob_id)?;
        let path = validate_blob_path(&self.tmp_dir, &r.path)?;
        std::fs::read(path).map_err(PluginError::from)
    }
}

/// 汇总诊断信息（vw doctor 的数据源）。
pub struct HostDiagnostics {
    pub plugin_id: String,
    pub crashed: bool,
    pub stderr: Vec<String>,
}

impl HostViewer {
    pub fn diagnostics(&self) -> HostDiagnostics {
        let (crashed, stderr) = match self.conn.lock().unwrap().as_ref() {
            Some(c) => (c.is_crashed(), c.stderr_tail.lock().unwrap().iter().cloned().collect()),
            None => (false, vec![]),
        };
        HostDiagnostics { plugin_id: self.manifest.id.clone(), crashed, stderr }
    }

    pub fn viewer_id(&self) -> &str {
        &self.viewer_id
    }
}
