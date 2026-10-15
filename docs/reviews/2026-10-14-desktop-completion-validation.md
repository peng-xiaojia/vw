# 桌面开发收尾验证

日期：2026-10-14。基线提交 `f9e21c2`，沿用并保留此前未提交成果；没有提交、推送或更改系统默认程序。

## 本轮交付

- 去掉地址栏和打开路径按钮，顶部提供打开文件、打开文件夹及文档操作。
- 将文件、目录、搜索三个标签页改为一个侧栏：常驻文档内搜索，可折叠文件树、文档目录和搜索结果。
- 原生拖放监听 `tauri.webview.getCurrentWebview()`，与 Tauri 的 WebView 事件目标一致；关闭保护、拖放和启动路由分别初始化，单项失败不会阻断其余入口。
- 补齐未完成的 Office XLSX/PPTX 解析模块，适配 quick-xml 0.42，并接入标准注册表、工作区和安装关联。修复共享字符串输出放大、逆序单元格被截掉、PPTX 关系 ID 混淆与满幅表格预算边界。
- 更新能力矩阵与 README，修正早期“全部实施完成”不等于完整桌面交付的表述。

## 自动验证

| 检查 | 结果 / 证据 |
| --- | --- |
| `node --test app/ui-tests/*.test.mjs` | 32 项通过，包含 WebView 拖放进入正常草稿保护以及错误时关闭覆盖层 |
| Office 格式与预算回归 | 9 项通过；共享字符串放大和 500×128 边界均验证过失败再修复 |
| `cargo test --workspace --release --offline` | 最终 91 项通过，0 失败；输出保存在 `target/workspace-tests-final.log` |
| `node --check app/ui/main.js` | 通过 |
| 真实 Chromium DOM | 顶部按钮、合并导航、搜索、清空搜索、拖放、草稿取消、Ctrl+F、720px 无横向溢出；无页面脚本错误 |
| 独立代码审查 | 本轮 GUI 没有发现新的阻断问题；发现的 Office 输出预算与边界问题均已修复 |
| Windows 发布脚本 | 生成 EXE、便携 ZIP、NSIS 安装包、manifest 与 SHA-256 清单；校验 GUI PE 子系统与签名状态 |

原生窗口冒烟使用独立标识 `dev.vw.smoke.navigation`，避免影响用户运行中的旧版；正式产物仍使用 `dev.vw.app`。在真实 WebView2 中向 WebView 目标注入原生事件名并调用真实 IPC，验证了进入提示、中文路径文件打开、连续两次保存后的预览和磁盘一致、未保存草稿取消、DOCX 只读内容预览及 PDF 受控 PNG 实际显示。结果在 `target/native-navigation/result.json`，截图在同目录 `native.png` 和 `native-hex.png`。

这验证了 Tauri 事件到打开流程的链路；未将程序化事件注入描述为人工操作系统鼠标拖拽验收。完整 Windows 文件关联、安装、升级、卸载及 SmartScreen 声誉仍需干净桌面环境复核。

## 大文件首屏与内存

真实原生窗口、release 构建；每个夹具 67108864 字节，首屏为 40 块。通过真实前端 `openDocument` 计时，结束点为首屏返回，包含 IPC、打开和首屏 DOM 更新。

| 夹具 | 首轮首屏 | 复测首屏 | 复测宿主工作集 | 宿主峰值工作集 |
| --- | --- | --- | --- | --- |
| 64 MiB 文本 | 120 ms | 46 ms | 75 MiB | 77 MiB |
| 64 MiB hex | 350 ms | 217 ms | 75 MiB | 87 MiB |

同一机器的复测会复用文件系统缓存，不代表冷启动保证。内存来自 Windows `WorkingSet64` / `PeakWorkingSet64`，只测原生宿主，不包含 WebView2/GPU 子进程；峰值是该实例截至测量时的峰值，实例此前浏览过 DOCX/PDF。没有据此声称所有文件规模或全进程树内存均达标。

测试实例继承的系统临时目录受执行环境权限限制，首次 PDF 打开报 PermissionDenied。将已有 `VW_TMP_DIR` 覆盖设为工作区测试缓存后通过；未更改产品默认目录策略。系统限制下的测试失败与正常用户运行结果分别记录。

## 发行边界

当前本地产物为 `NotSigned`，没有可用于正式签名的证书。已提供签名、时间戳及最终签名验证入口；未把未签名构建描述为正式签名发行。

最终产物及摘要以 `dist/windows/manifest.json` 和 `SHA256SUMS.txt` 为准。Office 是内容预览，PDF 是 Windows 图像页预览，具体限制见 [桌面能力矩阵](../desktop-capabilities.md)。
