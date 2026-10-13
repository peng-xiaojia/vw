//! vw CLI 外壳（设计规格 §4.1）：与 GUI/MCP 走完全相同的 Session API。

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use vw_core::render::json::render_json;
use vw_core::render::text::{render_blocks, render_document};
use vw_core::registry::viewer_id_of;
use vw_core::{Registry, Session};
use vw_plugin_csv::CsvViewer;
use vw_plugin_hex::HexViewer;
use vw_plugin_host::{HostConfig, HostViewer};
use vw_plugin_json::JsonViewer;
use vw_plugin_markdown::MarkdownViewer;
use vw_plugin_text::TextViewer;
use vw_protocol::{ErrorKind, Manifest, OpenOptions, PluginError, Source, TransportKind};

#[derive(Parser)]
#[command(name = "vw", version, about = "vw —— 插件化文件查看器（CLI）")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 查看文件（默认文本渲染；--json 输出 IR；--raw 输出原文）
    View {
        path: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        raw: bool,
        /// 指定 viewer id，绕过探测仲裁
        #[arg(long = "as", value_name = "VIEWER")]
        as_viewer: Option<String>,
        /// 起始块（0 起）
        #[arg(long, default_value_t = 0)]
        from: u64,
        /// 最多取多少块（0 = 全部，上限 100_000）
        #[arg(long, default_value_t = 0)]
        count: u64,
    },
    /// 保存文件：新内容来自文件或 stdin（保存管线的 conformance 驱动器）
    Save {
        path: PathBuf,
        /// 从该文件读取新内容；省略则读 stdin
        #[arg(long = "from", value_name = "FILE")]
        from: Option<PathBuf>,
        /// 指定 viewer id
        #[arg(long = "as", value_name = "VIEWER")]
        as_viewer: Option<String>,
    },
    /// 查看探测打分与仲裁结果
    Probe {
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// 列出已注册插件
    Plugins {
        #[arg(long)]
        json: bool,
    },
    /// 诊断：外部插件进程状态与 stderr 尾部（§9）
    Doctor,
}

/// 组装注册表：五个内置插件 + `VW_PLUGIN_DIR` 下的外部 sidecar。
fn build_registry() -> Result<(Registry, Vec<Arc<HostViewer>>), PluginError> {
    let mut reg = Registry::new();
    reg.register(Arc::new(TextViewer::new(&["txt", "ini", "toml", "cfg"], 50)));
    reg.register(Arc::new(MarkdownViewer::new(&["md", "markdown"], 50)));
    reg.register(Arc::new(JsonViewer::new(&["json", "jsonl"], 50)));
    reg.register(Arc::new(CsvViewer::new(&["csv", "tsv"], 50)));
    reg.register(Arc::new(HexViewer::new(0)));

    let mut hosts = Vec::new();
    let dir = std::env::var("VW_PLUGIN_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs_plugin_root().join("plugins"));
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let manifest_path = entry.path().join("manifest.json");
            let Ok(raw) = std::fs::read_to_string(&manifest_path) else { continue };
            let manifest: Manifest = match serde_json::from_str(&raw) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("warn: 跳过 {}：manifest 解析失败：{e}", entry.path().display());
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
            // 相对 executable 以插件目录为基准解析（不含占位符时）
            if let Some(exe) = &m2.transport.executable {
                if !exe.contains("{platform}") {
                    let p = PathBuf::from(exe);
                    if p.is_relative() {
                        m2.transport.executable =
                            Some(entry.path().join(p).to_string_lossy().to_string());
                    }
                }
            }
            match HostViewer::from_manifest(m2, HostConfig::default()) {
                Ok(h) => {
                    let arc = Arc::new(h);
                    reg.register(arc.clone());
                    hosts.push(arc);
                }
                Err(e) => eprintln!("warn: 插件 {} 不可用：{e}", manifest.id),
            }
        }
    }
    Ok((reg, hosts))
}

fn dirs_plugin_root() -> PathBuf {
    if let Ok(d) = std::env::var("VW_HOME") {
        return PathBuf::from(d);
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
        return p;
    }
    PathBuf::from(".vw")
}

fn err_exit_code(e: &PluginError) -> ExitCode {
    eprintln!("error[{:?}]: {}", e.kind, e.message);
    match e.kind {
        ErrorKind::Conflict => ExitCode::from(3),
        _ => ExitCode::from(1),
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => err_exit_code(&e),
    }
}

fn run(cli: Cli) -> Result<(), PluginError> {
    match cli.cmd {
        Cmd::View { path, json, raw, as_viewer, from: _from, count } => {
            let (reg, hosts) = build_registry()?;
            let _keep_alive = hosts;
            let session = Session::new(Arc::new(reg));
            let opts = OpenOptions { as_viewer, ..Default::default() };
            let doc = session.open(Source::path(&path), opts)?;
            let max_blocks = if count == 0 { 100_000 } else { count as usize };
            let all = doc.page_all(max_blocks)?;
            let shown = all.blocks.len();
            let total = all.total.unwrap_or(shown as u64);
            let truncated = ((shown as u64) < total).then_some((shown, total));
            if json {
                println!("{}", render_json(&doc.meta, &all.blocks, all.total));
            } else if raw {
                // 原文：可编辑文档直接取快照；否则回退文本渲染
                match doc.snapshot_text() {
                    Ok(text) => print!("{text}"),
                    Err(_) => print!("{}", render_blocks(&all.blocks)),
                }
            } else {
                print!("{}", render_document(&doc.meta, &all.blocks, truncated));
            }
            doc.close();
            Ok(())
        }
        Cmd::Save { path, from, as_viewer } => {
            let (reg, hosts) = build_registry()?;
            let _keep_alive = hosts;
            let session = Session::new(Arc::new(reg));
            let opts = OpenOptions { as_viewer, ..Default::default() };
            let doc = session.open(Source::path(&path), opts)?;
            let text = match from {
                Some(f) => std::fs::read_to_string(&f)
                    .map_err(|e| PluginError::malformed(format!("读取 {f:?} 失败：{e}")))?,
                None => {
                    let mut buf = String::new();
                    std::io::stdin().read_to_string(&mut buf)
                        .map_err(|e| PluginError::malformed(format!("stdin 非有效 UTF-8：{e}")))?;
                    buf
                }
            };
            let report = doc.save_text(text)?;
            eprintln!(
                "saved {} bytes → {}{}",
                report.bytes,
                path.display(),
                report.backup.as_ref().map(|b| format!("（备份 {}）", b.display())).unwrap_or_default()
            );
            doc.close();
            Ok(())
        }
        Cmd::Probe { path, json } => {
            let (reg, hosts) = build_registry()?;
            let _keep_alive = hosts;
            let src = Source::path(&path);
            let scored = reg.probe_all(&src);
            if json {
                let items: Vec<_> = scored
                    .iter()
                    .map(|(v, s)| {
                        serde_json::json!({
                            "viewer": viewer_id_of(v),
                            "plugin": v.manifest().id,
                            "score": s.score,
                            "reason": s.reason,
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&items).unwrap());
            } else {
                for (v, s) in &scored {
                    println!("{:>3}  {:<12} {}", s.score, viewer_id_of(v), s.reason);
                }
            }
            Ok(())
        }
        Cmd::Plugins { json } => {
            let (reg, _) = build_registry()?;
            let rows: Vec<_> = reg
                .viewers()
                .iter()
                .map(|v| {
                    let m = v.manifest();
                    serde_json::json!({
                        "id": m.id,
                        "name": m.name,
                        "version": m.version,
                        "transport": format!("{:?}", m.transport.kind),
                        "viewers": m.contributes.viewers.iter().map(|d| d.id.clone()).collect::<Vec<_>>(),
                    })
                })
                .collect();
            if json {
                println!("{}", serde_json::to_string_pretty(&rows).unwrap());
            } else {
                println!("{:<22} {:<8} {:<16} viewers", "plugin", "version", "transport");
                for r in &rows {
                    println!(
                        "{:<22} {:<8} {:<16} {}",
                        r["id"].as_str().unwrap_or(""),
                        r["version"].as_str().unwrap_or(""),
                        r["transport"].as_str().unwrap_or(""),
                        r["viewers"].as_array().map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(",")).unwrap_or_default(),
                    );
                }
            }
            Ok(())
        }
        Cmd::Doctor => {
            let (_, hosts) = build_registry()?;
            println!("外部插件数：{}", hosts.len());
            for h in &hosts {
                let d = h.diagnostics();
                println!("— {}：crashed={}", d.plugin_id, d.crashed);
                for line in d.stderr.iter().take(5) {
                    println!("    stderr: {line}");
                }
            }
            Ok(())
        }
    }
}
