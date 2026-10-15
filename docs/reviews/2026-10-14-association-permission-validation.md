# 关联格式权限修复验证

用户报告关联保存返回 os error 5，Windows 默认应用设置入口失败。

根因验证：工作区及生成 EXE 的 icacls 输出包含继承的 Low Mandatory Level；原生诊断进程的令牌为 Low，正常 PowerShell 为 Medium。同一隔离注册表测试在工作区执行时创建键失败，复制到普通用户临时目录后通过。没有修改目录 ACL、注册表权限或系统安全策略。

实现：关联注册只请求所需读写权限；错误包含失败步骤及普通用户目录运行提示。系统设置入口加入绝对系统路径 Explorer 的 URI 激活后备调用，避免从当前目录搜索可执行文件。注册、勾选项持久化、取消格式、清除和另一份应用注册保护使用同一实现进行隔离测试。

便携 ZIP 应在普通用户目录解压后运行。直接点击开发工作区中的 EXE 可能继续受 Low 标签限制。默认应用仍由用户在 Windows 中指定。

最终验证：`cargo test -p vw-app --release --offline` 通过（两项需要系统写入/打开设置的测试默认忽略）。将同一构建的测试 EXE 复制到普通用户临时目录后执行 `--include-ignored --nocapture`，四项关联测试全部通过，无忽略。前端 36 项回归通过。

本轮交付目录为 `C:\Users\jjiap\Downloads\vw-associations-fixed-20261014`，与受限开发工作区分开。没有写入用户当前默认应用的 UserChoice，也没有修改目录或注册表 ACL。
