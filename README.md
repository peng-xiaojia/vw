# vw —— 插件化文件查看器

以图形界面为主交付物，供人查看各种格式的文件并对其中的文本类格式做编辑保存；同时提供 CLI 与 MCP 两个接口，供脚本和外部 AI agent 使用。

一句话架构：**内核零格式知识，一切格式能力都是插件；插件只产出一份统一的文档模型（Document IR），所有外壳消费同一份模型；编辑能力跟随插件，核心只负责保存管线。**

设计规格见 [docs/superpowers/specs/2026-10-13-file-viewer-design.md](docs/superpowers/specs/2026-10-13-file-viewer-design.md)（当前 v1.3）。

## 布局

```
crates/
  vw-protocol      IR 与 RPC 消息的共享定义（core 与插件 SDK 共用）
  vw-core          内核：registry / doc / session / policy / render（零格式知识）
  vw-plugin-host   外置插件宿主：sidecar 进程管理、stdio JSON-RPC
  vw-plugin-sdk    插件作者使用的 Rust SDK（sidecar 服务端 + 内存管道）
  vw-conformance   一致性套件：同一用例驱动 in-process / 内存管道 / 真实子进程
  vw-std           标准插件集装配（所有外壳共用）
  vw-cli           CLI 外壳（vw）
  vw-mcp           MCP 外壳（stdio 上的 Model Context Protocol 服务器）
plugins/           内置插件：text / markdown / json / csv / hex / dir / archive
                   example-sidecar（最小 sidecar 示例，conformance 被测对象）
plugins-heavy/     重格式参考插件：vw-parquet-sidecar（独立发布模板）
app/               GUI 外壳（Tauri 2 + 零构建链原生 JS 前端）
tools/wry-probe    诊断工具：最小 WebView2 创建探针（隔离 tauri/环境因素）
docs/              设计规格与参考实现规范
```

## 构建

```bash
cargo build --release          # 全部（含 Tauri GUI，首次编译较久）
cargo test  --workspace        # 全部测试
```

Windows 下若涉及 C 依赖编译（如 SQLite 类插件开发），需要 MSVC + Windows SDK 的
`INCLUDE`/`LIB`；`.cargo/config.toml`（不入库）可按机器配置。

## 快速上手

```bash
# CLI 查看（自动探测插件；未知类型 hex 兜底）
vw view README.md
vw view data.csv --json          # 输出 Document IR JSON
vw view app.log                  # sidecar 插件示例（经目录安装后）
vw view archive.zip --inner dir/file.txt   # 打开归档内成员

# 编辑保存（stdin 或 --from；走冲突检测 + 原子写 + .bak 备份）
vw view a.txt --raw | vw save a.txt

# 插件管理（GitHub Releases 为源，零服务端）
vw plugin add github:owner/repo@v1.2.3
vw plugin list
vw plugin new my-format          # 脚手架，内置多平台 release CI 模板

# MCP（供外部 AI agent）
vw-mcp                           # stdio；保存需 VW_MCP_ALLOW_SAVE=1

# GUI
vw-app                           # Tauri 2 窗口
```

GUI 首启说明：
- Windows 首次运行未签名的新 exe 会被 SmartScreen 拦截，点「更多信息 → 仍要运行」即可；
- WebView2 数据目录默认在 **exe 旁 `webview-data/`**（便携形态），可用 `VW_WEBVIEW_DATA` 覆盖。
  刻意不用 `%LOCALAPPDATA%\<identifier>`：受限令牌/安全策略环境下该位置不可写，
  会导致 WebView2 报 E_UNEXPECTED / 拒绝访问（本项目开发环境实测复现，详见 app/src/lib.rs 注释）。

## 一致性保证

`vw-conformance` 用同一套用例驱动三种传输路径（进程内直调、内存管道、真实子进程），
输出必须一致——这是「架构统一」从设计承诺变成结构保证的手段（规格 §12.1）。

## 许可与参考

本项目 Apache-2.0。参考实现规范（只读不 fork）见
[docs/reference/dbx-as-reference.md](docs/reference/dbx-as-reference.md)。
