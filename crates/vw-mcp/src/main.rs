//! vw MCP 外壳（设计规格 §4.1 / §13 P3）：在 stdio 上讲 Model Context Protocol。
//!
//! 复用与 CLI/GUI 完全相同的 Session API——「无特权路径」在 MCP 上同样成立。
//! 工具集（只读为主，保存需 `VW_MCP_ALLOW_SAVE=1` 显式开启）：
//! - `vw_view`    查看文件/目录（文本渲染或 IR JSON）
//! - `vw_search`  全文搜索
//! - `vw_save`    保存（默认关闭）
//! - `vw_plugins` 列出插件

use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::sync::Arc;

use vw_core::{OpenedDoc, Registry, Session};
use vw_protocol::{ErrorKind, OpenOptions, PluginError, Source};

const PROTOCOL_VERSION: &str = "2024-11-05";

fn err(id: Option<Value>, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id.unwrap_or(Value::Null), "error": { "code": code, "message": message } })
}

fn result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn session() -> Result<Arc<Session>, PluginError> {
    let (reg, _hosts) = vw_std::default_registry()?;
    Ok(Arc::new(Session::new(Arc::new(reg))))
}

/// 打开并整体取块（MCP 工具的统一入口；有状态句柄对无状态工具调用意义有限）。
fn view_once(path: &str, as_viewer: Option<&str>) -> Result<OpenedDoc, PluginError> {
    let s = session()?;
    let opts = OpenOptions { as_viewer: as_viewer.map(String::from), ..Default::default() };
    let src = if std::path::Path::new(path).is_dir() {
        Source::Dir { path: path.into() }
    } else {
        Source::path(path)
    };
    s.open(src, opts)
}

fn tool_view(args: &Value) -> Result<Value, PluginError> {
    let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| PluginError::internal("缺少 path"))?;
    let as_viewer = args.get("as_viewer").and_then(|v| v.as_str());
    let want_json = args.get("json").and_then(|v| v.as_bool()).unwrap_or(false);
    let doc = view_once(path, as_viewer)?;
    let all = doc.page_all(100_000)?;
    let text = if want_json {
        vw_core::render::json::render_json(&doc.meta, &all.blocks, all.total)
    } else {
        vw_core::render::text::render_document(&doc.meta, &all.blocks, None)
    };
    doc.close();
    Ok(json!({ "text": text }))
}

fn tool_search(args: &Value) -> Result<Value, PluginError> {
    let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| PluginError::internal("缺少 path"))?;
    let query = args.get("query").and_then(|v| v.as_str()).ok_or_else(|| PluginError::internal("缺少 query"))?;
    let doc = view_once(path, None)?;
    let mut hits = Vec::new();
    let mut cursor = vw_protocol::Cursor::start();
    loop {
        let page = doc.search(query, cursor)?;
        hits.extend(page.hits.iter().map(|h| json!({ "block": h.block, "start": h.start, "excerpt": h.excerpt })));
        match page.next {
            Some(c) => cursor = c,
            None => break,
        }
        if hits.len() >= 500 {
            break;
        }
    }
    doc.close();
    Ok(json!({ "query": query, "hits": hits }))
}

fn tool_save(args: &Value) -> Result<Value, PluginError> {
    let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| PluginError::internal("缺少 path"))?;
    let content = args.get("content").and_then(|v| v.as_str()).ok_or_else(|| PluginError::internal("缺少 content"))?;
    let s = session()?;
    let doc = s.open(Source::path(path), OpenOptions::default())?;
    let report = doc.save_text(content.to_string())?;
    doc.close();
    Ok(json!({ "saved": true, "bytes": report.bytes,
        "backup": report.backup.as_ref().map(|b| b.display().to_string()) }))
}

fn tool_plugins() -> Value {
    let (reg, hosts) = vw_std::default_registry().unwrap_or_else(|_| (Registry::new(), Vec::new()));
    let _ = hosts;
    let items: Vec<Value> = reg
        .viewers()
        .iter()
        .map(|v| {
            let m = v.manifest();
            json!({ "id": m.id, "version": m.version, "transport": format!("{:?}", m.transport.kind) })
        })
        .collect();
    json!({ "plugins": items })
}

fn tool_defs(allow_save: bool) -> Value {
    let mut tools = vec![
        json!({
            "name": "vw_view",
            "description": "查看本地文件/目录（支持 txt/markdown/json/csv/zip/目录，未知类型 hex 兜底）。返回结构化文本渲染；json=true 时返回 Document IR。",
            "inputSchema": { "type": "object", "properties": {
                "path": { "type": "string", "description": "本地绝对或相对路径" },
                "as_viewer": { "type": "string", "description": "可选，指定 viewer id" },
                "json": { "type": "boolean", "description": "输出 IR JSON 而非文本渲染" } },
                "required": ["path"] }
        }),
        json!({
            "name": "vw_search",
            "description": "在文件内做不区分大小写的子串搜索，返回块号、偏移与摘录。",
            "inputSchema": { "type": "object", "properties": {
                "path": { "type": "string" }, "query": { "type": "string" } },
                "required": ["path", "query"] }
        }),
        json!({
            "name": "vw_plugins",
            "description": "列出已注册的 vw 插件。",
            "inputSchema": { "type": "object", "properties": {} }
        }),
    ];
    if allow_save {
        tools.push(json!({
            "name": "vw_save",
            "description": "以文本方式保存文件（走冲突检测 + 原子写管线；仅在显式开启时可用）。",
            "inputSchema": { "type": "object", "properties": {
                "path": { "type": "string" }, "content": { "type": "string" } },
                "required": ["path", "content"] }
        }));
    }
    json!({ "tools": tools })
}

fn call_tool(name: &str, args: &Value, allow_save: bool) -> Result<Value, PluginError> {
    match name {
        "vw_view" => tool_view(args),
        "vw_search" => tool_search(args),
        "vw_plugins" => Ok(tool_plugins()),
        "vw_save" if allow_save => tool_save(args),
        other => Err(PluginError::unsupported(format!("未知工具：{other}"))),
    }
}

fn handle_msg(line: &str, allow_save: bool) -> Option<Value> {
    let v: Value = serde_json::from_str(line).ok()?;
    let method = v.get("method").and_then(|m| m.as_str())?.to_string();
    // 通知（无 id）不回包
    let id = v.get("id").cloned()?;

    match method.as_str() {
        "initialize" => Some(result(
            id,
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "vw-mcp", "version": "0.1.0" }
            }),
        )),
        "ping" => Some(result(id, json!({}))),
        "tools/list" => Some(result(id, tool_defs(allow_save))),
        "tools/call" => {
            let name = v.pointer("/params/name").and_then(|n| n.as_str()).unwrap_or("").to_string();
            let args = v.pointer("/params/arguments").cloned().unwrap_or(json!({}));
            match call_tool(&name, &args, allow_save) {
                Ok(r) => Some(result(
                    id,
                    json!({ "content": [ { "type": "text", "text": r.to_string() } ], "isError": false }),
                )),
                Err(e) if e.kind == ErrorKind::Unsupported => {
                    Some(result(id, json!({ "content": [ { "type": "text", "text": e.message } ], "isError": true })))
                }
                Err(e) => Some(err(Some(id), -32000, &format!("[{:?}] {}", e.kind, e.message))),
            }
        }
        "notifications/initialized" | "notifications/cancelled" => None,
        other => Some(err(Some(id), -32601, &format!("method not found: {other}"))),
    }
}

fn main() {
    let allow_save = std::env::var("VW_MCP_ALLOW_SAVE").map(|v| v == "1").unwrap_or(false);
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(resp) = handle_msg(&line, allow_save) {
            let Ok(mut s) = serde_json::to_string(&resp) else { continue };
            s.push('\n');
            if out.write_all(s.as_bytes()).and_then(|_| out.flush()).is_err() {
                break;
            }
        }
    }
}
