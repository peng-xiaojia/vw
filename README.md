# vw —— 插件化文件查看器

以图形界面为主交付物，供人查看各种格式的文件并对其中的文本类格式做编辑保存；同时提供 CLI 与 MCP 两个接口，供脚本和外部 AI agent 使用。

一句话架构：**内核零格式知识，一切格式能力都是插件；插件只产出一份统一的文档模型（Document IR），所有外壳消费同一份模型；编辑能力跟随插件，核心只负责保存管线。**

设计规格见 [docs/superpowers/specs/2026-10-13-file-viewer-design.md](docs/superpowers/specs/2026-10-13-file-viewer-design.md)（当前 v1.4），实际能力和验收边界见 [桌面能力矩阵](docs/desktop-capabilities.md)。

历史需求、Bug 修复、未完成事项及阶段验证见 [需求与 Bug 台账](docs/requirements-and-bug-history.md)。

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
plugins/           内置插件：text / html / markdown / json / csv / hex / dir / archive / image / pdf / office
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

GUI 操作：顶部原生「文件」菜单中打开文件/文件夹、保存、导出、关闭，或从资源管理器拖入文件/文件夹。左侧默认仅显示图标栏，点击展开导航，文件树使用文件夹和类型图标。快捷键：`Ctrl+O` 打开文件，`Ctrl+Shift+O` 打开文件夹，`Ctrl+F` 搜索，`Ctrl+S` 保存，`Ctrl+B` 收起/展开侧栏，`Ctrl+W` 关闭文档。切换文档与退出时会询问未保存修改的处理方式。

插件中心：顶部「插件 → 插件中心」或左侧拼块图标。提供真实内置/已安装列表、格式筛选、GitHub Releases 和本地 `.vwpkg` 安装、外部插件卸载。安装前展示权限并校验 SHA-256；本地包同目录需有 `.vwpkg.sha256`。安装或卸载后重启生效；无中央商店，不兼容 `.dbxp`。遵循 [DBX 参考边界](docs/reference/dbx-as-reference.md)，复用与来源见 [第三方台账](docs/THIRD-PARTY-NOTICES.md)。

Windows 数据目录默认使用 `%LOCALAPPDATA%\vw\webview-data`。便携包含 `vw-portable.marker`，优先使用 EXE 旁可写的 `webview-data/`，不可写时回退到用户目录；可用 `VW_WEBVIEW_DATA` 覆盖。启动失败会显示错误并记录 `%LOCALAPPDATA%\vw\logs\startup.log`。

Windows 发布：`pwsh -File tools/windows/build-release.ps1` 生成 `dist/windows/` 中的便携包、NSIS 安装包和 SHA-256 清单。提供代码签名证书时可加 `-CertificateThumbprint <证书指纹> -RequireSigning`；云签名可加 `-UseVSign -RequireSigning`，详见 [签名配置](docs/windows-signing.md)。未提供证书的本地构建为未签名产物。

## 一致性保证

`vw-conformance` 用同一套用例驱动三种传输路径（进程内直调、内存管道、真实子进程），
输出必须一致——这是「架构统一」从设计承诺变成结构保证的手段（规格 §12.1）。

## 许可与参考

本项目 Apache-2.0。参考实现规范（只读不 fork）见
[docs/reference/dbx-as-reference.md](docs/reference/dbx-as-reference.md)。
