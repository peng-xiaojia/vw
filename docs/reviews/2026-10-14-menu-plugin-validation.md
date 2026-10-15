# 原生菜单与插件中心交付验证

日期：2026-10-14。沿用现有工作区改动，不重置前轮成果。

## 已交付

- Tauri/muda 原生「文件、编辑、视图、插件、帮助」菜单，文件操作不再占据常驻工具栏，保留原有草稿保护与快捷键。
- 默认收起的导航面板、图标工具栏、文件夹与类型图标；文件、目录与搜索共用导航区。原生拖放继续走文档控制器。
- 插件中心提供安装、已安装与设置分区，显示真实注册表/本机 manifest，支持名称（含中文显示名）、ID、格式筛选。GitHub Releases 与本地 .vwpkg 安装前校验 SHA-256 并展示权限；外部插件卸载后重启生效。
- GUI/CLI 共用下载、校验、暂存安装与卸载边界；修正平台占位符相对于安装目录解析。
- 本机证书与 VSign 云签名配置；仅生成便携版也执行独立签名。缺少凭据在构建前报错，签名产物必须通过 Authenticode 和时间戳检查。
- 按 DBX 参考实现文件更新固定锚点与移植台账；Lucide 和 DBX 许可证随发布包附带。便携 ZIP 仅包含发布文件，不打包既有 WebView 用户数据。

## 验证证据

| 验证 | 结果 |
| --- | --- |
| cargo test --workspace --release --offline | 95 项通过，0 失败；target/menu-plugins-workspace-final.log |
| node --test app/ui-tests/*.test.mjs | 34 项通过，0 失败；target/frontend-menu-tests-final.log |
| 实际 Chromium DOM | 菜单事件、图标侧栏、安装确认/取消、校验失败、文本安全、中文筛选、卸载、设置、草稿与拖放保护、720px 布局通过；target/ui-menu-plugins-smoke.cjs |
| 独立 WebView2 原生窗口 | 从 Windows 实际菜单读取文字与启用状态；向仅匹配测试 EXE 的窗口发送菜单命令，验证真实菜单回调 → 前端 → IPC；保存、退出草稿取消、关闭放弃、真实 sidecar 加载、卸载与过期 token 拒绝通过；target/native-menu/result.json |
| 插件边界回归 | ZIP 路径/平台二进制、离线校验失败、失败不留半成品、拒绝覆盖已有安装、卸载路径与不一致解压大小；11 项插件管理测试通过 |
| PowerShell 签名脚本 | 两个脚本语法解析通过，缺少 VSign 配置预检通过；无证书/账号，未实际调用正式签名 |
| Windows 发布 | build-release.ps1 成功，EXE/便携 ZIP/NSIS 生成，PE GUI 子系统与 SHA-256 核对通过；target/windows-menu-release.log |

独立审查发现并修复退出菜单 ACL、实际解压大小限制、CLI 卸载边界、中文名筛选、便携签名分支和参考锚点文字冲突。此前 Office 与拖放成果保留。

## 边界

当前 EXE 与安装包为 NotSigned。用户还未申请自己的代码签名证书/账号，本轮按用户要求先完成配置。本机 DSH/DBX 的有效签名检查和签名准备见 [Windows 签名](../windows-signing.md)。正式签名不能保证立即获得 SmartScreen 信誉。

GUI 本地安装 UI 使用 mock 检查确认流程，包校验与写盘用真实离线 ZIP 回归；GitHub 在线分发未以第三方真实插件 Release 做端到端安装验收。插件权限为声明式，非 OS 沙箱。当前无中央商店、不兼容 .dbxp，不引入数据库领域代码。

未执行干净机器安装/升级/卸载、真实 CA/云服务签名或人工鼠标拖放验收。原生菜单由 Windows 命令消息验证，拖放由 WebView 事件链路验证。当前文档可继续使用已加载插件，安装/卸载需要重启；Windows 可能将正在使用的插件二进制锁住，卸载若已撤出发现目录但清理失败，会保留隐藏残余目录。

## 产物

`dist/windows/portable/vw-app.exe`、`dist/windows/vw-windows-portable.zip`、`dist/windows/vw_0.1.0_x64-setup.exe`。源码修订、锁文件哈希、字节数、完整性哈希与签名状态见 `dist/windows/manifest.json`。
