//! P4 插件分发（设计规格 §8）：GitHub Releases 为源、零服务端。
//!
//! - `add github:owner/repo[@tag]`：解析 release → 按平台选 asset →
//!   下载 → sha256 校验 → 解包到插件目录（含 zip-slip 防护）；
//! - `remove`：删除插件目录；
//! - `list`：扫描插件目录；
//! - `new`：生成插件脚手架（含多平台 release CI 模板，让「打 tag 即发布」成立）。

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use vw_protocol::{ErrorKind, Manifest, PluginError};

pub const GITHUB_API: &str = "https://api.github.com";

/// 一次 release 解析结果。
pub struct Release {
    pub tag: String,
    pub assets: Vec<Asset>,
}

#[derive(Clone)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

/// §8：`VW_PLUGIN_MIRROR` 做前缀替换——镜像端点作为前缀，实际请求
/// `{mirror}/{原 URL}`（兼容 ghproxy 类前缀镜像）。空值/未设置走官方源。
pub fn apply_mirror(url: &str, mirror: Option<&str>) -> String {
    match mirror {
        Some(m) if !m.trim().is_empty() => {
            let m = m.trim().trim_end_matches('/');
            format!("{m}/{url}")
        }
        _ => url.to_string(),
    }
}

fn current_mirror() -> Option<String> {
    std::env::var("VW_PLUGIN_MIRROR").ok().filter(|m| !m.trim().is_empty())
}

fn http_get(url: &str) -> Result<Vec<u8>, PluginError> {
    let resp = ureq::get(&apply_mirror(url, current_mirror().as_deref()))
        .set("User-Agent", "vw-cli")
        .set("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(60))
        .call()
        .map_err(|e| PluginError::internal(format!("下载失败 {url}：{e}")))?;
    let mut buf = Vec::new();
    resp.into_reader()
        .read_to_end(&mut buf)
        .map_err(|e| PluginError::internal(format!("读取响应失败：{e}")))?;
    Ok(buf)
}

fn http_get_json(url: &str) -> Result<serde_json::Value, PluginError> {
    let bytes = http_get(url)?;
    serde_json::from_slice(&bytes).map_err(|e| PluginError::internal(format!("GitHub API 响应解析失败：{e}")))
}

/// 解析 `github:owner/repo[@tag]` 的 release 资产清单。
pub fn resolve_release(repo: &str, tag: Option<&str>) -> Result<Release, PluginError> {
    let repo = repo.strip_prefix("github:").unwrap_or(repo);
    let url = match tag {
        Some(t) => format!("{GITHUB_API}/repos/{repo}/releases/tags/{t}"),
        None => format!("{GITHUB_API}/repos/{repo}/releases/latest"),
    };
    let v = http_get_json(&url)?;
    let release_tag = v
        .get("tag_name")
        .and_then(|t| t.as_str())
        .ok_or_else(|| PluginError::malformed("release 响应缺少 tag_name"))?
        .to_string();
    let assets = v
        .get("assets")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| {
                    Some(Asset {
                        name: a.get("name")?.as_str()?.to_string(),
                        url: a.get("browser_download_url")?.as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Release { tag: release_tag, assets })
}

/// §8 命名约定：`{id}-{version}-{platform}.vwpkg`。
pub fn pick_asset<'a>(release: &'a Release, platform: &str) -> Option<&'a Asset> {
    release.assets.iter().find(|a| {
        a.name.ends_with(".vwpkg") && a.name.contains(&format!("-{platform}.vwpkg"))
    })
}

/// `sha256sum` 格式：`<hex>  <name>` 或裸 hex。
pub fn parse_sha256(text: &str) -> Option<String> {
    let t = text.trim();
    let hex = t.split_whitespace().next()?;
    if hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(hex.to_lowercase())
    } else {
        None
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(bytes);
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// zip-slip 防护：拒绝绝对路径与 `..` 穿越。
fn safe_entry_path(root: &Path, name: &str) -> Result<PathBuf, PluginError> {
    let p = Path::new(name);
    if p.is_absolute() || name.split(['/', '\\']).any(|seg| seg == "..") {
        return Err(PluginError::permission(format!("压缩包内路径越界：{name}")));
    }
    Ok(root.join(p))
}

/// 解包 `.vwpkg`（zip：manifest.json 在包根）到 `plugins_dir/{manifest.id}/`。
/// 返回插件 id。所有条目都经过 zip-slip 校验。
pub fn extract_vwpkg(bytes: &[u8], plugins_dir: &Path) -> Result<String, PluginError> {
    let mut za = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| PluginError::malformed(format!("无法读取 .vwpkg：{e}")))?;
    // 先读 manifest 校验合法性，再落盘
    let manifest_bytes = {
        let mut f = za
            .by_name("manifest.json")
            .map_err(|_| PluginError::malformed(".vwpkg 根缺少 manifest.json"))?;
        let mut b = Vec::new();
        f.read_to_end(&mut b).map_err(PluginError::from)?;
        b
    };
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| PluginError::malformed(format!("manifest 解析失败：{e}")))?;
    manifest.validate().map_err(PluginError::internal)?;
    let id = manifest.id.clone();

    let dest = plugins_dir.join(&id);
    std::fs::create_dir_all(&dest).map_err(PluginError::from)?;
    for i in 0..za.len() {
        let mut f = za
            .by_index(i)
            .map_err(|e| PluginError::malformed(format!("压缩包条目读取失败：{e}")))?;
        if f.is_dir() {
            continue;
        }
        let out_path = safe_entry_path(&dest, f.name())?;
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(PluginError::from)?;
        }
        let mut out = std::fs::File::create(&out_path).map_err(PluginError::from)?;
        std::io::copy(&mut f, &mut out).map_err(PluginError::from)?;
    }
    Ok(id)
}

/// `vw plugin add` 的完整流程（网络操作可注入测试的除外）。
pub fn plugin_add(spec: &str, plugins_dir: &Path, insecure: bool) -> Result<String, PluginError> {
    let (repo, tag) = match spec.split_once('@') {
        Some((r, t)) => (r, Some(t)),
        None => (spec, None),
    };
    let release = resolve_release(repo, tag)?;
    let platform = vw_plugin_host::platform_tag().to_string();
    let asset = pick_asset(&release, &platform)
        .ok_or_else(|| PluginError::malformed(format!(
            "release {} 中没有平台 {platform} 的 .vwpkg 资产（可用资产：{}）",
            release.tag,
            release.assets.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
        )))?;
    eprintln!("下载 {}（release {}）…", asset.name, release.tag);
    let bytes = http_get(&asset.url)?;
    if insecure {
        eprintln!("warn: --insecure 跳过 sha256 校验");
    } else {
        let sha_url = format!("{}.sha256", asset.url);
        let sha_text = String::from_utf8(http_get(&sha_url)?)
            .map_err(|_| PluginError::malformed("校验文件不是 UTF-8"))?;
        let expected = parse_sha256(&sha_text)
            .ok_or_else(|| PluginError::malformed("sha256 校验文件格式不合法"))?;
        let actual = sha256_hex(&bytes);
        if expected != actual {
            return Err(PluginError::new(
                ErrorKind::MalformedSource,
                format!("sha256 不匹配：期望 {expected}，实际 {actual}"),
            ));
        }
        eprintln!("sha256 校验通过");
    }
    let id = extract_vwpkg(&bytes, plugins_dir)?;
    eprintln!("已安装插件 {id} → {}", plugins_dir.display());
    Ok(id)
}

/// 扫描插件目录，返回已安装插件清单（id, version, dir）。
pub fn plugin_list(plugins_dir: &Path) -> Vec<(String, String, PathBuf)> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(plugins_dir) {
        for e in entries.flatten() {
            let mf = e.path().join("manifest.json");
            let Ok(raw) = std::fs::read_to_string(&mf) else { continue };
            let Ok(m) = serde_json::from_str::<Manifest>(&raw) else { continue };
            out.push((m.id, m.version, e.path()));
        }
    }
    out.sort();
    out
}

// ---------- 脚手架模板（§8：`vw plugin new` 必须内置 release CI） ----------

pub fn cargo_toml_tpl(bin: &str) -> String {
    format!(
        r#"[package]
name = "{bin}"
version = "0.1.0"
edition = "2021"
license = "Apache-2.0"
description = "vw 插件：<一句话描述>"

[[bin]]
name = "{bin}"
path = "src/main.rs"

[dependencies]
vw-protocol = "0.1"
vw-plugin-sdk = "0.1"
"#
    )
}

pub const LIB_RS_TPL: &str = r#"//! <插件名>：实现 RpcViewer / RpcDoc，参照 vw 仓库 plugins/example-sidecar。

use std::path::Path;
use std::sync::Arc;

use vw_plugin_sdk::{CancelFlag, RpcDoc, RpcViewer};
use vw_protocol::{
    Block, BlockList, Cursor, DocMeta, OpenOptions, Outline, PageRange, ProbeScore, PluginError, SearchHits, Source,
};

pub struct DemoViewer;

pub struct DemoDoc;

impl RpcViewer for DemoViewer {
    fn probe(&self, source: &Source) -> Option<ProbeScore> {
        // TODO: 按你的格式打分；扩展名命中给 60–75，魔数命中给 80–95
        let p = source.as_fs_path()?;
        p.extension().and_then(|e| e.to_str()).filter(|e| e.eq_ignore_ascii_case("example"))?;
        Some(ProbeScore { score: 65, reason: "扩展名匹配".into() })
    }

    fn open(
        &self,
        source: &Source,
        _opts: &OpenOptions,
        _tmp_dir: &Path,
        _cancel: CancelFlag,
    ) -> Result<Box<dyn RpcDoc>, PluginError> {
        let p = source.as_fs_path().ok_or_else(|| PluginError::malformed("只支持本地路径"))?;
        let bytes = std::fs::read(p)?;
        // TODO: 把你的格式解析为 Document IR（见 vw-protocol::Block）
        Ok(Box::new(DemoDoc))
    }
}

impl RpcDoc for DemoDoc {
    fn meta(&self) -> DocMeta {
        DocMeta { viewer: "demo".into(), ..Default::default() }
    }
    fn page(&self, _range: PageRange, _cancel: CancelFlag) -> Result<BlockList, PluginError> {
        Ok(BlockList { blocks: vec![Block::preformatted("TODO")], total: Some(1) })
    }
    fn search(&self, _q: &str, _from: Cursor, _cancel: CancelFlag) -> Result<SearchHits, PluginError> {
        Ok(SearchHits { hits: vec![], next: None })
    }
}

/// sidecar 入口：接管 stdin/stdout 直到 EOF。
pub fn run() {
    vw_plugin_sdk::enable_idle_exit_from_env();
    vw_plugin_sdk::serve(std::io::stdin(), std::io::stdout(), Arc::new(DemoViewer));
}
"#;

pub const MAIN_RS_TPL: &str = r#"fn main() {
    <bin>::run();
}
"#;

pub const RELEASE_YML_TPL: &str = r#"# 「打 tag 即发布」（设计规格 §8）：多平台构建 → 按平台 asset → sha256
name: release
on:
  push:
    tags: ["v*"]
permissions:
  contents: write
jobs:
  build:
    strategy:
      fail-fast: false
      matrix:
        include:
          - { os: windows-latest, platform: win32-x64, ext: .exe }
          - { os: ubuntu-22.04,   platform: linux-x64, ext: "" }
          - { os: macos-14,       platform: darwin-arm64, ext: "" }
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build --release
      - name: 打包 .vwpkg（manifest.json 在包根 + 该平台二进制）
        shell: bash
        run: |
          VERSION="${GITHUB_REF_NAME#v}"
          NAME="<bin>-${VERSION}-${{ matrix.platform }}.vwpkg"
          mkdir -p pkg/bin
          cp manifest.json pkg/
          cp "target/release/${{ matrix.bin }}${{ matrix.ext }}" "pkg/bin/${{ matrix.bin }}${{ matrix.ext }}"
          cd pkg && zip -r "../${NAME}" . && cd ..
          sha256sum "${NAME}" > "${NAME}.sha256"
          echo "ASSET=${NAME}" >> "$GITHUB_ENV"
      - uses: softprops/action-gh-release@v2
        with:
          files: |
            ${{ env.ASSET }}
            ${{ env.ASSET }}.sha256
"#;

pub const README_TPL: &str = r#"# <插件名>

vw sidecar 插件。开发：

```bash
cargo run -- bin/example.example   # 本地调试（stdin/stdout 讲 JSON Lines）
```

发布：打 tag `v0.1.0` 并 push，GitHub Actions 自动为三个平台构建
`<bin>-<version>-<platform>.vwpkg` 与 sha256 校验文件（设计规格 §8）。

安装（用户侧）：`vw plugin add github:你/本仓库`
"#;

/// `vw plugin new <name>`：生成可直接构建的插件工程。
pub fn plugin_new(name: &str, out_root: &Path) -> Result<PathBuf, PluginError> {
    let name = name.trim().trim_end_matches("-plugin").to_string();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        || name.starts_with('-')
    {
        return Err(PluginError::malformed(
            "插件名只允许小写字母、数字与连字符（如 my-format）",
        ));
    }
    let bin = format!("vw-{name}");
    let dir = out_root.join(&name);
    if dir.exists() {
        return Err(PluginError::malformed(format!("目录已存在：{}", dir.display())));
    }
    let src = dir.join("src");
    let wf = dir.join(".github/workflows");
    std::fs::create_dir_all(&src).map_err(PluginError::from)?;
    std::fs::create_dir_all(&wf).map_err(PluginError::from)?;

    std::fs::write(dir.join("Cargo.toml"), cargo_toml_tpl(&bin)).map_err(PluginError::from)?;
    std::fs::write(dir.join("src/lib.rs"), LIB_RS_TPL).map_err(PluginError::from)?;
    std::fs::write(dir.join("src/main.rs"), MAIN_RS_TPL.replace("<bin>", &bin)).map_err(PluginError::from)?;
    std::fs::write(
        dir.join(".github/workflows/release.yml"),
        RELEASE_YML_TPL.replace("<bin>", &bin).replace("${{ matrix.bin }}", &bin),
    )
    .map_err(PluginError::from)?;
    std::fs::write(dir.join("README.md"), README_TPL.replace("<bin>", &bin).replace("<插件名>", &name)).map_err(PluginError::from)?;
    std::fs::write(dir.join(".gitignore"), "/target\n").map_err(PluginError::from)?;

    // 示例 manifest（安装期由 .vwpkg 提供，这里给出模板便于本地目录安装调试）
    let manifest = serde_json::json!({
        "manifest_version": 1,
        "id": format!("dev.vw.{name}"),
        "name": name,
        "version": "0.1.0",
        "transport": { "kind": "stdio-jsonrpc", "framing": "jsonl",
            "executable": "bin/{platform}/vw-".to_string() + name.as_str() + "{ext}" },
        "contributes": { "viewers": [ { "id": name, "extensions": ["example"], "capabilities": ["view"] } ] },
        "permissions": ["fs.read", "tmp.write"],
        "priority": 40
    });
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).map_err(PluginError::from)?,
    )
    .map_err(PluginError::from)?;
    Ok(dir)
}
