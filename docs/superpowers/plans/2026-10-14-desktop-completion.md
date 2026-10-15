# 桌面交付整改实现计划

目标：落实 2026-10-14 审查的 R1–R16，以及文件选择、目录树、文件关联、无控制台启动和可重复 Windows 发布。用户已授权完整实施，保留现有插件内核；本计划与此前审查报告一起构成实施规格。

架构：核心保存成功后重建插件文档，归档成员强制只读；GUI 用统一文档控制器管理草稿和异步请求；原生桥提供选择器、目录浏览、受控链接和资源。安装版与便携版共用 GUI，数据位置与注册策略分开。

## 文件责任与固定接口

- 核心任务：`crates/vw-core/**`、`crates/vw-plugin-host/**`、`plugins/text/**`、`plugins/hex/**`；修复保存刷新、归档只读、生命周期、按需读页和 sidecar 控制台。保持公开 Session / OpenedDoc 调用方式可兼容现有外壳。
- 界面任务：`app/ui/**`、`app/ui-tests/**`；文档控制器、安全 DOM、目录树、大纲、分页、草稿保护、拖放和快捷键。
- 原生桥任务：`app/src/**`、`app/tests/**`、`app/Cargo.toml`、`app/build.rs`、`app/tauri.conf.json`、`app/capabilities/**`、`plugins/dir/**`、`tools/windows/**`；原生选择、外部链接、资源、启动路由、数据位置、打包与签名入口。
- 集成任务：根 `Cargo.toml`、`crates/vw-std/**`、新增图片插件、README、能力矩阵、性能验证和最终审查。

保留现有 `vw_open / vw_page / vw_outline / vw_search / vw_snapshot / vw_save / vw_close`。新接口：

- `vw_pick_path({directory: boolean}) -> string | null`：原生文件或目录选择。
- `vw_list_dir({path: string}) -> {path, parent, entries: [{name,path,isDir,size}]}`：目录插件负责列表，外壳负责展开状态。
- `vw_startup_paths() -> string[]`，事件 `vw-open-paths` 载荷 `string[]`：首次启动与运行中再次打开统一进入控制器。
- `vw_open_external({url: string}) -> void`：仅允许 HTTP/HTTPS/mailto，交给系统浏览器。
- `vw_export({handle,path})`：若加入归档成员导出，明确为另存，不声称更新 ZIP。
- 受控资源 URL：`http://vw-blob.localhost/<handle>/<encoded-id>`（Windows），`vw-blob://localhost/...`（其他平台）。后端仅允许当前句柄的 blob，不接受任意本地文件路径。

## 执行顺序与验收

- [x] 核心安全回归：增加「保存后 snapshot/page/search 更新」「连续两次保存」「归档不可保存」「释放会话」测试；运行红灯，修复后跑核心与格式套件。
- [x] GUI 回归：用真实控制器验证草稿保存/放弃/取消、失败打开保留完整旧文档、旧请求不污染新页、串行分页、句柄释放、保存后展示刷新；使用 `node --test app/ui-tests/*.test.mjs`。
- [x] 链接和资源：测试脚本 URL 被拒绝、外部链接不导航应用页面；实现图片 blob 渲染并拒绝越界资源。
- [x] 桌面浏览：原生选择文件/文件夹；左侧目录树按需展开、返回上级、刷新、文档大纲；搜索定位真实目标；拖放与快捷键共用打开入口。
- [x] Windows 集成：GUI PE Subsystem=2；sidecar 不新建控制台；文件参数支持中文和空格；再次打开进入已有窗口且遵守草稿保护；普通用户数据目录可写。
- [x] 发布工程：产出 release GUI、便携 ZIP 和安装包；安装/卸载注册「打开方式」与支持格式，不改写 UserChoice；提供证书签名与时间戳命令，校验最终产物，不伪造签名成功。
- [x] 性能与格式：文本和 hex 按需页读取，后台 IPC；实际支持图片，验证已有格式；性能报告注明文件规模、首屏和内存，不以分页接口代替测量。
- [x] 最终验证：`cargo test --workspace`、前端行为测试、release 构建、PE/签名/产物检查；独立审查规格符合性与代码质量，修复发现；更新能力矩阵和审查整改状态。

代码签名证书、独立仓库发布和真实干净机器的系统策略是外部条件；完成所有本地实现与可验证产物后，逐项报告已验证结果及这些条件，不能把它们冒充完成。

## 本机收尾记录

2026-10-14：上述本地实现、前端32项、Rust工作区91项、真实WebView2窗口冒烟与64MiB首屏/宿主内存测量已通过；release程序、便携ZIP、NSIS安装包已生成。清单勾选表示本地实施完成；正式签名与干净Windows安装/升级/卸载属于仍未执行的外部验收，详见 `docs/reviews/2026-10-14-desktop-completion-validation.md`。
