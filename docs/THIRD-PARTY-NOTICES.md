# 第三方代码声明

本文件登记所有从外部项目移植（逐字复制或改写）的代码。**只借鉴设计思想与接口形态的不计入**——那不需要署名。

规则与判据见 [以 dbx 作为参考实现的使用方式](reference/dbx-as-reference.md) §6。

| 项目 | 内容 |
| --- | --- |
| 版本 | v1.3 |
| 日期 | 2026-10-15 |

任何修改必须递增版本号并在文末修订记录追加一行。

## 登记台账

| 来源仓库 | 锚点 commit | 原文件路径 | 目标文件路径 | 修改摘要 | 日期 |
| --- | --- | --- | --- | --- | --- |
| t8y2/dbx | 270a61cd1e8777155a824d6dd897109b5bc5769a | .github/scripts/vsign-sign.ps1 | tools/windows/vsign-sign.ps1 | 改为用户自己的调用身份；替换产物前验证有效 Authenticode 和时间戳 | 2026-10-14 |
| lucide-icons/lucide | 2026-10-14 获取的 SVG；文件随仓库固定 | icons/{folder,folder-open,file-text,file-code,image,file,search,list-tree,blocks,settings}.svg | app/ui/icons/ 同名文件 | 原样复用 SVG，无图标逻辑修改 | 2026-10-14 |

## 新增复用与测试样本

[CodeMirror 5.65.20](https://github.com/codemirror/codemirror5) 与 [Prettier 3.6.2](https://github.com/prettier/prettier) 来自 npm 官方包，原样选择 lib、mode、addon、standalone 和 parser 模块，固定在 `app/ui/vendor/`，用于离线编辑与格式化。MIT 许可和版权声明分别在 [CodeMirror LICENSE](../app/ui/vendor/codemirror/LICENSE) 与 [Prettier LICENSE](../app/ui/vendor/prettier/LICENSE)，随安装包与便携包分发。

[office_oxide 0.1.14](https://github.com/yfedoseev/office_oxide) 通过 Cargo 锁定版本，用于旧版 DOC/XLS/PPT 内容提取，采用 MIT / Apache-2.0 双许可。[encoding_rs 0.8.42](https://github.com/hsivonen/encoding_rs) 用于常用编码无损转换，包含 Apache-2.0 / MIT 及 WHATWG 许可与版权声明。许可副本保存在 `docs/licenses/`，随安装包与便携包分发。

Apache POI 的 DOC 测试文件与 calamine 的 XLS 测试文件仅用于测试，来源记录在 `plugins/office/tests/fixtures/README.md`，各自 Apache-2.0、NOTICE 和 MIT 许可在 `docs/licenses/`；测试样本不随应用分发。

## 许可全文

DBX 的 Apache-2.0 许可全文保存在 [dbx-LICENSE-apache-2.0.txt](licenses/dbx-LICENSE-apache-2.0.txt)。该锚点根目录没有 NOTICE 文件，所移植脚本没有其他著作权声明。DBX 仅作为工程实现参考，商标与品牌不作为 vw 产品标识。

Lucide 的 ISC 及源自 Feather 的 MIT 声明保存在 [LICENSE.txt](../app/ui/icons/LICENSE.txt)，包含 Lucide Icons and Contributors 与 Cole Bemis 的版权声明。便携包随附以上许可证与本声明。

Tauri 使用现有依赖承载桌面窗口，不移植源码；其许可证为 MIT / Apache-2.0。VS Code Activity Bar 和 DBX 插件管理仅参考交互与校验设计，未复制相应组件源码。

## 修订记录

| 版本 | 日期 | 说明 |
| --- | --- | --- |
| v1.2 | 2026-10-14 | 登记 DBX VSign 脚本和 Lucide SVG；补许可全文与分发声明 |
| v1.3 | 2026-10-15 | 登记离线 CodeMirror / Prettier、Office 和编码依赖、测试样本来源及分发许可 |
| v1.1 | 2026-10-13 | 统一第三方许可副本存放位置为 `docs/licenses/`；引入版本记录 |
| v1.0 | 2026-10-13 | 初版 |
