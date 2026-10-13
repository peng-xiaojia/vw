//! MCP 端到端：spawn 真实 vw-mcp 子进程，走 initialize → tools/list → tools/call。

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};

struct Server {
    child: Child,
}

impl Server {
    fn start(allow_save: bool) -> Self {
        let mut c = Command::new(env!("CARGO_BIN_EXE_vw-mcp"));
        c.env("VW_TMP_DIR", std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("mcp-tmp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if allow_save {
            c.env("VW_MCP_ALLOW_SAVE", "1");
        }
        Self { child: c.spawn().unwrap() }
    }

    fn send(&mut self, v: serde_json::Value) -> serde_json::Value {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{v}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        BufReader::new(self.child.stdout.as_mut().unwrap()).read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn write_sample(name: &str, content: &[u8]) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("mcp");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, content).unwrap();
    p
}

#[test]
fn mcp_initialize_tools_and_view_end_to_end() {
    let mut s = Server::start(false);
    let init = s.send(serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": { "name": "test" } }
    }));
    assert_eq!(init["result"]["serverInfo"]["name"], "vw-mcp");

    // 通知不回包：直接写入即可（不读响应）
    let stdin = s.child.stdin.as_mut().unwrap();
    writeln!(stdin, r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#).unwrap();
    stdin.flush().unwrap();

    let tools = s.send(serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }));
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array().unwrap().iter()
        .map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"vw_view") && names.contains(&"vw_search"));
    assert!(!names.contains(&"vw_save"), "未开启时不得暴露保存工具");

    let f = write_sample("a.txt", b"alpha\nbeta needle\ngamma\n");
    let view = s.send(serde_json::json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": { "name": "vw_view", "arguments": { "path": f.to_string_lossy() } }
    }));
    let text = view["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("beta needle"), "{text}");

    let search = s.send(serde_json::json!({
        "jsonrpc": "2.0", "id": 4, "method": "tools/call",
        "params": { "name": "vw_search", "arguments": { "path": f.to_string_lossy(), "query": "needle" } }
    }));
    let r = search["result"]["content"][0]["text"].as_str().unwrap();
    assert!(r.contains("\"block\":0"), "{r}");
}

#[test]
fn mcp_save_tool_gated_by_env() {
    // 关闭时：调用保存 → isError 文本
    let mut s = Server::start(false);
    let resp = s.send(serde_json::json!({
        "jsonrpc": "2.0", "id": 9, "method": "tools/call",
        "params": { "name": "vw_save", "arguments": { "path": "x", "content": "y" } }
    }));
    assert_eq!(resp["result"]["isError"], true);

    // 开启时：保存成功
    let mut s2 = Server::start(true);
    let f = write_sample("b.txt", b"old\n");
    let resp = s2.send(serde_json::json!({
        "jsonrpc": "2.0", "id": 10, "method": "tools/call",
        "params": { "name": "vw_save", "arguments": { "path": f.to_string_lossy(), "content": "new content\n" } }
    }));
    assert_eq!(resp["result"]["isError"], false, "{}", resp);
    assert_eq!(std::fs::read(&f).unwrap(), b"new content\n");
}
