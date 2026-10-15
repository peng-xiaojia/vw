# HTML 原页与多文件标签验收

日期：2026-10-15。在现有查看器上继续开发，未回退此前菜单、插件、格式关联或格式预览功能。

## 实现

- 每标签复用一个 DocumentController，保留草稿、源码/展示、搜索、滚动位置及章节折叠。文件选择可多选，重复路径激活已有标签；支持标签关闭按钮、中键关闭与 Ctrl+Tab / Ctrl+Shift+Tab 切换。
- 关闭窗口时先检查全部标签，取消退出保留所有标签；等待保存期间冻结编辑和切换，防止已检查标签再次产生未保存修改。
- HTML 使用 `vw-preview` 受控目录挂载与独立 sandbox iframe，保留原文 CSS、字体、图片及本地脚本。frame 隐藏时保留布局视口、滚动和子页面位置。
- 网页资源经过 URL 解码、扩展名与 canonical 路径验证，禁止目录越界及未知资源类型，单个原始资源最多 64 MiB。关闭文档撤销挂载，预览版本防止旧请求覆盖新草稿。
- 原始 HTML 保留浏览器编码识别，UTF-16 BOM 文件转码后注入桥脚本，编辑草稿使用 UTF-8。桥只提供缩放、标签切换及外链请求提示；网页不能直接调用宿主打开 URL，需用户在应用内点击具体 URL 的打开按钮。

## 验证

| 验证 | 结果 |
| --- | --- |
| `node --test app/ui-tests/*.test.mjs` | 46 项通过，包括 7 项标签状态/并发/关闭保护回归 |
| `node app/ui-tests/feature-smoke.cjs` | 实际 Chromium：菜单、日夜模式、侧栏调整、插件确认、JSON/Markdown 折叠、HTML CSS/脚本、父 DOM 隔离、伪造外链与文件操作拒绝、多选、标签切换及关闭取消/放弃通过 |
| `cargo test --workspace --offline` | 100 项通过，0 失败；2 项需要注册表或打开系统设置的交互测试默认忽略；最终日志 `target/html-tabs-workspace-final.log` |
| `cargo test -p vw-app --test html_preview --offline` | 2 项资源安全、关闭撤销、预览逆序及编码回归通过 |
| `node app/ui-tests/native-html-tabs.cjs` | 真实 WebView2 + IPC：CSS、SVG、脚本、父 DOM 隔离、IPC 阻断、草稿取消、后台草稿退出保护、滚动与子页面状态保留通过 |
| 原型 HTML 与浏览器对比 | 在相同可用内容宽度下，网格均为 3 列，背景 `rgb(238,241,245)`、标题 `26px` / `rgb(0,36,72)`、字体及 Logo 原图宽度一致；卡片宽度 `321.0625px` 对 `321px` |
| Windows 构建 | 安装版、便携版、清单、SHA-256 生成并核对；当前产物 NotSigned |

原型只读使用 `D:/desktop/临时文件/设计/design/index.html`；验证目录子页可导航且可生成页面链接。截图与比较 JSON 位于 `output/playwright/html-tabs/`。原生测试使用独立构建标识 `dev.vw.test.htmltabs`、独立 WebView 数据目录和临时文件，不替换用户程序。

独立代码审查及复核通过，修复退出等待保存时的草稿竞态、HTML 预览逆序覆盖、旧编码声明覆盖及网页伪造外链命令。首次原生测试触发了旧版单实例转发，此后使用独立标识；后续测试未关闭或修改用户现有应用。

复现隔离原生测试：PowerShell 中设置 `$env:TAURI_CONFIG='{"identifier":"dev.vw.test.htmltabs"}'`，执行 `cargo build -p vw-app --offline` 后运行该 Node 脚本；生产打包不使用此标识。脚本只对自建测试文本写入草稿，用户原型文件保持只读。

## 范围

HTML 为本地网页预览，外部 CDN、站点根路径、联网 API、登录存储及表单提交不保证兼容；不会承诺任意网站等同完整浏览器。无同源权限的沙箱中，WebView2 可能注入 Tauri 全局对象，但实际 IPC 不可用，且 CSP 阻断 IPC 网络请求。HTML 页内焦点下桥只支持 Ctrl+Tab 与 Ctrl+滚轮；保存、打开、关闭可使用应用菜单或标签栏。

Windows 成品位于 `dist/windows-20261015-html-tabs/`。未执行干净机器安装/升级/卸载验收；正式签名仍需用户证书或签名账号。
