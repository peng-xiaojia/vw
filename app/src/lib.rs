//! vw GUI 外壳（Tauri 2，设计规格 §4.1 / §13 P2）。
//!
//! 命令层是「薄壳」：全部状态与逻辑在 [`AppState`] 的 `*_impl` 函数里，
//! 与 GUI 框架解耦、可被单元测试直接驱动；Tauri 命令只做参数搬运。
//! 与 CLI/MCP 一样，GUI 只消费同一份 Session API——无特权路径。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

use serde::Serialize;
use vw_core::{OpenedDoc, Session};
use vw_protocol::{OpenOptions, PluginError, Source};
mod file_associations;
mod plugins;
mod html_preview;

/// 全局应用状态：一个 Session + 打开的文档句柄表。
pub struct AppState {
    session: Arc<Session>,
    docs: Mutex<HashMap<String, Arc<OpenedDoc>>>,
    startup: Mutex<(bool, Vec<String>)>,
    html_mounts: Mutex<html_preview::Mounts>,
}

impl AppState {
    pub fn new() -> Result<Self, PluginError> {
        let (reg, _hosts) = vw_std::default_registry()?;
        Ok(Self {
            session: Arc::new(Session::new(Arc::new(reg))),
            docs: Mutex::new(HashMap::new()),
            startup: Mutex::new((false, vec![])),
            html_mounts: Mutex::new(HashMap::new()),
        })
    }

    pub fn with_registry(reg: vw_core::Registry) -> Self {
        Self {
            session: Arc::new(Session::new(Arc::new(reg))),
            docs: Mutex::new(HashMap::new()),
            startup: Mutex::new((false, vec![])),
            html_mounts: Mutex::new(HashMap::new()),
        }
    }

    fn get(&self, handle: &str) -> Result<Arc<OpenedDoc>, PluginError> {
        self.docs
            .lock()
            .unwrap()
            .get(handle)
            .cloned()
            .ok_or_else(|| PluginError::internal(format!("未知 handle：{handle}")))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocInfo {
    pub handle: String,
    pub viewer_id: String,
    pub mime: Option<String>,
    pub title: Option<String>,
    pub size: Option<u64>,
    pub pages: Option<u64>,
    /// 展示用源（归档成员带溯源）。
    pub source_display: String,
    pub editable: bool,
    pub path: String,
}

fn info_of(handle: &str, doc: &OpenedDoc) -> DocInfo {
    let editable = doc.edit_capability().is_some();
    let meta = doc.current_meta();
    DocInfo {
        handle: handle.to_string(),
        viewer_id: doc.viewer_id.clone(),
        mime: meta.mime,
        title: meta.title,
        size: meta.size,
        pages: meta.pages,
        source_display: doc.source.display(),
        editable,
        path: doc.path.display().to_string(),
    }
}

impl AppState {
    pub fn open_impl(
        &self,
        path: &str,
        as_viewer: Option<String>,
        inner: Option<String>,
    ) -> Result<DocInfo, PluginError> {
        let opts = OpenOptions {
            as_viewer,
            ..Default::default()
        };
        let doc = match &inner {
            Some(member) => self
                .session
                .open_member(std::path::Path::new(path), member, opts)?,
            None => {
                let p = std::path::Path::new(path);
                let src = if p.is_dir() {
                    Source::Dir {
                        path: p.to_path_buf(),
                    }
                } else {
                    Source::path(p)
                };
                self.session.open(src, opts)?
            }
        };
        let handle = doc.id.clone();
        let info = info_of(&handle, &doc);
        self.docs.lock().unwrap().insert(handle, Arc::new(doc));
        Ok(info)
    }

    pub fn page_impl(
        &self,
        handle: &str,
        start: u64,
        count: u32,
    ) -> Result<serde_json::Value, PluginError> {
        let doc = self.get(handle)?;
        let list = doc.page(vw_protocol::PageRange { start, count })?;
        Ok(serde_json::json!({ "blocks": list.blocks, "total": list.total }))
    }

    pub fn outline_impl(&self, handle: &str) -> Result<Option<vw_protocol::Outline>, PluginError> {
        Ok(self.get(handle)?.outline())
    }

    pub fn search_impl(
        &self,
        handle: &str,
        query: &str,
    ) -> Result<Vec<vw_protocol::SearchHit>, PluginError> {
        let doc = self.get(handle)?;
        let mut hits = Vec::new();
        let mut cursor = vw_protocol::Cursor::start();
        for _ in 0..32 {
            let page = doc.search(query, cursor)?;
            hits.extend(page.hits.into_iter().take(500 - hits.len()));
            if hits.len() >= 500 {
                break;
            }
            match page.next {
                Some(c) if (c.block, c.offset) > (cursor.block, cursor.offset) => cursor = c,
                Some(_) => return Err(PluginError::malformed("插件搜索游标未前进")),
                None => break,
            }
        }
        Ok(hits)
    }

    pub fn search_page_impl(&self, handle: &str, query: &str, from: Option<vw_protocol::Cursor>) -> Result<vw_protocol::SearchHits, PluginError> {
        let cursor = from.unwrap_or_else(vw_protocol::Cursor::start);
        let mut page = self.get(handle)?.search(query, cursor)?;
        if page.hits.len() > 500 { return Err(PluginError::malformed("插件搜索单页超过 500 条")); }
        if page.next.is_some_and(|next| (next.block, next.offset) <= (cursor.block, cursor.offset)) {
            return Err(PluginError::malformed("插件搜索游标未前进"));
        }
        page.hits.truncate(500);
        Ok(page)
    }

    pub fn snapshot_impl(&self, handle: &str) -> Result<String, PluginError> {
        self.get(handle)?.snapshot_text()
    }

    pub fn save_impl(&self, handle: &str, text: &str) -> Result<SaveReportDto, PluginError> {
        let doc = self.get(handle)?;
        let r = doc.save_text(text.to_string())?;
        Ok(SaveReportDto {
            bytes: r.bytes,
            backup: r.backup.map(|b| b.display().to_string()),
            info: info_of(handle, &doc),
        })
    }

    pub fn close_impl(&self, handle: &str) -> Result<(), PluginError> {
        // Drop the docs lock before taking the mount lock (prepare takes them in reverse order).
        let doc = self.docs.lock().unwrap().remove(handle);
        self.revoke_html(handle);
        if let Some(doc) = doc {
            doc.close();
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveReportDto {
    pub bytes: u64,
    pub backup: Option<String>,
    pub info: DocInfo,
}

impl AppState {
    pub fn blob_impl(&self, handle: &str, id: &str) -> Result<(String, Vec<u8>), PluginError> {
        use std::io::Read;
        let doc = self.get(handle)?;
        let (blob, path) = doc.resolve_blob(id)?;
        // Defence in depth, also for in-process plugins.
        let canonical = path.canonicalize().map_err(PluginError::from)?;
        let root = vw_core::tmp_root()
            .canonicalize()
            .map_err(PluginError::from)?;
        if !canonical.starts_with(root) || !canonical.is_file() {
            return Err(PluginError::permission("资源必须位于会话临时目录内"));
        }
        let mime = blob
            .mime
            .ok_or_else(|| PluginError::unsupported("资源缺少媒体类型"))?;
        if !matches!(
            mime.as_str(),
            "image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/bmp"
        ) {
            return Err(PluginError::unsupported(
                "不允许该资源类型（包括 SVG 和 HTML）",
            ));
        }
        const LIMIT: u64 = 64 * 1024 * 1024;
        let file = std::fs::File::open(canonical).map_err(PluginError::from)?;
        if file.metadata().map_err(PluginError::from)?.len() > LIMIT {
            return Err(PluginError::unsupported("图片资源超过 64 MiB"));
        }
        let mut bytes = Vec::new();
        file.take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(PluginError::from)?;
        if bytes.len() as u64 > LIMIT {
            return Err(PluginError::unsupported("资源超出限制"));
        }
        Ok((mime, bytes))
    }

    /// Export is a copy of the opened source, including materialized archive members.
    /// Create-new semantics deliberately reject accidental replacement of another file.
    pub fn export_impl(&self, handle: &str, path: &str) -> Result<u64, PluginError> {
        use std::io::Write;
        let doc = self.get(handle)?;
        if !doc.path.is_file() {
            return Err(PluginError::unsupported("目录不能导出为文件"));
        }
        let mut source = std::fs::File::open(&doc.path).map_err(PluginError::from)?;
        let mut target = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(PluginError::from)?;
        match std::io::copy(&mut source, &mut target).and_then(|n| {
            target.flush()?;
            target.sync_all()?;
            Ok(n)
        }) {
            Ok(n) => Ok(n),
            Err(e) => {
                drop(target);
                let _ = std::fs::remove_file(path);
                Err(e.into())
            }
        }
    }

    pub fn startup_paths_impl(&self) -> Vec<String> {
        let mut pending = self.startup.lock().unwrap();
        pending.0 = true;
        std::mem::take(&mut pending.1)
    }
}

async fn blocking<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, PluginError> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(job)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn vw_open(
    state: tauri::State<'_, Arc<AppState>>,
    path: String,
    as_viewer: Option<String>,
    inner: Option<String>,
) -> Result<DocInfo, String> {
    let state = state.inner().clone();
    blocking(move || state.open_impl(&path, as_viewer, inner)).await
}
#[tauri::command]
async fn vw_page(
    state: tauri::State<'_, Arc<AppState>>,
    handle: String,
    start: u64,
    count: u32,
) -> Result<serde_json::Value, String> {
    let state = state.inner().clone();
    blocking(move || state.page_impl(&handle, start, count.min(1000))).await
}
#[tauri::command]
async fn vw_outline(
    state: tauri::State<'_, Arc<AppState>>,
    handle: String,
) -> Result<Option<vw_protocol::Outline>, String> {
    let state = state.inner().clone();
    blocking(move || state.outline_impl(&handle)).await
}
#[tauri::command]
async fn vw_search(
    state: tauri::State<'_, Arc<AppState>>,
    handle: String,
    query: String,
) -> Result<Vec<vw_protocol::SearchHit>, String> {
    let state = state.inner().clone();
    blocking(move || state.search_impl(&handle, &query)).await
}
#[tauri::command]
async fn vw_search_page(state: tauri::State<'_, Arc<AppState>>, handle: String, query: String, from: Option<vw_protocol::Cursor>) -> Result<vw_protocol::SearchHits, String> {
    let state = state.inner().clone();
    blocking(move || state.search_page_impl(&handle, &query, from)).await
}
#[tauri::command]
async fn vw_snapshot(
    state: tauri::State<'_, Arc<AppState>>,
    handle: String,
) -> Result<String, String> {
    let state = state.inner().clone();
    blocking(move || state.snapshot_impl(&handle)).await
}
#[tauri::command]
async fn vw_preview_markdown(text: String) -> Result<serde_json::Value, String> {
    blocking(move || {
        if text.len() > vw_plugin_markdown::EDIT_MAX_BYTES as usize {
            return Err(PluginError::unsupported("Markdown 预览超出大小上限"));
        }
        let blocks = vw_plugin_markdown::parse_markdown(&text);
        let outline = vw_plugin_markdown::outline_of(&blocks);
        Ok(serde_json::json!({ "blocks": blocks, "outline": outline }))
    }).await
}
#[tauri::command]
async fn vw_save(
    state: tauri::State<'_, Arc<AppState>>,
    handle: String,
    text: String,
) -> Result<SaveReportDto, String> {
    let state = state.inner().clone();
    blocking(move || state.save_impl(&handle, &text)).await
}
#[tauri::command]
async fn vw_close(state: tauri::State<'_, Arc<AppState>>, handle: String) -> Result<(), String> {
    let state = state.inner().clone();
    blocking(move || state.close_impl(&handle)).await
}
#[tauri::command]
async fn vw_list_dir(path: String) -> Result<vw_plugin_dir::DirectoryListing, String> {
    blocking(move || vw_plugin_dir::list_directory(Path::new(&path))).await
}
#[tauri::command]
fn vw_startup_paths(state: tauri::State<'_, Arc<AppState>>) -> Vec<String> {
    state.startup_paths_impl()
}
#[tauri::command]
async fn vw_pick_path(directory: bool) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dialog = rfd::FileDialog::new().set_title(if directory {
            "选择目录"
        } else {
            "打开文件"
        });
        let selected = if directory {
            dialog.pick_folder()
        } else {
            dialog.pick_file()
        };
        selected.map(|path| path.display().to_string())
    })
    .await
    .map_err(|e| e.to_string())
}
#[tauri::command]
async fn vw_pick_files() -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        rfd::FileDialog::new().set_title("打开文件（可多选）").pick_files().unwrap_or_default()
            .into_iter().map(|path| path.display().to_string()).collect()
    }).await.map_err(|error| error.to_string())
}
#[tauri::command]
async fn vw_export(
    state: tauri::State<'_, Arc<AppState>>,
    handle: String,
    path: Option<String>,
) -> Result<Option<u64>, String> {
    let state = state.inner().clone();
    blocking(move || {
        let path = path.map(PathBuf::from).or_else(|| {
            rfd::FileDialog::new()
                .set_title("导出副本（请选择新文件名）")
                .save_file()
        });
        path.map(|path| state.export_impl(&handle, &path.display().to_string()))
            .transpose()
    })
    .await
}

pub fn validate_external_url(url: &str) -> Result<tauri::Url, PluginError> {
    let parsed = tauri::Url::parse(url).map_err(|_| PluginError::malformed("链接格式无效"))?;
    if url.chars().any(|c| c.is_control())
        || !matches!(parsed.scheme(), "http" | "https" | "mailto")
    {
        return Err(PluginError::permission(
            "仅允许 http、https 和 mailto 外部链接",
        ));
    }
    if matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_none() {
        return Err(PluginError::malformed("链接缺少主机名"));
    }
    Ok(parsed)
}
#[tauri::command]
async fn vw_open_external(url: String) -> Result<(), String> {
    blocking(move || {
        let url = validate_external_url(&url)?;
        #[cfg(windows)]
        {
            let wide: Vec<u16> = url.as_str().encode_utf16().chain(Some(0)).collect();
            let result = unsafe {
                windows_sys::Win32::UI::Shell::ShellExecuteW(
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    wide.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    1,
                )
            };
            if result as isize <= 32 {
                return Err(PluginError::internal("无法打开外部链接"));
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            let _ = url;
            Err(PluginError::unsupported("当前平台尚不支持外部浏览器"))
        }
    })
    .await
}

/// Resolve command-line paths against the launching instance's cwd, never this process's cwd.
pub fn launch_paths(args: impl IntoIterator<Item = String>, cwd: &Path) -> Vec<String> {
    args.into_iter()
        .skip(1)
        .filter(|s| !s.is_empty() && s != "--")
        .map(|arg| {
            let path = PathBuf::from(arg);
            if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            }
            .display()
            .to_string()
        })
        .collect()
}

fn writable_directory(path: &Path) -> bool {
    if std::fs::create_dir_all(path).is_err() {
        return false;
    }
    let probe = path.join(format!(".vw-write-{}", std::process::id()));
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(file) => {
            drop(file);
            let _ = std::fs::remove_file(probe);
            true
        }
        Err(_) => false,
    }
}

pub fn webview_data_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("VW_WEBVIEW_DATA").filter(|p| !p.is_empty()) {
        return PathBuf::from(path);
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    let parent = exe.parent().unwrap_or_else(|| Path::new("."));
    let portable = parent.join("webview-data");
    if parent.join("vw-portable.marker").is_file() && writable_directory(&portable) {
        return portable;
    }
    let user = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("vw")
        .join("webview-data");
    if writable_directory(&user) {
        user
    } else {
        std::env::temp_dir().join("vw").join("webview-data")
    }
}

pub fn startup_log(message: &str) {
    use std::io::Write;
    let root = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("vw")
        .join("logs");
    let _ = std::fs::create_dir_all(&root);
    if let Ok(mut log) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("startup.log"))
    {
        let _ = writeln!(log, "{:?} {}", std::time::SystemTime::now(), message);
    }
}

pub fn show_startup_error(message: &str) {
    startup_log(message);
    rfd::MessageDialog::new()
        .set_title("vw 启动失败")
        .set_description(message)
        .set_level(rfd::MessageLevel::Error)
        .show();
}

pub fn run_app() -> tauri::Result<()> {
    startup_log("starting desktop");
    let state = Arc::new(AppState::new().map_err(|e| tauri::Error::Anyhow(e.into()))?);
    state.startup.lock().unwrap().1 = launch_paths(
        std::env::args_os().map(|s| s.to_string_lossy().into_owned()),
        &std::env::current_dir()?,
    );
    tauri::Builder::default()
        .on_menu_event(|app, event| { let _ = app.emit("vw-menu", event.id().as_ref()); })
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            let paths = launch_paths(args, Path::new(&cwd));
            if let Some(state) = app.try_state::<Arc<AppState>>() {
                let mut pending = state.startup.lock().unwrap();
                if pending.0 {
                    let _ = app.emit("vw-open-paths", &paths);
                } else {
                    pending.1.extend(paths);
                }
            }
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .manage(state)
        .manage(Arc::new(plugins::PluginManager::default()))
        .register_asynchronous_uri_scheme_protocol("vw-preview", |context, request, responder| {
            let app = context.app_handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                let result = if request.method() == tauri::http::Method::GET {
                    app.state::<Arc<AppState>>().html_resource_impl(request.uri().path())
                } else { Err(PluginError::permission("仅允许 GET")) };
                let (status, mime, bytes) = match result {
                    Ok((mime, bytes)) => (200, mime, bytes),
                    Err(_) => (403, "text/plain; charset=utf-8".into(), "资源不存在或超出此网页的可访问范围".as_bytes().to_vec()),
                };
                if let Ok(response) = tauri::http::Response::builder().status(status)
                    .header("Content-Type", mime).header("X-Content-Type-Options", "nosniff")
                    .header("Cache-Control", "no-store").header("Access-Control-Allow-Origin", "*")
                    .header("Content-Security-Policy", html_preview::CSP).body(bytes) {
                    responder.respond(response);
                }
            });
        })
        .register_asynchronous_uri_scheme_protocol("vw-blob", |context, request, responder| {
            let app = context.app_handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                let result = (|| {
                    if request.method() != tauri::http::Method::GET {
                        return Err(PluginError::permission("仅允许 GET"));
                    }
                    let (handle, encoded) = request
                        .uri()
                        .path()
                        .trim_start_matches('/')
                        .split_once('/')
                        .ok_or_else(|| PluginError::malformed("无效资源引用"))?;
                    let id = percent_encoding::percent_decode_str(encoded)
                        .decode_utf8()
                        .map_err(|_| PluginError::malformed("资源 ID 非 UTF-8"))?;
                    app.state::<Arc<AppState>>().blob_impl(handle, &id)
                })();
                let response = match result {
                    Ok((mime, bytes)) => tauri::http::Response::builder()
                        .status(200)
                        .header("Content-Type", mime)
                        .header("X-Content-Type-Options", "nosniff")
                        .header("Cache-Control", "no-store")
                        .header("Content-Security-Policy", "default-src 'none'; sandbox")
                        .body(bytes),
                    Err(_) => tauri::http::Response::builder()
                        .status(403)
                        .header("Content-Type", "text/plain")
                        .body(b"Resource denied".to_vec()),
                };
                if let Ok(response) = response {
                    responder.respond(response);
                }
            });
        })
        .invoke_handler(tauri::generate_handler![
            vw_open,
            vw_page,
            vw_outline,
            vw_search,
            vw_search_page,
            vw_snapshot,
            vw_preview_markdown,
            vw_save,
            vw_close,
            vw_pick_path,
            vw_pick_files,
            html_preview::vw_html_preview,
            vw_list_dir,
            vw_startup_paths,
            vw_open_external,
            vw_export
            ,plugins::vw_plugins
            ,plugins::vw_plugin_market
            ,plugins::vw_plugin_prepare
            ,plugins::vw_plugin_install
            ,plugins::vw_plugin_discard
            ,plugins::vw_plugin_remove
            ,file_associations::vw_association_state
            ,file_associations::vw_association_apply
            ,file_associations::vw_association_defaults
        ])
        .setup(|app| {
            let data_dir = webview_data_dir();
            std::fs::create_dir_all(&data_dir)?;
            startup_log(&format!("WebView2 data: {}", data_dir.display()));
            tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("vw —— 文件查看器")
            .decorations(false)
            .inner_size(1100.0, 760.0)
            .min_inner_size(720.0, 480.0)
            .data_directory(data_dir)
            .on_navigation(|url| {
                (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
                    || (matches!(url.scheme(), "http" | "https")
                        && url.host_str() == Some("tauri.localhost"))
            })
            .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
            .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
}
