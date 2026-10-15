//! vw 标准插件集：所有外壳（CLI/GUI/MCP）共用的注册表装配。
//!
//! 内置插件在此注册；外部 sidecar 从 `VW_PLUGIN_DIR`（默认平台插件目录）
//! 的 `manifest.json` 装载——目录式安装是 P4 GitHub Releases 的落点。

use std::path::PathBuf;
use std::sync::Arc;

use vw_core::{Registry, Viewer};
use vw_plugin_archive::ArchiveViewer;
use vw_plugin_csv::CsvViewer;
use vw_plugin_dir::DirViewer;
use vw_plugin_hex::HexViewer;
use vw_plugin_host::{HostConfig, HostViewer};
use vw_plugin_image::ImageViewer;
use vw_plugin_json::JsonViewer;
use vw_plugin_markdown::MarkdownViewer;
use vw_plugin_office::OfficeViewer;
use vw_plugin_pdf::PdfViewer;
use vw_plugin_text::TextViewer;
use vw_protocol::{Manifest, PluginError, TransportKind};
pub mod plugin_cmd;
/// 注册全部内置插件。
pub fn register_builtins(reg: &mut Registry) {
    reg.register(Arc::new(DirViewer::new(60)));
    reg.register(Arc::new(ArchiveViewer::new(55)));
    reg.register(Arc::new(ImageViewer::new(70)));
    reg.register(Arc::new(OfficeViewer::new(75)));
    if cfg!(windows) {
        reg.register(Arc::new(PdfViewer::new(80)));
    }
    reg.register(Arc::new(TextViewer::new(
        &["txt", "log", "ini", "toml", "cfg"],
        50,
    )));
    reg.register(Arc::new(TextViewer::html()));
    reg.register(Arc::new(MarkdownViewer::new(&["md", "markdown"], 50)));
    reg.register(Arc::new(JsonViewer::new(&["json", "jsonl"], 50)));
    reg.register(Arc::new(CsvViewer::new(&["csv", "tsv"], 50)));
    reg.register(Arc::new(HexViewer::new(0)));
}

/// 内置插件 + 外部 sidecar 目录装载。
pub fn default_registry() -> Result<(Registry, Vec<Arc<HostViewer>>), PluginError> {
    let mut reg = Registry::new();
    register_builtins(&mut reg);
    let hosts = attach_external(&mut reg, &plugins_dir())?;
    Ok((reg, hosts))
}

/// 外部插件目录（`VW_PLUGIN_DIR` 覆盖；默认 `<平台数据目录>/vw/plugins`，§7.1）。
pub fn plugins_dir() -> PathBuf {
    if let Ok(d) = std::env::var("VW_PLUGIN_DIR") {
        if !d.is_empty() {
            return PathBuf::from(d);
        }
    }
    if let Ok(d) = std::env::var("VW_HOME") {
        return PathBuf::from(d).join("plugins");
    }
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let mut p = PathBuf::from(home);
        if cfg!(windows) {
            p.push("AppData");
            p.push("Roaming");
        } else {
            p.push(".local");
            p.push("share");
        }
        p.push("vw");
        p.push("plugins");
        return p;
    }
    PathBuf::from(".vw/plugins")
}

/// 从目录装载外部 sidecar 插件（每个子目录一个 manifest.json）。
/// 解析失败或校验不过的插件只告警不阻断。
pub fn attach_external(
    reg: &mut Registry,
    dir: &std::path::Path,
) -> Result<Vec<Arc<HostViewer>>, PluginError> {
    let mut hosts = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(hosts);
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let manifest_path = entry.path().join("manifest.json");
        let Ok(raw) = std::fs::read_to_string(&manifest_path) else {
            continue;
        };
        let manifest: Manifest = match serde_json::from_str(&raw) {
            Ok(m) => m,
            Err(e) => {
                eprintln!(
                    "warn: 跳过 {}：manifest 解析失败：{e}",
                    entry.path().display()
                );
                continue;
            }
        };
        if let Err(e) = manifest.validate() {
            eprintln!("warn: 跳过 {}：{e}", entry.path().display());
            continue;
        }
        if manifest.transport.kind != TransportKind::StdioJsonrpc {
            continue;
        }
        let mut m2 = manifest.clone();
        // 先展开平台，再以安装目录解析，不能相对于 GUI 的启动目录。
        if let Some(exe) = &m2.transport.executable {
                let expanded = exe.replace("{platform}", vw_plugin_host::platform_tag()).replace("{ext}", if cfg!(windows) { ".exe" } else { "" });
                let p = PathBuf::from(expanded);
                if p.is_relative() {
                    m2.transport.executable =
                        Some(entry.path().join(p).to_string_lossy().to_string());
                }
        }
        match HostViewer::from_manifest(m2, HostConfig::default()) {
            Ok(h) => {
                let hv = Arc::new(h);
                let erased: Arc<dyn Viewer> = hv.clone();
                reg.register(erased);
                hosts.push(hv);
            }
            Err(e) => eprintln!("warn: 插件 {} 不可用：{e}", manifest.id),
        }
    }
    Ok(hosts)
}
