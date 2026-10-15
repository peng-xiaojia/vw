# 编辑器功能参考

当前 `vw` 优先把查看与轻量编辑做好。以下取自官方功能说明，按本软件的文件类型和使用场景排序。

| 功能 | 参考 | 在 vw 中的价值与现状 |
| --- | --- | --- |
| 分层目录、单项及批量折叠 | [VS Code Outline / folding](https://code.visualstudio.com/docs/editing/codebasics) | 长 Markdown、Office 文档快速定位；已支持目录树折叠。 |
| 源码与展示切换 | [VS Code Markdown](https://code.visualstudio.com/docs/languages/markdown)、[Obsidian](https://help.obsidian.md/Live%2Bpreview%2Bupdate) | Markdown/HTML 已有两种模式，展示模式可查看当前未保存草稿。 |
| 双栏同步预览 | [VS Code Markdown](https://code.visualstudio.com/docs/languages/markdown) | 适合反复修改 Markdown；需要先解决大文档增量渲染、滚动锚点和未保存预览。 |
| 文件监控与自动追尾 | [Notepad++ User Manual](https://npp-user-manual.org/docs/user-interface/) | 特别适合 `.log`，比手动重开更有用；应区分自动刷新和编辑冲突。 |
| 文件/章节面包屑与快速打开 | [VS Code User Interface](https://code.visualstudio.com/docs/editing/getting-started/userinterface) | 适合目录层级较深时定位，但需保持工具栏紧凑。 |
| 行号、自动换行、编码与换行符状态 | [Notepad++ User Manual](https://npp-user-manual.org/docs/user-interface/) | 已提供公共行号/换行开关、严格重新解码及保存编码选择、LF/CRLF/CR；文档地图尚未实现。 |
| 代码高亮、每标签撤销、查找替换 | [CodeMirror 5](https://codemirror.net/5/doc/manual.html) | 复用 CodeMirror 5.65.20；支持脚本/代码编辑、大小写查找、替换/全部替换，全部替换为单次撤销。 |
| 插件格式化、失败保留草稿 | [Prettier standalone](https://prettier.io/docs/browser) | 复用离线 Prettier 3.6.2 Worker；JS/TS、HTML、Markdown、CSS 和 YAML 等支持，JSON 由自身插件保留词法信息排版。 |

HTML 展示现使用独立网页协议与 sandbox iframe，保留网页原始 CSS、本地图片、字体与脚本交互。相对路径按原目录解析，支持目录内子页面导航；页面无法访问应用 DOM / IPC，不开放远程 CDN、浏览器存储与表单提交。大于文本插件可编辑上限的 HTML 仍可只读展示，单资源最多 64 MiB。

多文档交互参考 [VS Code 编辑器标签](https://code.visualstudio.com/docs/getstarted/userinterface#_editor-groups)：文件多选、同路径激活已有标签、脏文档标记、关闭前确认、Ctrl+Tab 切换与中键关闭。每个标签复用原文档控制器，保留草稿、阅读与折叠状态；CodeMirror.Doc 分别保留各标签编辑/只读文本模式的撤销、选区和滚动状态。
