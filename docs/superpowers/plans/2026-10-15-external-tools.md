# 外部编辑器与终端入口实现计划

**目标：** 在展示/源码切换旁提供其他软件打开与终端打开，并保留原标签与草稿。

**架构：** 文档控制器绑定当前文档句柄并检查未保存内容；原生命令只根据已打开句柄取得真实源路径。Windows 用 SHOpenWithDialog 选择应用，用系统 PowerShell 的新控制台启动到文件目录，文件名不进入命令字符串。

**范围：** 取消或保存失败不启动；不保存时保留当前草稿并交出磁盘版本；归档成员需先导出；文件与二进制标签可用；目录可开终端。编辑器启动返回后不宣称外部修改已同步到当前标签。

- [x] `app/ui-tests/external-actions.test.mjs`：先验证取消、保存失败、磁盘版本、异步标签绑定、关闭等待和归档禁用。
- [x] `app/ui/controller.mjs`、`tabs.mjs`：实现绑定句柄的外部动作与关闭保护。
- [x] `app/src/external_open.rs`、`lib.rs`、`app/tests/gui.rs`：句柄路径验证、Windows 应用选择器与安全终端启动；测试未知/已关闭句柄、归档、特殊字符和正确目录。
- [x] `index.html`、`main.js`、`menu.mjs`、`style.css`、`desktop_menu.rs`：展示/源码旁按钮及文件菜单，未打开与繁忙时灰色，窄窗口可用。
- [x] 运行 JS、Rust app 和 Chromium 回归，独立原生窗口验证布局；只读代码审查。
- [x] 更新能力文档与验证记录，构建并核验 Windows 产物，本地提交，不推送。
