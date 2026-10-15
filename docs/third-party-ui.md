# 本轮复用与参考

| 来源 | 使用方式 | 许可证 |
| --- | --- | --- |
| [Tauri 桌面窗口](https://github.com/tauri-apps/tauri/tree/dev/crates/tauri) | 使用现有窗口与 WebView API 承载自绘单行标题和菜单，保留原有命令控制器 | MIT / Apache-2.0 |
| [Lucide](https://github.com/lucide-icons/lucide/tree/main/icons) | 复用 10 个 SVG 文件，随 UI 打包 | ISC；部分图标源自 Feather MIT，完整声明位于 app/ui/icons/LICENSE.txt |
| [VS Code Activity Bar](https://github.com/microsoft/vscode/blob/main/src/vs/workbench/browser/parts/activitybar/activitybarPart.ts) | 参考图标栏与导航区的布局和展开行为，没有复制源码 | MIT |
| [CodeMirror 5.65.20](https://github.com/codemirror/codemirror5) | 原样复用本地运行库、语法模式、查找游标与括号匹配；每标签使用独立 Doc | MIT，app/ui/vendor/codemirror/LICENSE |
| [Prettier 3.6.2](https://github.com/prettier/prettier) | 原样复用 standalone / parser；格式插件的前端适配在离线 Worker 调用 | MIT，app/ui/vendor/prettier/LICENSE |
| [office_oxide 0.1.14](https://github.com/yfedoseev/office_oxide) | Cargo 依赖，解析旧 DOC/XLS/PPT 后将正文转换到现有块模型 | MIT / Apache-2.0，docs/licenses/office-oxide-LICENSE-* |
| [DBX 插件平台](https://github.com/t8y2/dbx/blob/main/plugins/README.md)与[插件清单实现](https://github.com/t8y2/dbx/blob/main/apps/desktop/src/lib/plugins/pluginMarketplace.ts) | 参考管理分区、平台选择、包先校验后安装。保留 vw 自己的 .vwpkg / manifest / JSON-RPC 协议 | Apache-2.0 |
| [DBX VSign 脚本](https://github.com/t8y2/dbx/blob/main/.github/scripts/vsign-sign.ps1) | 改编云签名调用与原子替换；改为使用用户自己的调用身份，验证签名和时间戳 | Apache-2.0，完整许可证位于 tools/windows/licenses/DBX-LICENSE.txt |

DBX 源码参考版本：270a61cd1e8777155a824d6dd897109b5bc5769a。vw v1 不引入 DBX 官方商店、.dbxp、Ed25519 信任目录或数据库 Host API。插件权限与校验按当前设计文件 §7.3、§8 和 §10 实施。
