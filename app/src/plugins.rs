//! 插件中心薄桥：与 CLI 共用包校验和安装实现。
use std::{collections::HashMap, sync::{Mutex, atomic::{AtomicU64, Ordering}}};
use serde::Serialize;
use tauri::State;
use vw_protocol::{Manifest, PluginError};
use vw_std::plugin_cmd::{self, PreparedPlugin};
use crate::{AppState, blocking};

#[derive(Default)]
pub struct PluginManager { pending: Mutex<HashMap<String, PreparedPlugin>>, mutation: Mutex<()> }

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginEntry { manifest: Manifest, builtin: bool, loaded: bool, path: Option<String>, error: Option<String> }

#[tauri::command]
pub async fn vw_plugin_market(query: String) -> Result<Vec<plugin_cmd::MarketEntry>, String> {
    blocking(move || plugin_cmd::search_market(&query)).await
}

#[tauri::command]
pub async fn vw_plugins(state: State<'_, std::sync::Arc<AppState>>) -> Result<serde_json::Value, String> {
    let loaded: Vec<Manifest> = state.session.registry().viewers().iter().map(|v| v.manifest().clone()).collect();
    blocking(move || {
        let root = vw_std::plugins_dir();
        let mut entries: Vec<PluginEntry> = loaded.iter().filter(|m| m.transport.kind == vw_protocol::TransportKind::InProcess).map(|m| PluginEntry { manifest: m.clone(), builtin: true, loaded: true, path: None, error: None }).collect();
        for (id, _, path) in plugin_cmd::plugin_list(&root) {
            let manifest: Manifest = serde_json::from_slice(&std::fs::read(path.join("manifest.json"))?).map_err(|e| PluginError::malformed(e.to_string()))?;
            let error = manifest.validate().err();
            entries.push(PluginEntry { loaded: loaded.iter().any(|m| m.id == id && m.version == manifest.version), manifest, builtin: false, path: Some(path.display().to_string()), error });
        }
        Ok(serde_json::json!({ "entries": entries, "directory": root.display().to_string(), "platform": vw_plugin_host_platform() }))
    }).await
}

fn vw_plugin_host_platform() -> &'static str { vw_std::plugin_cmd::current_platform() }

#[tauri::command]
pub async fn vw_plugin_prepare(manager: State<'_, std::sync::Arc<PluginManager>>, spec: Option<String>) -> Result<Option<serde_json::Value>, String> {
    let manager = manager.inner().clone();
    blocking(move || {
        let prepared = if let Some(spec) = spec {
            plugin_cmd::prepare_github(&spec, false)?
        } else {
            let Some(path) = rfd::FileDialog::new().add_filter("vw 插件包", &["vwpkg"]).pick_file() else { return Ok(None); };
            plugin_cmd::prepare_local(&path)?
        };
        // 安装期仅认可完整的当前平台包，普通查看和 CLI 测试不受影响。
        plugin_cmd::require_platform_binary(&prepared)?;
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let token = NEXT.fetch_add(1, Ordering::Relaxed).to_string();
        let result = serde_json::json!({ "token": token, "manifest": prepared.manifest });
        let mut pending = manager.pending.lock().unwrap();
        pending.clear(); pending.insert(token, prepared);
        Ok(Some(result))
    }).await
}

#[tauri::command]
pub async fn vw_plugin_install(manager: State<'_, std::sync::Arc<PluginManager>>, token: String) -> Result<String, String> {
    let manager = manager.inner().clone();
    blocking(move || {
        let _guard = manager.mutation.lock().unwrap();
        let prepared = manager.pending.lock().unwrap().remove(&token).ok_or_else(|| PluginError::malformed("安装预览已失效，请重新选择插件"))?;
        plugin_cmd::extract_vwpkg(&prepared.bytes, &vw_std::plugins_dir())
    }).await
}

#[tauri::command]
pub fn vw_plugin_discard(manager: State<'_, std::sync::Arc<PluginManager>>) { manager.pending.lock().unwrap().clear(); }

#[tauri::command]
pub async fn vw_plugin_remove(manager: State<'_, std::sync::Arc<PluginManager>>, id: String) -> Result<(), String> {
    let manager = manager.inner().clone();
    blocking(move || {
        let _guard = manager.mutation.lock().unwrap();
        plugin_cmd::plugin_remove(&id, &vw_std::plugins_dir())
    }).await
}
