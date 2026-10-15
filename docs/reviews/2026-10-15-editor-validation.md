# 公共编辑与格式插件验收

日期：2026-10-15。开始修改前已将上一轮代码提交为 `5efae70`。本轮继续现有文档控制器和插件框架，复用 CodeMirror 5.65.20、Prettier 3.6.2 与 office_oxide 0.1.14，来源和许可见 [第三方声明](../THIRD-PARTY-NOTICES.md)。

## 自动验证

| 命令 / 环境 | 实际结果 |
| --- | --- |
| `node --test app/ui-tests/*.test.mjs` | 53 项通过，0 失败 |
| `cargo test --workspace --offline` | 108 项通过，0 失败；2 项原系统交互测试忽略 |
| `node app/ui-tests/feature-smoke.cjs` | Chromium 的菜单、主题、侧栏宽度、插件中心、JSON/Markdown 折叠、HTML 隔离、标签、多选与未保存保护通过 |
| 独立 identifier `dev.vw.test.editor` 构建后运行 `node app/ui-tests/native-editor.cjs` | 真实 WebView2/IPC：离线 Prettier Worker、高亮、查找下一个、替换及单次撤销/重做、公共行号/换行、独立二进制标签、每标签撤销历史、文本搜索定位通过 |
| 同一原生编辑测试 | UTF-16 LE BOM / CRLF 的实际保存字节、原文件备份、GBK 重新解码后 UTF-8 / LF 保存通过；DOC/XLS/DOCX/XLSX 默认内容预览与二进制切换通过；底部状态栏未超出视口 |
| 独立 identifier `dev.vw.test.htmltabs` 构建后运行 `node app/ui-tests/native-html-tabs.cjs` | HTML 原样式、字体、Logo、网格与浏览器计算样式匹配；子页面、隔离和标签回归通过 |

测试应用使用独立 single-instance 标识与缓存目录，不向用户正在使用的软件传入测试路径。原生证据在 `output/playwright/editor/`、`output/playwright/html-tabs/`；Rust 完整输出在 `target/editor-workspace-final.log`。

## 审查修复

两名只读审查代理分别检查前端异步状态及后端格式/Office/编码。修复了 JSON 用 Value 重排会丢重复键与大数字的问题，改为验证后仅改变空白；DOC 列表和嵌套正文遗漏的问题，增加真实 DOC 样本；大于 1 MiB UTF-16 BOM 文本误入 UTF-8 分页的问题；GBK 与 GB18030 的选择项区分。

前端修复格式化期间退出丢失新草稿、重解码时保存已选择丢弃的旧草稿、异步编码选择误应用另一标签、文本搜索定位到隐藏渲染器。复核再发现关闭冻结后异步选项仍产生修改，现统一到标签层，等待前后检查关闭状态及原标签存在。新增跨标签和关闭冻结测试，移除关闭检查可复现失败，恢复后通过。

## 支持范围

SH/BAT/CMD/PowerShell、JS/TS 等常用文件已注册文本查看与编辑；格式化由插件能力启用。JSON/JSONL、JS/TS、HTML、Markdown、CSS/SCSS/Less、YAML 当前有实现；Shell、BAT、Python、Rust 等暂未提供格式化器，入口禁用。格式化只更新草稿；无效语法、超限或超时保留原文。

公共行号和自动换行作用于文本/源码。常用编码重新解码与保存编码分别选择；编码转换拒绝不可表示字符，保存继续执行格式校验、冲突检测、备份和原子写入。外置旧插件默认拒绝编码转换，不能绕过其序列化契约。

所有正常文件可额外使用只读二进制标签，保留原标签草稿。Office 为正文与表格内容查看，不承诺 Word/Excel 原版排版、公式重算或编辑。旧 Office 输入限制 32 MiB；损坏、加密或解析库不支持的文档会报错。

本轮未执行干净机器安装/升级/卸载、SmartScreen 声誉与签名验收。正式代码签名仍需要用户证书或账号，本轮未签名。

## 发布产物

`pwsh -NoProfile -File tools/windows/build-release.ps1 -OutputDirectory dist/windows-20261015-editor` 成功生成安装包、便携 ZIP、EXE、manifest 和 SHA256SUMS。独立重新计算三项产物摘要与 manifest 相同，ZIP 内 EXE 与外部便携 EXE 摘要相同；ZIP 随附 CodeMirror / Prettier / Office / 编码库许可证，安装资源声明对应文件均存在。发布构建先清除测试 `TAURI_CONFIG`，使用配置中的生产 identifier `dev.vw.app`。发布脚本同时验证 EXE 与安装包使用 GUI 子系统。

便携包：`dist/windows-20261015-editor/vw-windows-portable.zip`。安装包：`dist/windows-20261015-editor/vw_0.1.0_x64-setup.exe`。本轮源码在构建时尚未提交，manifest 如实记录 `sourceDirty: true`；构建后的 Git 提交不改动这些产物字节。
