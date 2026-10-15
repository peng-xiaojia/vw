# 格式插件能力注册

公共框架负责菜单、标签、保存和能力派发；格式解析、查看模型、目录和格式化归插件实现。内置插件使用同一 manifest 声明，不根据 UI 中的后缀分支临时启用能力。

`contributes.viewers[].capabilities` 支持 `view`、`edit`、`outline`、`format`。`Doc` / SDK `RpcDoc` 的 `outline()` 返回目录；`format_support()` 返回可选 `FormatSupport { provider, parser }`。没有格式化声明或描述符时，公共格式化入口禁用。

外置 stdio 插件使用 `provider: "rpc"`；框架调用 `formatSupport` RPC 获取描述，再调用 `format`，参数为 `handle` 与 `text`，返回格式化后的字符串。插件实现 `format_text()`；失败返回协议错误，不改变草稿。旧插件默认不提供格式化，保持兼容。

内置文本插件把前端格式化实现放在 `plugins/text/ui/formatter.mjs`，构建时复制到生成的 UI 资源目录；它向框架 `registerFormatter()` 注册 `prettier` provider，按插件描述的 parser 调用离线 Worker。HTML、Markdown 与文本插件分别声明适用 parser。外置插件当前使用 RPC 提供自有算法，不支持安装后执行任意前端脚本。

JSON/JSONL 的算法在 `plugins/json/src/format.rs`，验证后仅改变字符串之外的空白，保留重复键、键顺序和大数字原文字面量。SH/BAT 可查看与编辑，尚无格式化算法，框架禁用按钮。

格式化只更新草稿，并通过文档版本与草稿比较防止异步结果覆盖新输入。关闭前等待格式化完成，再检查未保存内容。公共编码/换行选项绑定发起操作的标签，并在关闭冻结后停止更新。

编码由 `OpenOptions.encoding` 与 `EditableDoc::serialize_with_encoding()` 选择。默认 trait 对非空编码参数返回不支持；支持转换的插件复用 `TextFile::decode` / `serialize_as` 做严格无损转换和本格式校验。内核仍执行大小上限、冲突检测、备份、原子写入和新编码重开。外置旧插件尚未注册编码转换能力，不能通过框架强制转码绕过插件。

二进制模式独立使用 hex viewer 标签，所有正常文档均可另外查看字节；Office 插件仍只读，不能用文本编辑器改写 DOC/XLS/OOXML 二进制容器。
