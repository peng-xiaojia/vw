//! Isolated HTML documents. A mount grants access only to web assets below its directory.
use crate::AppState;
use std::{collections::HashMap, io::Read, path::PathBuf};
use vw_protocol::PluginError;

const LIMIT: u64 = 64 * 1024 * 1024;
pub const CSP: &str = "default-src 'none'; script-src http://vw-preview.localhost vw-preview: 'unsafe-inline'; style-src http://vw-preview.localhost vw-preview: 'unsafe-inline'; img-src http://vw-preview.localhost vw-preview: data: blob:; font-src http://vw-preview.localhost vw-preview: data:; media-src http://vw-preview.localhost vw-preview: blob:; connect-src http://vw-preview.localhost vw-preview:; frame-src http://vw-preview.localhost vw-preview:; object-src 'none'; base-uri http://vw-preview.localhost vw-preview:; form-action 'none'; sandbox allow-scripts";

pub(crate) struct Mount {
    handle: String,
    root: PathBuf,
    entry: PathBuf,
    draft: Option<String>,
    generation: u64,
}
pub(crate) type Mounts = HashMap<String, Mount>;

fn mime(path: &std::path::Path) -> Option<&'static str> {
    Some(match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
        "json" | "map" => "application/json",
        "png" => "image/png", "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif", "webp" => "image/webp", "ico" => "image/x-icon",
        "svg" => "image/svg+xml", "bmp" => "image/bmp", "avif" => "image/avif",
        "woff" => "font/woff", "woff2" => "font/woff2", "ttf" => "font/ttf", "otf" => "font/otf",
        "mp3" => "audio/mpeg", "mp4" => "video/mp4", "webm" => "video/webm", "ogg" => "audio/ogg",
        _ => return None,
    })
}

fn origin() -> &'static str {
    if cfg!(any(windows, target_os = "android")) { "http://vw-preview.localhost" }
    else { "vw-preview://localhost" }
}

impl AppState {
    pub fn html_preview_impl(&self, handle: &str, text: Option<String>, generation: u64) -> Result<String, PluginError> {
        let doc = self.get(handle)?;
        if doc.viewer_id != "html" { return Err(PluginError::permission("仅 HTML 文档允许网页预览")); }
        if text.as_ref().is_some_and(|s| s.len() as u64 > LIMIT) {
            return Err(PluginError::unsupported("HTML 超过 64 MiB"));
        }
        let entry = doc.path.canonicalize().map_err(PluginError::from)?;
        let root = entry.parent().ok_or_else(|| PluginError::permission("网页缺少父目录"))?.to_path_buf();
        let mut mounts = self.html_mounts.lock().unwrap();
        self.get(handle)?; // Closing a handle must not race with creating a new grant.
        let token = mounts.iter().find(|(_, mount)| mount.handle == handle).map(|(key, _)| key.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        if mounts.get(&token).is_none_or(|mount| generation > mount.generation) {
            mounts.insert(token.clone(), Mount { handle: handle.into(), root, entry: entry.clone(), draft: text, generation });
        }
        let name = percent_encoding::utf8_percent_encode(entry.file_name().unwrap().to_str().ok_or_else(|| PluginError::unsupported("网页路径不是 UTF-8"))?, percent_encoding::NON_ALPHANUMERIC);
        Ok(format!("{}/{token}/{name}", origin()))
    }

    pub fn html_resource_impl(&self, path: &str) -> Result<(String, Vec<u8>), PluginError> {
        let (token, encoded) = path.trim_start_matches('/').split_once('/')
            .ok_or_else(|| PluginError::permission("无效网页资源"))?;
        let decoded = percent_encoding::percent_decode_str(encoded).decode_utf8()
            .map_err(|_| PluginError::permission("无效网页路径"))?;
        if decoded.is_empty() || decoded.contains(['\\', ':', '\0'])
            || decoded.split('/').any(|part| part == ".." || part.is_empty()) {
            return Err(PluginError::permission("网页路径越界"));
        }
        let mounts = self.html_mounts.lock().unwrap();
        let mount = mounts.get(token).ok_or_else(|| PluginError::permission("网页已关闭"))?;
        if decoded == "__vw_bridge__.js" {
            return Ok(("text/javascript; charset=utf-8".into(), include_bytes!("../ui/html-frame-bridge.js").to_vec()));
        }
        let resolved = mount.root.join(decoded.as_ref()).canonicalize().map_err(PluginError::from)?;
        if !resolved.starts_with(&mount.root) || !resolved.is_file() {
            return Err(PluginError::permission("网页资源必须在当前文件夹内"));
        }
        let mut content_type = mime(&resolved).ok_or_else(|| PluginError::permission("不支持的网页资源类型"))?;
        let is_draft = resolved == mount.entry && mount.draft.is_some();
        let mut bytes = if is_draft {
            mount.draft.as_ref().unwrap().as_bytes().to_vec()
        } else {
            let file = std::fs::File::open(&resolved).map_err(PluginError::from)?;
            if file.metadata().map_err(PluginError::from)?.len() > LIMIT { return Err(PluginError::unsupported("网页资源超过 64 MiB")); }
            let mut bytes = Vec::new();
            file.take(LIMIT + 1).read_to_end(&mut bytes).map_err(PluginError::from)?;
            bytes
        };
        if bytes.len() as u64 > LIMIT { return Err(PluginError::unsupported("网页资源超过 64 MiB")); }
        if content_type.starts_with("text/html") {
            if is_draft { content_type = "text/html; charset=utf-8"; }
            else if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
                let little = bytes[0] == 0xff;
                let units: Vec<u16> = bytes[2..].chunks_exact(2).map(|unit| if little {
                    u16::from_le_bytes([unit[0], unit[1]])
                } else { u16::from_be_bytes([unit[0], unit[1]]) }).collect();
                bytes = String::from_utf16_lossy(&units).into_bytes();
                content_type = "text/html; charset=utf-8";
            }
            // Append after the original document, preserving its doctype and authored CSS.
            bytes.extend_from_slice(format!("\n<script src=\"{}/{token}/__vw_bridge__.js\"></script>", origin()).as_bytes());
        }
        Ok((content_type.into(), bytes))
    }

    pub(crate) fn revoke_html(&self, handle: &str) {
        self.html_mounts.lock().unwrap().retain(|_, mount| mount.handle != handle);
    }
}

#[tauri::command]
pub async fn vw_html_preview(state: tauri::State<'_, std::sync::Arc<AppState>>, handle: String, text: Option<String>, generation: u64) -> Result<String, String> {
    let state = state.inner().clone();
    crate::blocking(move || state.html_preview_impl(&handle, text, generation)).await
}
