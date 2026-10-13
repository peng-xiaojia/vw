//! vw CLI 外壳（设计规格 §4.1）：与 GUI/MCP 走完全相同的 Session API。

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use vw_cli::plugin_cmd;

use vw_core::render::json::render_json;
use vw_core::render::text::{render_blocks, render_document};
use vw_core::registry::viewer_id_of;
use vw_core::Session;
use vw_plugin_host::HostViewer;
use vw_protocol::{ErrorKind, OpenOptions, PluginError, Source};
use vw_std::default_registry;

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
        /// 归档成员路径：把 <path> 当作归档（zip）打开该成员（§5.1）
        #[arg(long = "inner", value_name = "MEMBER")]
        inner: Option<String>,
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
    /// 插件分发（§8）：add / remove / list / new
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },
}

#[derive(Subcommand)]
enum PluginAction {
    /// 从 GitHub Releases 安装：github:owner/repo[@tag]
    Add {
        spec: String,
        /// 跳过 sha256 校验（不建议）
        #[arg(long)]
        insecure: bool,
    },
    /// 卸载插件
    Remove { id: String },
    /// 列出已安装的外部插件
    List,
    /// 生成插件脚手架（含多平台 release CI 模板）
    New { name: String },
}

fn build_registry() -> Result<(vw_core::Registry, Vec<Arc<HostViewer>>), PluginError> {
    default_registry()
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
        Cmd::View { path, json, raw, as_viewer, inner, from: _from, count } => {
            let (reg, hosts) = build_registry()?;
            let _keep_alive = hosts;
            let session = Session::new(Arc::new(reg));
            let opts = OpenOptions { as_viewer, ..Default::default() };
            let doc = match &inner {
                Some(member) => session.open_member(&path, member, opts)?,
                None => {
                    let src = if path.is_dir() {
                        Source::Dir { path: path.clone() }
                    } else {
                        Source::path(&path)
                    };
                    session.open(src, opts)?
                }
            };
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
        Cmd::Plugin { action } => match action {
            PluginAction::Add { spec, insecure } => {
                let dir = vw_std::plugins_dir();
                let id = plugin_cmd::plugin_add(&spec, &dir, insecure)?;
                println!("已安装：{id}");
                Ok(())
            }
            PluginAction::Remove { id } => {
                let dir = vw_std::plugins_dir().join(&id);
                if dir.is_dir() {
                    std::fs::remove_dir_all(&dir)
                        .map_err(|e| PluginError::internal(format!("删除失败：{e}")))?;
                    println!("已卸载：{id}");
                } else {
                    return Err(PluginError::malformed(format!("未安装插件：{id}")));
                }
                Ok(())
            }
            PluginAction::List => {
                let rows = plugin_cmd::plugin_list(&vw_std::plugins_dir());
                if rows.is_empty() {
                    println!("（插件目录为空）");
                }
                for (id, ver, dir) in rows {
                    println!("{:<24} {:<8} {}", id, ver, dir.display());
                }
                Ok(())
            }
            PluginAction::New { name } => {
                let dir = plugin_cmd::plugin_new(&name, &std::env::current_dir().unwrap_or_default())?;
                println!("已生成 {}（构建通过后打 tag v0.1.0 即发布）", dir.display());
                Ok(())
            }
        },
    }
}
