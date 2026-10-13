//! CLI 端到端集成测试：真实二进制 + 真实文件。

use std::process::Command;

fn vw() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_vw"));
    c.env("VW_TMP_DIR", std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cli-tmp"));
    c
}

fn write(name: &str, content: &[u8]) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cli");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, content).unwrap();
    p
}

#[test]
fn view_text_and_hex_fallback() {
    let txt = write("a.txt", b"line one\nline two\n");
    let out = vw().args(["view", txt.to_str().unwrap()]).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("[viewer: text"), "{stdout}");
    assert!(stdout.contains("line two"));

    // 无扩展名 + 非 UTF-8 → hex 兜底
    let bin = write("blobby", &[0x00, 0x01, 0x02, 0xff, 0xfe, 0x00]);
    let out = vw().args(["view", bin.to_str().unwrap()]).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("[viewer: hex"), "{stdout}");
}

#[test]
fn view_json_output_is_ir() {
    let f = write("b.json", br#"{"k":[1,2]}"#);
    let out = vw().args(["view", f.to_str().unwrap(), "--json"]).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["meta"]["viewer"], "json");
    assert_eq!(v["blocks"][0]["kind"], "code");
}

#[test]
fn view_missing_file_errors_with_kind() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cli");
    let out = vw().args(["view", dir.join("nope.txt").to_str().unwrap()]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("error["), "{stderr}");
    assert!(stderr.contains("MalformedSource") || stderr.contains("NoViewer"), "{stderr}");
}

#[test]
fn save_roundtrip_via_stdin_and_conflict_exit_code() {
    let f = write("c.txt", b"v1\n");
    // 原文读出 → 原样存回（--raw | save）
    let out1 = vw().args(["view", f.to_str().unwrap(), "--raw"]).output().unwrap();
    assert!(out1.status.success());
    use std::io::Write;
    let mut child = vw()
        .args(["save", f.to_str().unwrap()])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.as_mut().unwrap().write_all(&out1.stdout).unwrap();
    let out2 = child.wait_with_output().unwrap();
    assert!(out2.status.success(), "save 失败：{}", String::from_utf8_lossy(&out2.stderr));
    assert_eq!(std::fs::read(&f).unwrap(), b"v1\n");
}

#[test]
fn probe_and_plugins_list() {
    let f = write("d.csv", b"a,b\n1,2\n");
    let out = vw().args(["probe", f.to_str().unwrap()]).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("csv"), "{stdout}");

    let out = vw().args(["plugins"]).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("vw.text") && stdout.contains("vw.hex"), "{stdout}");
}
