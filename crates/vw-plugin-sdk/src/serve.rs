//! sidecar 服务端循环：读 JSON Lines 请求，分发到 [`RpcViewer`] / [`RpcDoc`]。
//!
//! 每个请求在独立线程上执行（取消依赖 [`CancelFlag`]）；响应可能乱序返回
//! （协议允许）。blob 一律写到 `open` 下发的 `tmp_dir` 内并返回相对路径。

use std::collections::HashMap;
use std::io::{BufRead, BufWriter, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use vw_protocol::{
    BlockList, BlobRef, Cursor, DocMeta, EditCapability, EditSnapshot, OpenOptions, Outline, PageRange, PluginError,
    ProbeScore, Request, Response, SearchHits, Source,
};

/// 取消标志：长操作在块边界检查。
pub type CancelFlag = Arc<AtomicBool>;

pub trait RpcViewer: Send + Sync {
    fn probe(&self, source: &Source) -> Option<ProbeScore>;

    fn open(
        &self,
        source: &Source,
        opts: &OpenOptions,
        tmp_dir: &std::path::Path,
        cancel: CancelFlag,
    ) -> Result<Box<dyn RpcDoc>, PluginError>;
}

pub trait RpcDoc: Send + Sync {
    fn meta(&self) -> DocMeta;

    fn outline(&self) -> Option<Outline> {
        None
    }

    fn page(&self, range: PageRange, _cancel: CancelFlag) -> Result<BlockList, PluginError>;

    fn search(&self, query: &str, from: Cursor, _cancel: CancelFlag) -> Result<SearchHits, PluginError>;

    fn blob(&self, _id: &str) -> Result<BlobRef, PluginError> {
        Err(PluginError::unsupported("该插件不提供 blob 资源"))
    }

    fn edit_capability(&self) -> EditCapability {
        EditCapability::None
    }

    fn snapshot(&self, _cancel: CancelFlag) -> Result<EditSnapshot, PluginError> {
        Err(PluginError::unsupported("该文档不可编辑"))
    }

    fn serialize(&self, _edited: &EditSnapshot, _cancel: CancelFlag) -> Result<Vec<u8>, PluginError> {
        Err(PluginError::unsupported("该文档不可编辑"))
    }

    fn close(&self) {}
}

struct DocEntry {
    doc: Arc<dyn RpcDoc>,
    tmp_dir: PathBuf,
}

impl Clone for DocEntry {
    fn clone(&self) -> Self {
        Self { doc: self.doc.clone(), tmp_dir: self.tmp_dir.clone() }
    }
}

struct ServeState {
    writer: Mutex<BufWriter<Box<dyn Write + Send>>>,
    docs: Mutex<HashMap<String, DocEntry>>,
    in_flight: Mutex<HashMap<u64, CancelFlag>>,
    /// serialize 产出的虚拟 blob：id → 绝对路径
    blobs: Mutex<HashMap<String, PathBuf>>,
    next_blob: AtomicU64,
    last_activity: Mutex<Instant>,
}

/// 接管 input/output，直到 EOF 或 `shutdown`。阻塞调用线程。
pub fn serve<R: Read + Send + 'static, W: Write + Send + 'static>(input: R, output: W, viewer: Arc<dyn RpcViewer>) {
    let state = Arc::new(ServeState {
        writer: Mutex::new(BufWriter::new(Box::new(output) as Box<dyn Write + Send>)),
        docs: Mutex::new(HashMap::new()),
        in_flight: Mutex::new(HashMap::new()),
        blobs: Mutex::new(HashMap::new()),
        next_blob: AtomicU64::new(1),
        last_activity: Mutex::new(Instant::now()),
    });

    let mut reader = std::io::BufReader::new(input);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        *state.last_activity.lock().unwrap() = Instant::now();
        note_activity();
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let req: Request = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                // 无法解析的消息没有可靠 id，用 0 回错误
                write_response(&state, Response::err(0, PluginError::internal(format!("请求解析失败：{e}"))));
                continue;
            }
        };

        if req.method == "shutdown" {
            write_response(&state, Response::ok(req.id, serde_json::json!(true)));
            break;
        }
        if req.method == "cancel" {
            let target = req.params.get("target").and_then(|v| v.as_u64()).unwrap_or(u64::MAX);
            if let Some(flag) = state.in_flight.lock().unwrap().get(&target) {
                flag.store(true, Ordering::SeqCst);
            }
            write_response(&state, Response::ok(req.id, serde_json::json!({"cancelled": true})));
            continue;
        }

        // 每个请求一个工作线程
        let st = state.clone();
        let v = viewer.clone();
        std::thread::spawn(move || {
            let cancel: CancelFlag = Arc::new(AtomicBool::new(false));
            st.in_flight.lock().unwrap().insert(req.id, cancel.clone());
            let resp = dispatch(&st, &v, &req, cancel);
            st.in_flight.lock().unwrap().remove(&req.id);
            write_response(&st, resp);
        });
    }

    // EOF / shutdown：清理全部文档
    let docs: Vec<DocEntry> = std::mem::take(&mut *state.docs.lock().unwrap()).into_values().collect();
    for e in docs {
        e.doc.close();
    }
    let _ = state.writer.lock().unwrap().flush();
}

fn write_response(state: &ServeState, resp: Response) {
    let mut w = state.writer.lock().unwrap();
    if let Ok(line) = serde_json::to_string(&resp) {
        let _ = w.write_all(line.as_bytes());
        let _ = w.write_all(b"\n");
        let _ = w.flush();
    }
}

fn handle_err(id: u64, e: impl std::fmt::Display) -> Response {
    Response::err(id, PluginError::internal(format!("{e}")))
}

fn dispatch(state: &ServeState, viewer: &Arc<dyn RpcViewer>, req: &Request, cancel: CancelFlag) -> Response {
    let id = req.id;
    match req.method.as_str() {
        "probe" => match serde_json::from_value::<ProbeParams>(req.params.clone()) {
            Ok(p) => match viewer.probe(&p.source) {
                Some(score) => Response::ok(id, serde_json::to_value(score).unwrap_or(serde_json::Value::Null)),
                None => Response::ok(id, serde_json::Value::Null),
            },
            Err(e) => handle_err(id, format!("probe 参数错误：{e}")),
        },
        "open" => match serde_json::from_value::<OpenParams>(req.params.clone()) {
            Ok(p) => {
                let tmp_dir = PathBuf::from(&p.tmp_dir);
                match viewer.open(&p.source, &p.opts, &tmp_dir, cancel) {
                    Ok(doc) => {
                        let handle = uuid::Uuid::new_v4().to_string();
                        let meta = doc.meta();
                        state.docs.lock().unwrap().insert(
                            handle.clone(),
                            DocEntry { doc: Arc::from(doc), tmp_dir },
                        );
                        Response::ok(id, serde_json::json!({ "handle": handle, "meta": meta }))
                    }
                    Err(e) => Response::err(id, e),
                }
            }
            Err(e) => handle_err(id, format!("open 参数错误：{e}")),
        },
        "meta" => with_doc(state, req, |entry, id| {
            Response::ok(id, serde_json::to_value(entry.doc.meta()).unwrap_or(serde_json::Value::Null))
        }),
        "outline" => with_doc(state, req, |entry, id| {
            match entry.doc.outline() {
                Some(o) => Response::ok(id, serde_json::to_value(o).unwrap_or(serde_json::Value::Null)),
                None => Response::ok(id, serde_json::Value::Null),
            }
        }),
        "page" => match serde_json::from_value::<PageParams>(req.params.clone()) {
            Ok(p) => with_doc(state, req, move |entry, id| match entry.doc.page(p.range, cancel) {
                Ok(list) => Response::ok(id, serde_json::to_value(list).unwrap_or(serde_json::Value::Null)),
                Err(e) => Response::err(id, e),
            }),
            Err(e) => handle_err(id, format!("page 参数错误：{e}")),
        },
        "search" => match serde_json::from_value::<SearchParams>(req.params.clone()) {
            Ok(p) => with_doc(state, req, move |entry, id| {
                match entry.doc.search(&p.query, p.from, cancel) {
                    Ok(hits) => Response::ok(id, serde_json::to_value(hits).unwrap_or(serde_json::Value::Null)),
                    Err(e) => Response::err(id, e),
                }
            }),
            Err(e) => handle_err(id, format!("search 参数错误：{e}")),
        },
        "blob" => match serde_json::from_value::<BlobParams>(req.params.clone()) {
            Ok(p) => with_doc(state, req, move |entry, id| {
                // 先查 serialize 产出的虚拟 blob，再委托文档
                let mapped = state
                    .blobs
                    .lock()
                    .unwrap()
                    .get(&p.id)
                    .map(|abs| relative_to(&entry.tmp_dir, abs));
                match mapped {
                    Some(Ok(rel)) => {
                        Response::ok(id, serde_json::to_value(BlobRef { path: rel, mime: None, size: None }).unwrap())
                    }
                    Some(Err(e)) => Response::err(id, e),
                    None => match entry.doc.blob(&p.id) {
                        Ok(mut r) => {
                            if let Err(e) = sanitize_relative(&r.path) {
                                Response::err(id, e)
                            } else {
                                r.mime = r.mime.take();
                                Response::ok(id, serde_json::to_value(r).unwrap())
                            }
                        }
                        Err(e) => Response::err(id, e),
                    },
                }
            }),
            Err(e) => handle_err(id, format!("blob 参数错误：{e}")),
        },
        "editCapability" => with_doc(state, req, |entry, id| {
            let cap = entry.doc.edit_capability();
            Response::ok(id, serde_json::to_value(cap).unwrap_or(serde_json::Value::Null))
        }),
        "snapshot" => with_doc(state, req, |entry, id| match entry.doc.snapshot(cancel) {
            Ok(s) => Response::ok(id, serde_json::to_value(s).unwrap_or(serde_json::Value::Null)),
            Err(e) => Response::err(id, e),
        }),
        "serialize" => match serde_json::from_value::<SerializeParams>(req.params.clone()) {
            Ok(p) => with_doc(state, req, move |entry, id| match entry.doc.serialize(&p.edited, cancel) {
                Ok(bytes) => {
                    let n = state.next_blob.fetch_add(1, Ordering::SeqCst);
                    let blob_id = format!("b{n}");
                    let path = entry.tmp_dir.join(format!("blob-{blob_id}.bin"));
                    match std::fs::write(&path, &bytes) {
                        Ok(()) => {
                            state.blobs.lock().unwrap().insert(blob_id.clone(), path);
                            Response::ok(id, serde_json::json!({ "blob": blob_id }))
                        }
                        Err(e) => Response::err(id, PluginError::internal(format!("blob 写入失败：{e}"))),
                    }
                }
                Err(e) => Response::err(id, e),
            }),
            Err(e) => handle_err(id, format!("serialize 参数错误：{e}")),
        },
        "close" => with_doc(state, req, |entry, id| {
            entry.doc.close();
            Response::ok(id, serde_json::json!(true))
        }),
        other => Response::err(id, PluginError::unsupported(format!("未知方法：{other}"))),
    }
}

fn with_doc<F>(state: &ServeState, req: &Request, f: F) -> Response
where
    F: FnOnce(&DocEntry, u64) -> Response,
{
    let id = req.id;
    let handle = req.params.get("handle").and_then(|v| v.as_str()).map(String::from);
    let Some(handle) = handle else {
        return Response::err(id, PluginError::internal("缺少 handle"));
    };
    let entry = state.docs.lock().unwrap().get(&handle).cloned();
    match entry {
        Some(entry) => {
            if req.method == "close" {
                state.docs.lock().unwrap().remove(&handle);
            }
            f(&entry, id)
        }
        None => Response::err(id, PluginError::internal(format!("未知 handle：{handle}"))),
    }
}

fn sanitize_relative(p: &str) -> Result<(), PluginError> {
    let path = std::path::Path::new(p);
    if path.is_absolute() || p.contains("..") {
        return Err(PluginError::internal(format!("blob 路径必须是 tmp_dir 内的相对路径：{p}")));
    }
    Ok(())
}

fn relative_to(tmp_dir: &std::path::Path, abs: &std::path::Path) -> Result<String, PluginError> {
    abs.strip_prefix(tmp_dir)
        .map_err(|_| PluginError::internal("blob 不在会话临时目录内"))
        .map(|p| p.to_string_lossy().to_string())
}

#[derive(serde::Deserialize)]
struct ProbeParams {
    source: Source,
}

#[derive(serde::Deserialize)]
struct OpenParams {
    source: Source,
    #[serde(default)]
    opts: OpenOptions,
    tmp_dir: String,
}

#[derive(serde::Deserialize)]
struct PageParams {
    #[allow(dead_code)]
    handle: String,
    range: PageRange,
}

#[derive(serde::Deserialize)]
struct SearchParams {
    #[allow(dead_code)]
    handle: String,
    query: String,
    from: Cursor,
}

#[derive(serde::Deserialize)]
struct BlobParams {
    #[allow(dead_code)]
    handle: String,
    id: String,
}

#[derive(serde::Deserialize)]
struct SerializeParams {
    #[allow(dead_code)]
    handle: String,
    edited: EditSnapshot,
}

/// 生产 sidecar 调用：按 `VW_SIDECAR_IDLE_SECS`（默认 300）空闲退出。
/// `serve` 本身不启动看门狗，便于测试进程内复用。
pub fn enable_idle_exit_from_env() {
    let secs: u64 = std::env::var("VW_SIDECAR_IDLE_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);
    std::thread::spawn(move || {
        let dur = Duration::from_secs(secs);
        loop {
            std::thread::sleep(Duration::from_secs(1));
            if last_idle().map(|idle| idle > dur).unwrap_or(false) {
                std::process::exit(0);
            }
        }
    });
}

// 看门狗读取全局活跃时间：serve 循环更新自己的 state；这里是简化实现，
// 以进程级「最近活动」跟踪。为了避免跨结构共享，用进程启动时间兜底。
static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
static LAST: Mutex<Option<Instant>> = Mutex::new(None);

fn last_idle() -> Option<Duration> {
    let start = START.get_or_init(Instant::now);
    let last = LAST.lock().unwrap().unwrap_or(*start);
    Some(Instant::now().duration_since(last))
}

/// 供宿主/测试记录活动时间（当前实现：仅 watchdog 需要粗粒度信号）。
pub fn note_activity() {
    *LAST.lock().unwrap() = Some(Instant::now());
}
