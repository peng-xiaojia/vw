//! vw GUI 外壳（Tauri 2，设计规格 §4.1 / §13 P2）。
//!
//! 命令层是「薄壳」：全部状态与逻辑在 [`AppState`] 的 `*_impl` 函数里，
//! 与 GUI 框架解耦、可被单元测试直接驱动；Tauri 命令只做参数搬运。
//! 与 CLI/MCP 一样，GUI 只消费同一份 Session API——无特权路径。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use vw_core::{OpenedDoc, Session};
use vw_protocol::{OpenOptions, PluginError, Source};

/// 全局应用状态：一个 Session + 打开的文档句柄表。
pub struct AppState {
    session: Arc<Session>,
    docs: Mutex<HashMap<String, Arc<OpenedDoc>>>,
}

impl AppState {
    pub fn new() -> Result<Self, PluginError> {
        let (reg, _hosts) = vw_std::default_registry()?;
        Ok(Self { session: Arc::new(Session::new(Arc::new(reg))), docs: Mutex::new(HashMap::new()) })
    }

    pub fn with_registry(reg: vw_core::Registry) -> Self {
        Self { session: Arc::new(Session::new(Arc::new(reg))), docs: Mutex::new(HashMap::new()) }
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
    DocInfo {
        handle: handle.to_string(),
        viewer_id: doc.viewer_id.clone(),
        mime: doc.meta.mime.clone(),
        title: doc.meta.title.clone(),
        size: doc.meta.size,
        pages: doc.meta.pages,
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
        let opts = OpenOptions { as_viewer, ..Default::default() };
        let doc = match &inner {
            Some(member) => self.session.open_member(std::path::Path::new(path), member, opts)?,
            None => {
                let p = std::path::Path::new(path);
                let src = if p.is_dir() {
                    Source::Dir { path: p.to_path_buf() }
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
        loop {
            let page = doc.search(query, cursor)?;
            hits.extend(page.hits);
            match page.next {
                Some(c) => cursor = c,
                None => break,
            }
            if hits.len() >= 500 {
                break;
            }
        }
        Ok(hits)
    }

    pub fn snapshot_impl(&self, handle: &str) -> Result<String, PluginError> {
        self.get(handle)?.snapshot_text()
    }

    pub fn save_impl(&self, handle: &str, text: &str) -> Result<SaveReportDto, PluginError> {
        let doc = self.get(handle)?;
        let r = doc.save_text(text.to_string())?;
        Ok(SaveReportDto { bytes: r.bytes, backup: r.backup.map(|b| b.display().to_string()) })
    }

    pub fn close_impl(&self, handle: &str) -> Result<(), PluginError> {
        if let Some(doc) = self.docs.lock().unwrap().remove(handle) {
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
}

// ---------- Tauri 命令薄壳 ----------

#[tauri::command]
fn vw_open(
    state: tauri::State<AppState>,
    path: String,
    as_viewer: Option<String>,
    inner: Option<String>,
) -> Result<DocInfo, String> {
    state.open_impl(&path, as_viewer, inner).map_err(|e| e.to_string())
}

#[tauri::command]
fn vw_page(
    state: tauri::State<AppState>,
    handle: String,
    start: u64,
    count: u32,
) -> Result<serde_json::Value, String> {
    state.page_impl(&handle, start, count).map_err(|e| e.to_string())
}

#[tauri::command]
fn vw_outline(state: tauri::State<AppState>, handle: String) -> Result<Option<vw_protocol::Outline>, String> {
    state.outline_impl(&handle).map_err(|e| e.to_string())
}

#[tauri::command]
fn vw_search(
    state: tauri::State<AppState>,
    handle: String,
    query: String,
) -> Result<Vec<vw_protocol::SearchHit>, String> {
    state.search_impl(&handle, &query).map_err(|e| e.to_string())
}

#[tauri::command]
fn vw_snapshot(state: tauri::State<AppState>, handle: String) -> Result<String, String> {
    state.snapshot_impl(&handle).map_err(|e| e.to_string())
}

#[tauri::command]
fn vw_save(state: tauri::State<AppState>, handle: String, text: String) -> Result<SaveReportDto, String> {
    state.save_impl(&handle, &text).map_err(|e| e.to_string())
}

#[tauri::command]
fn vw_close(state: tauri::State<AppState>, handle: String) -> Result<(), String> {
    state.close_impl(&handle).map_err(|e| e.to_string())
}

pub fn run_app() -> tauri::Result<()> {
    let state = AppState::new().expect("初始化插件注册表失败");
    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            vw_open, vw_page, vw_outline, vw_search, vw_snapshot, vw_save, vw_close
        ])
        .run(tauri::generate_context!())
}
