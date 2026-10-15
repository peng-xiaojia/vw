//! P4 插件分发的离线单元测试（网络路径不做真实调用）。

use std::io::Write;

use vw_cli::plugin_cmd::{extract_vwpkg, parse_sha256, pick_asset, plugin_new, sha256_hex, Asset, Release};

fn build_vwpkg(name: &str, entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut zw = zip::ZipWriter::new(&mut buf);
        for (n, c) in entries {
            zw.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
            zw.write_all(c).unwrap();
        }
        zw.finish().unwrap();
    }
    let _ = name;
    buf.into_inner()
}

use std::io::Cursor;

const MANIFEST: &str = r#"{
  "manifest_version": 1,
  "id": "dev.vw.demo",
  "name": "demo", "version": "0.1.0",
  "transport": { "kind": "stdio-jsonrpc", "framing": "jsonl", "executable": "bin/{platform}/vw-demo{ext}" },
  "contributes": { "viewers": [ { "id": "demo", "extensions": ["example"], "capabilities": ["view"] } ] },
  "permissions": ["fs.read", "tmp.write"]
}"#;

#[test]
fn extract_vwpkg_installs_into_id_dir() {
    let bytes = build_vwpkg("demo", &[
        ("manifest.json", MANIFEST.as_bytes()),
        ("bin/win32-x64/vw-demo.exe", b"MZ fake bin"),
    ]);
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("p4-install");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let id = extract_vwpkg(&bytes, &dir).unwrap();
    assert_eq!(id, "dev.vw.demo");
    assert!(dir.join("dev.vw.demo/manifest.json").is_file());
    assert!(dir.join("dev.vw.demo/bin/win32-x64/vw-demo.exe").is_file());
}

#[test]
fn vwpkg_without_manifest_rejected() {
    let bytes = build_vwpkg("bad", &[("only.txt", b"x")]);
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("p4-bad");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let err = match extract_vwpkg(&bytes, &dir) {
        Err(e) => e,
        Ok(_) => panic!("缺 manifest 的包应被拒绝"),
    };
    assert_eq!(err.kind, vw_protocol::ErrorKind::MalformedSource);
}

#[test]
fn zip_slip_entries_rejected() {
    let bytes = build_vwpkg("slip", &[
        ("manifest.json", MANIFEST.as_bytes()),
        ("../evil.txt", b"pwned"),
    ]);
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("p4-slip");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let err = match extract_vwpkg(&bytes, &dir) {
        Err(e) => e,
        Ok(_) => panic!("zip-slip 条目必须被拒绝"),
    };
    assert_eq!(err.kind, vw_protocol::ErrorKind::PermissionDenied);
    assert!(!dir.join("evil.txt").exists());
    assert!(!dir.join("dev.vw.demo").exists(), "失败不能留下部分安装");
}

#[test]
fn package_id_and_windows_aliases_cannot_escape_or_overwrite() {
    for (index, name) in ["C:escape", "file:stream", "dir/CON", "bin/../bad", "file.", "dir\\file"].iter().enumerate() {
        let bytes = build_vwpkg("bad", &[("manifest.json", MANIFEST.as_bytes()), (name, b"bad")]);
        let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("p4-path-{index}"));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(extract_vwpkg(&bytes, &dir).is_err(), "{name}");
        assert!(!dir.join("dev.vw.demo").exists());
    }
    let manifest = MANIFEST.replace("dev.vw.demo", "../escape");
    let bytes = build_vwpkg("bad", &[("manifest.json", manifest.as_bytes())]);
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("p4-id-escape");
    assert!(extract_vwpkg(&bytes, &dir).is_err());
}

#[test]
fn offline_install_requires_matching_sha256_and_keeps_existing_install() {
    use vw_cli::plugin_cmd::prepare_local;
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("p4-offline");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bytes = build_vwpkg("demo", &[("manifest.json", MANIFEST.as_bytes())]);
    let file = dir.join("demo.vwpkg");
    std::fs::write(&file, &bytes).unwrap();
    assert!(prepare_local(&file).is_err());
    std::fs::write(dir.join("demo.vwpkg.sha256"), sha256_hex(b"wrong")).unwrap();
    assert!(prepare_local(&file).is_err());
    std::fs::write(dir.join("demo.vwpkg.sha256"), sha256_hex(&bytes)).unwrap();
    assert_eq!(prepare_local(&file).unwrap().manifest.id, "dev.vw.demo");
    extract_vwpkg(&bytes, &dir.join("plugins")).unwrap();
    assert!(extract_vwpkg(&bytes, &dir.join("plugins")).is_err());
    assert_eq!(std::fs::read_to_string(dir.join("plugins/dev.vw.demo/manifest.json")).unwrap(), MANIFEST);
}

#[test]
fn remove_rejects_parent_paths_and_uninstalls_only_matching_manifest() {
    use vw_cli::plugin_cmd::plugin_remove;
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("p4-remove-boundary");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("keep.txt"), "keep").unwrap();
    let bytes = build_vwpkg("demo", &[("manifest.json", MANIFEST.as_bytes())]);
    extract_vwpkg(&bytes, &dir.join("plugins")).unwrap();
    for id in ["..", ".", "../other", "C:/other"] { assert!(plugin_remove(id, &dir.join("plugins")).is_err()); }
    assert_eq!(std::fs::read_to_string(dir.join("keep.txt")).unwrap(), "keep");
    plugin_remove("dev.vw.demo", &dir.join("plugins")).unwrap();
    assert!(!dir.join("plugins/dev.vw.demo").exists());
}

#[test]
fn inconsistent_zip_sizes_are_rejected_without_partial_install() {
    let mut bytes = build_vwpkg("demo", &[("manifest.json", MANIFEST.as_bytes()), ("payload.txt", b"small fixture payload")]);
    // Fixture central directory advertises a shorter size than its actual entry.
    let positions: Vec<_> = bytes.windows(4).enumerate().filter_map(|(i, b)| (b == b"PK\x01\x02").then_some(i)).collect();
    let last = *positions.last().unwrap();
    bytes[last + 24..last + 28].copy_from_slice(&1u32.to_le_bytes());
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("p4-size-mismatch");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(extract_vwpkg(&bytes, &dir).is_err());
    assert!(!dir.join("dev.vw.demo").exists());
}

#[test]
fn asset_pick_matches_platform_convention() {
    let release = Release {
        tag: "v0.1.0".into(),
        assets: vec![
            Asset { name: "vw-demo-0.1.0-win32-x64.vwpkg".into(), url: "u1".into() },
            Asset { name: "vw-demo-0.1.0-linux-x64.vwpkg".into(), url: "u2".into() },
            Asset { name: "vw-demo-0.1.0-win32-x64.vwpkg.sha256".into(), url: "u3".into() },
        ],
    };
    let pick = pick_asset(&release, "win32-x64").unwrap();
    assert_eq!(pick.url, "u1");
    assert!(pick_asset(&release, "darwin-arm64").is_none());
}

#[test]
fn sha256_parsing_and_hex() {
    let hex = sha256_hex(b"abc");
    assert_eq!(hex, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert_eq!(parse_sha256(&format!("{hex}  file.vwpkg")).as_deref(), Some(hex.as_str()));
    assert_eq!(parse_sha256(&hex.to_uppercase()).as_deref(), Some(hex.as_str()));
    assert!(parse_sha256("not-a-hash").is_none());
}

#[test]
fn plugin_new_scaffolds_complete_project() {
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("p4-new");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let dir = plugin_new("my-format", &root).unwrap();
    assert!(dir.join("Cargo.toml").is_file());
    assert!(dir.join("src/lib.rs").is_file());
    assert!(dir.join("src/main.rs").is_file());
    assert!(dir.join("manifest.json").is_file());
    assert!(dir.join(".github/workflows/release.yml").is_file());
    let cargo = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
    assert!(cargo.contains("name = \"vw-my-format\""));
    let wf = std::fs::read_to_string(dir.join(".github/workflows/release.yml")).unwrap();
    // §8 命名约定与 sha256 产物
    assert!(wf.contains("-${{ matrix.platform }}.vwpkg") || wf.contains(".vwpkg"));
    assert!(wf.contains("sha256sum"));
    // manifest 合法
    let m: vw_protocol::Manifest =
        serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap()).unwrap();
    m.validate().unwrap();
    // 非法名字被拒
    assert!(plugin_new("Bad_Name!", &root).is_err());
}

#[test]
fn mirror_prefix_replacement() {
    use vw_cli::plugin_cmd::apply_mirror;
    assert_eq!(apply_mirror("https://api.github.com/x", None), "https://api.github.com/x");
    assert_eq!(apply_mirror("https://api.github.com/x", Some("")), "https://api.github.com/x");
    assert_eq!(
        apply_mirror("https://api.github.com/repos/a/b", Some("https://mirror.example.com")),
        "https://mirror.example.com/https://api.github.com/repos/a/b"
    );
    assert_eq!(
        apply_mirror("https://github.com/a/b/releases/download/v1/f.vwpkg", Some("https://m.example.com/")),
        "https://m.example.com/https://github.com/a/b/releases/download/v1/f.vwpkg"
    );
}
