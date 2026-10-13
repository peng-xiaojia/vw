# 文件查看器（代号 vw）设计规格

| 项目 | 内容 |
| --- | --- |
| 版本 | v1.1 |
| 日期 | 2026-10-13 |
| 状态 | 已通过设计评审，待用户确认规格 |
| 代号 | vw（临时代号，可随时更名） |
| 参考项目 | [t8y2/dbx](https://github.com/t8y2/dbx) `051cab8`（仅作参考实现，不 fork，详见 [使用方式](../../reference/dbx-as-reference.md)） |

---

## 1. 定位

一个**插件化的桌面文件查看与编辑工具**：以图形界面（GUI）为主交付物，供人查看各种格式的文件，并对其中的文本类格式做编辑保存；同时提供 CLI 与 MCP 两个接口，供脚本和外部 AI agent 使用。

一句话概括架构：**内核零格式知识，一切格式能力都是插件；插件只产出一份统一的文档模型，所有外壳消费同一份模型；编辑能力同样跟随插件，核心只负责保存管线。**

---

## 2. 目标与非目标

### 2.1 目标

1. **给人用的 GUI 是第一优先级。** 打开文件、翻阅、搜索、看结构，体验要顺。
2. **内核极小且与格式无关。** 内核里不出现任何具体格式的解析代码，也不出现「认不出的文件怎么办」这类兜底逻辑。
3. **格式能力可无限扩展。** 新增格式 = 新增插件，不需要改内核。
4. **重依赖不进内核。** PDF、Office、媒体这类重格式的依赖由插件自带，按需下载。
5. **架构统一可验证。** 「内置插件」与「外置插件」走同一套契约，差异只在传输介质；这一点由测试强制保证。
6. **agent 可用。** 同一份文档模型通过 CLI 和 MCP 暴露，且与 GUI 走完全相同的代码路径。
7. **简单格式可编辑并保存。** 编辑能力**跟随插件**：插件自行声明是否可编辑，文本类格式可编辑保存，复杂格式（PDF、Office、媒体）只读即可。核心不实现任何编辑语义。

### 2.2 非目标（YAGNI）

| 非目标 | 理由 |
| --- | --- |
| 自动合并冲突 | v1 只做冲突**检测**与阻断，不做三方合并。合并需要理解格式语义，属于插件的职责 |
| 核心级撤销栈 | 撤销由前端编辑器的 undo/redo 承担。核心若实现文档级撤销栈，就必须理解编辑语义，等于把格式知识搬回内核 |
| 内置 AI 助手 | 明确不需要。agent 能力通过 MCP 交给外部工具 |
| 云同步、账号体系 | 与「轻」冲突 |
| Web 版与 Docker 部署 | dbx 有此能力，我们不需要 |
| 插件市场服务端与人工审核 | 采用 GitHub Releases 分发，零服务端 |
| 终端交互界面（TUI） | GUI 已覆盖人用场景，TUI 是重复投资 |
| 远程文件（URL / S3 / SSH） | v1 只做本地。`Source` 抽象预留扩展位 |
| 插件贡献自定义 UI | 会破坏「内容只有一种形状」的统一性，见第 3 章决策 D6 |

---

## 3. 关键决策记录

| 编号 | 决策 | 结论 | 核心理由 |
| --- | --- | --- | --- |
| D1 | 内核架构 | 插件化，**内核零格式知识**；内置格式也是插件，只是 `transport` 不同 | 内核若含格式知识，插件协议就退化为「另一条路」，架构不再统一 |
| D2 | 插件契约 | 全局**只有一份文档模型**（Document IR）；插件不产出 HTML 或自定义视图 | 「架构统一」的落点就是「内容只有一种形状」 |
| D3 | 重格式依赖 | sidecar 插件自带依赖，按需下载 | 与「内核极轻」直接冲突的部分必须外置 |
| D4 | 主界面 | Tauri 2 + 系统 WebView（不打包 Chromium），前端极简 | 「查看」的本质是渲染富内容，HTML/CSS 是最省力的技术栈 |
| D5 | 插件分发 | GitHub Releases 为源，无中心 registry | 零服务端、零审核、零运营；插件作者可独立迭代 |
| D6 | 插件 UI 入口 | **不提供** | dbx 的 `ui` 入口使插件内容出现两种形状，破坏了 IR 唯一性 |
| D7 | dbx 代码复用 | **不 fork 整体**；内核重写，工程资产移植，插件宿主精读后重写 | 见 3.1 |
| D8 | 产品范围 | 自用优先，协议上给开源生态留口子 | manifest 带协议版本号，签名位预留但 v1 不启用 |
| D9 | 编辑能力 | 编辑**跟随插件**，声明为可选能力；核心不实现任何编辑语义，只提供保存管线 | 查看可以统一（渲染是核心职责），编辑不能统一（每种格式的编辑语义天然不同）。强行统一会让 IR 长出编辑语义，最终变成通用编辑器框架 |

**D2 与 D9 不矛盾。** D2 约束的是「插件产出给核心渲染的内容」——只有一种形状。编辑是反方向的：用户的编辑意图经核心收敛为「一份新的文件字节」。因此准则是：**查看统一，编辑分级**。

### 3.1 为什么不对 dbx 做「复制后删减」

实测数据（GitHub API，2026-10-13）：

| 指标 | 数值 |
| --- | --- |
| 仓库体积 | 315 MB |
| 源码统计 | Rust 26.2 MB、TypeScript 19.7 MB、Vue 11.5 MB、Go 3.7 MB、Java 3.6 MB，合计约 65 MB（百万行量级） |
| 热度 | 26,073 stars、2,372 forks、1,131 个 open issue |
| 活跃度 | 当日仍有 push |
| 许可 | Apache-2.0 |

否决理由，按重要性排序：

1. **删减不产生价值。** 文件查看场景能用到的部分不足 3%，其余是 100+ 数据库驱动、连接管理、凭据加密、AI 助手、Web/Docker 后端、消息队列控制台。工作量会全部花在「识别边界并删除」上。
2. **领域模型不匹配。** dbx 的内核抽象是「连接 / 查询 / 结果集」，本项目需要的是「源 / 文档 / 块」。这不是删减能得到的，只能换血。
3. **上游过于活跃，fork 后无法跟进。** 当日仍在提交、上千个 open issue 的仓库，fork 等于把一份持续演进的参考实现降级成一次性快照。
4. **会继承需要砍掉的插件假设。** `ui` 入口、workbench、connection 语义、permission 命名，恰好都是本项目要移除的，改到最后等同于重写。
5. **构建负担。** 其 Cargo workspace 含 sqlx、tiberius、mongodb、duckdb（作者亦提示 DuckDB 编译耗时，建议本地开发用 `--no-default-features` 跳过）。本项目内核应当秒级编译完成。

**可移植的三类资产（不重写）：**

| 资产 | 做法 | 理由 |
| --- | --- | --- |
| 工程与分发：Tauri 打包、多平台 CI 矩阵、代码签名、自动更新、CLI 按平台发包 + npm launcher | 照搬或改造 | 与领域完全无关，自建极费时间 |
| 插件宿主：manifest schema、签名校验、sidecar 生命周期、stdio JSON-RPC framing、权限声明 | 精读后重写 | 设计思想值得借鉴，实现与数据库概念深度耦合 |
| 踩坑经验 | 直接吸收 | 例如「DuckDB 编译慢，本地开发跳过」「Windows 便携版需整体搬迁 data 目录」 |

移植代码需遵守 Apache-2.0：保留 LICENSE 与 NOTICE，并标注修改过的文件。**完整的使用方式——分阶段精读清单、移植判据、合规流程、版本锚定与红线——见 [以 dbx 作为参考实现的使用方式](../../reference/dbx-as-reference.md)。**

### 3.2 被否决的技术方案

| 方案 | 否决理由 |
| --- | --- |
| 纯 WASM 沙箱插件（wasmtime） | wasmtime 本身给内核增加 5–15 MB；WASI 下无法调用系统程序、无原生线程，PDF / Office / FFmpeg 几乎不可能编译进 WASM，与「重格式自带依赖」的诉求直接冲突 |
| egui / eframe 纯原生渲染 | 省下约 10 MB，但表格、富文本、PDF 版式、Markdown 样式全需从零绘制，UI 工作量数倍于 WebView 方案 |
| Electron | 自带 Chromium，体积 120–200 MB，直接违背「轻」 |
| 开放插件的 `ui` 入口（dbx 完整模式） | 插件内容出现两种形状，破坏 IR 唯一性（见 D6） |
| TUI | GUI 已覆盖人用场景（见非目标） |

---

## 4. 系统架构

### 4.1 分层

```
┌──────────── 外壳 shells（薄，均无特权路径） ────────────┐
│ GUI   Tauri 2 + WebView      主交付物，人用             │
│ CLI   vw file [--json|--raw] 脚本化 + 一致性测试驱动    │
│ MCP   vw mcp (stdio)         外部 agent 用              │
└───────────────────────┬─────────────────────────────────┘
                        │ 同一个 Session API
┌───────────────────────▼─────────────────────────────────┐
│ core —— 零格式知识                                      │
│  registry  插件注册表 / 发现 / probe 调度               │
│  doc       Document IR（唯一数据契约）                  │
│  session   懒加载、流式分页、搜索、取消、超时            │
│  policy    权限、路径校验、原子写与冲突检测             │
│  render    text(→纯文本) / json(→IR JSON)               │
└──────┬──────────────────────────────┬───────────────────┘
       │ Viewer 契约                  │ Viewer 契约
┌──────▼──────────────────┐  ┌────────▼──────────────────┐
│ 内置插件 in-process     │  │ 外置插件 sidecar          │
│ text / markdown / json  │  │ pdf / office / media /    │
│ csv / hex               │  │ parquet / sqlite …        │
│ P2 增补 dir / archive   │  │ 自带依赖，按需下载        │
│ transport: 内存通道     │  │ transport: stdio JSON-RPC │
│ 同一 manifest、同一 probe│  │ 同一 manifest、同一 probe │
└─────────────────────────┘  └───────────────────────────┘
```

**硬约束：`vw-core` 不依赖 Tauri。** 否则 CLI 与 MCP 会被迫拖入整个 GUI 依赖树，体积失控。

### 4.2 统一的是什么：契约，不是进程

插件模型拆成两个正交概念：

| 概念 | 内容 | 是否统一 |
| --- | --- | --- |
| 契约（descriptor） | manifest schema、probe 打分语义、分页与搜索语义、错误模型 | **完全统一** |
| 传输（transport） | 内存通道（同进程）与 stdio JSON-RPC（子进程） | 允许不同，是实现细节 |

协议本身定义成**消息**；传输只决定消息怎么走：

- 内置插件在**内存通道**上跑同一套消息。**序列化必须真实发生**：内存通道同样执行完整的 JSON 序列化与反序列化（写入内存缓冲再解析回来），不允许结构体引用直通——否则 conformance 测不到序列化层的 schema 漂移，「统一」只存在于类型系统而非协议之上。这笔开销可忽略。
- 外置插件在 **stdio** 上跑同一套消息，一行一个 JSON（JSON Lines）。

于是「内置 vs 外置」的差异**只剩下传输介质**。内核侧只有唯一一个调用点，若内核中出现「如果是内置则……否则……」这类分支，统一性即被破坏。

**这一点由测试强制**：同一份 conformance 用例分别驱动两种 transport，两边输出必须一致（见第 12 章）。

### 4.3 渲染器

渲染分两处，但消费的是同一份 IR。**内核不拼 HTML**，这样既避免字符串转义与注入类问题，也让核心更薄：

| 渲染器 | 位置 | 输出 | 使用方 |
| --- | --- | --- | --- |
| `render/text` | Rust core | 纯文本或 ANSI 转义 | CLI 默认输出 |
| `render/json` | Rust core | IR 的 JSON 序列化 | CLI `--json`、MCP、GUI |
| `render/dom` | 前端 TypeScript | DOM | GUI |

**三个外壳拿到的是同一份 JSON IR。** GUI 通过 Tauri 的 command/event 通道取 JSON IR，CLI 通过 `--json` 取，MCP 通过工具调用取——三者的数据格式与代码路径完全相同。这使得「无特权路径」不只是设计承诺，而是可断言的事实：比较三者的 JSON 输出即可验证。

---

## 5. 契约定义

### 5.1 控制面：Viewer 接口

```rust
/// 插件对外的唯一入口。
pub trait Viewer: Send + Sync {
    fn manifest(&self) -> &Manifest;

    /// 0..=100。返回 None 表示明确不处理。
    fn probe(&self, src: &Source) -> Option<ProbeScore>;

    fn open(&self, src: &Source, opts: &OpenOptions) -> Result<DocHandle, PluginError>;
}

/// 一个已打开的文档会话。按需取内容，不要求一次读完。
pub trait Doc: Send {
    fn meta(&self) -> DocMeta;
    fn outline(&self) -> Option<Outline>;
    fn page(&self, range: PageRange) -> Result<BlockList, PluginError>;
    fn search(&self, query: &str, from: Cursor) -> Result<SearchHits, PluginError>;
    fn blob(&self, id: BlobId) -> Result<BlobRef, PluginError>;
    fn close(self);
}
```

设计要点：

1. **`probe` 用打分而非布尔。** 同一文件常被多个插件认领（`.json` 既是文本又是 JSON），由分数与优先级裁决，用户可用 `--as <viewer-id>` 覆盖。
2. **`page` 是按需的。** 这是「大文件、大 PDF 不卡」的根源；插件无需一次性读完，会话可长驻。
3. **`blob` 用于二进制载荷。** 图片等较大二进制不经 JSON 传输，避免 base64 膨胀。插件把字节写进核心在 `open` 时下发的**会话临时目录**，以相对路径引用；核心校验路径边界后才经自定义协议暴露给 WebView。这一步是权限模型的一部分：没有它，插件就能借 blob 通道让核心读取任意用户文件（见 5.3 与第 10 章）。
4. **`PluginError` 携带统一的 `ErrorKind`**（见第 9 章），使两种 transport 的错误模型完全一致。

`Source` 设计为可组合，以支持「查看压缩包内的文件」这类场景：

```rust
pub enum Source {
    Path(PathBuf),
    Dir(PathBuf),
    /// 归档内的成员：外层源 + 内部路径
    Archive { outer: Box<Source>, inner: String },
    // 预留：Url(String) 等，v1 不实现
}
```

**`Archive` 成员的打开机制（P2 前需细化，此处先定原则）：** 归档插件负责把目标成员**物化**到会话临时目录（保留原扩展名），核心再以物化路径对全部插件**重新 probe**——对其他插件而言它就是一个普通文件，无需感知归档的存在；`Source::Archive` 保留溯源信息用于展示与校验。嵌套归档即该过程的自然递归。必须有**总解压上限**（默认 1 GB）与成员数上限以防 zip 炸弹，超限按 `MalformedSource` 报错。

### 5.2 数据面：Document IR

```rust
pub enum Block {
    Heading { level: u8, text: Inline },
    Para(Inline),
    List { ordered: bool, items: Vec<Block> },
    Table { head: Option<Vec<Inline>>, rows: Vec<Vec<Inline>>, align: Vec<Align> },
    Code { lang: Option<String>, text: String },
    KeyValue(Vec<(String, Inline)>),
    Quote(Vec<Block>),
    Image { blob: BlobId, alt: String, w: Option<u32>, h: Option<u32> },
    PageBreak,
    /// 受控的矢量绘图（暂缓引入，见下方说明）
    Canvas { w: u32, h: u32, ops: Vec<DrawOp> },
    /// 逃生舱：插件不想建结构时原样输出
    Preformatted { text: String },
    /// 局部失败，不炸整篇文档
    Error { message: String },
}

pub enum Inline {
    Text(String),
    Bold(Box<Inline>),
    Italic(Box<Inline>),
    Code(Box<Inline>),
    Link { text: Box<Inline>, href: String },
    Break,
}
```

**`Canvas` 暂缓引入。** 它是本 IR 中风险最高的块类型：`DrawOp` 一旦包含文本定位，IR 就开始长出排版语义，与纪律 1 直接冲突。P5 的 PDF 因此走**光栅化优先**——每页渲染为 `Image`，抽取的文本层用 `Para` / `Heading` 叠加；只有当光栅化质量确实不足时，再评审是否引入不含文本定位的受限 `DrawOp`。

四条防止 IR 退化为通用排版引擎的纪律：

1. **块类型数量受控。** 新增块类型需通过评审关卡：所有渲染器是否都能给出合理降级？
2. **降级必须可实现。** `render/text` 对 `Canvas` 降级为占位与替代文本，对 `Table` 降级为对齐文本。不存在让最弱渲染器无法处理的块。
3. **`Preformatted` 永远可用。** 任何插件即使完全不建结构，也能只丢纯文本，功能仍然成立。
4. **未知块类型必须可降级（前向兼容）。** 内核反序列化遇到未知的块 `kind`，一律映射为 `Block::Error`（附原始 kind 名），而不是让整个文档加载失败。这是「新插件跑在旧内核上」的日常演进通道，比 `manifest_version` 这类大版本号更重要。

### 5.3 传输协议

消息即 IR 的 JSON 形式，不另立数据模型。v1 使用 JSON Lines（一行一个 JSON 对象）。

```
→ {"id":1,"method":"probe","params":{"source":{"kind":"path","path":"/a/b.pdf"}}}
← {"id":1,"result":{"score":80,"reason":"magic %PDF-"}}

→ {"id":2,"method":"open","params":{"source":{...},"opts":{"encoding":null},"tmp_dir":"<session-tmp>/d1"}}
← {"id":2,"result":{"handle":"d1","meta":{"viewer":"pdf","mime":"application/pdf","pages":42}}}

→ {"id":3,"method":"page","params":{"handle":"d1","range":{"start":0,"count":200}}}
← {"id":3,"result":{"blocks":[ ... IR ... ]}}

→ {"id":4,"method":"editCapability","params":{"handle":"d1"}}
← {"id":4,"result":{"kind":"text","encoding":"utf-8","line_ending":"lf","has_bom":false}}

→ {"id":5,"method":"snapshot","params":{"handle":"d1"}}
← {"id":5,"result":{"kind":"text","text":"a,b\n1,2\n"}}

→ {"id":6,"method":"serialize","params":{"handle":"d1","edited":{"kind":"text","text":"a,b\n1,3\n"}}}
← {"id":6,"result":{"blob":"b7"}}   // 插件经 blob 通道交出字节，核心读取后原子写

// 用户快速滚动：第 7 个 page 请求尚未返回已被新请求取代，取消之
→ {"id":7,"method":"page","params":{"handle":"d1","range":{"start":200,"count":200}}}
→ {"id":8,"method":"cancel","params":{"target":7}}
← {"id":8,"result":{"cancelled":true}}
← {"id":7,"error":{"kind":"cancelled","message":"superseded"}}
```

示例只列出代表性消息；完整方法集以 `vw-protocol` 的定义为准（含 `meta`、`outline`、`search`、`blob`、`close`）。

约定：

- 一个文档 = 一条长驻会话，`handle` 标识；插件进程可服务多个文档。
- 每个请求带 `id`，支持乱序响应与取消。
- **会话临时目录**：核心在 `open` 时经 `tmp_dir` 下发本会话专属的可写目录，`blob` 引用一律是相对该目录的相对路径。核心把路径解析为绝对路径后校验必须落在目录内，越界即拒绝并记为协议错误——插件不能借 blob 通道让核心读取任意文件。会话结束由核心清理整个目录；插件崩溃遗留的孤儿文件，按目录时间在下次会话启动时清扫。
- 二进制经 `blob` 通道传递，不走内联 base64。
- **消息尺寸上限**：单条内联消息（一行 JSON）默认上限 8 MB（可配置）。超限内容——无论文本还是二进制——一律改走 blob 通道，避免几百 MB 的单行日志或超大代码块撑爆 stdio 与任一端的内存。
- **`serialize` 只把新字节写到会话临时目录，绝不写目标文件**；目标文件的写入由核心完成（见 5.4）。
- 内核收到无法识别的块 `kind` 一律降级为 `Block::Error`，不视为协议错误（前向兼容，见 5.2）。
- 错误统一为 `{"error":{"kind":"...","message":"..."}}`，`kind` 取值见第 9 章。

### 5.4 编辑契约（可选能力）

编辑**不是**核心的能力，而是插件的可选能力。核心只提供保存管线，不理解任何格式的编辑语义。

```rust
/// 插件可选实现。未实现即视为只读。
pub trait EditableDoc: Doc {
    /// 声明编辑方式
    fn edit_capability(&self) -> EditCapability;

    /// 取当前可编辑内容的快照，供编辑器填充
    fn snapshot(&self) -> Result<EditSnapshot, PluginError>;

    /// 校验并序列化用户编辑后的内容为文件字节
    fn serialize(&self, edited: &EditSnapshot) -> Result<Vec<u8>, PluginError>;
}

/// v1 只有两种取值。未知取值一律降级为只读，保证向前兼容。
pub enum EditCapability {
    /// 只读
    None,
    /// 纯文本编辑：核心提供通用文本编辑器
    Text {
        encoding: String,        // 如 "utf-8"
        line_ending: LineEnding, // Lf / CrLf / Preserve
        has_bom: bool,
        max_bytes: Option<u64>,
    },
}

pub enum EditSnapshot {
    Text(String),
}
```

**逐文件降级要求：** `edit_capability` 是逐文档调用的。插件检查具体文件内容后，若无法保证无损往返——典型如含无效 UTF-8 序列的「UTF-8」文件、存在无法映射回原编码的字符——必须对该文件返回 `None`（只读）。宁可少编辑一个文件，也不允许「打开 → 保存」悄悄改动用户数据。

**职责边界（本章的关键）：**

| 环节 | 负责方 | 说明 |
| --- | --- | --- |
| 能否编辑、以什么方式编辑 | 插件 | 通过 `edit_capability` 声明 |
| 编辑交互与 undo/redo | 前端编辑器 | 核心不实现撤销栈 |
| 编辑内容的合法性校验 | 插件 | `serialize` 返回 `Err` 即拒绝保存，错误直接呈现给用户 |
| 序列化（保持缩进、行尾、BOM、原格式） | 插件 | 只有插件知道原格式长什么样 |
| 冲突检测、原子写、备份、权限 | **核心** | 统一实施；插件**永不直接写文件** |

**保存管线（核心实现）：**

1. 用户触发保存 → 核心调 `serialize(edited)` 取得新字节。插件可在此校验并拒绝。
2. **冲突检测**：比对打开时记录的 `(mtime, size, hash)` 与当前磁盘状态，不一致即拒绝保存并提示文件已被外部修改。基线哈希用 BLAKE3，在 open 后由后台线程计算，不阻塞首屏；保存时若尚未就绪则等待其完成（可编辑文件受 `max_bytes` 约束，等待量级可控）。
3. **原子写**：写入同目录临时文件 → `fsync` → 原子重命名覆盖，避免中途失败损坏原文件。
4. **备份**：按配置保留一份 `.bak`，或写入带时间戳的备份。
5. 更新会话基线哈希，清除 dirty 标记。

**明确不做的事：** 不做三方合并，不做自动合并，不做文档级撤销栈。冲突发生时只提供「另存为 / 覆盖 / 放弃」三个选项。

**编辑界面由核心提供，插件不贡献 UI。** 这与「插件贡献自定义 UI」被列为非目标（见 2.2）是一致的：插件只能从核心提供的编辑形态中选择其一，选不到合适的形态就只读。

**为什么这样切分：** 插件负责**语义**（怎么序列化、是否合法），核心负责**安全**（原子写、冲突、确认、备份）。于是核心仍零格式知识，而插件完全不接触文件系统写入——权限模型因此可以保持简单。

---

## 6. 数据流

### 6.1 打开文件（GUI 主路径）

1. 用户在 GUI 中选择文件，GUI 调用 `Session::open(source)`。
2. 核心遍历注册表，对每个插件调用 `probe` 打分，取最高分（平手见 7.2）。
3. 若命中外置插件且其进程未启动，按需启动 sidecar（首次约 20–60 ms，之后复用）。
4. 插件返回 `DocHandle` 与 `DocMeta`。
5. 核心取首屏内容，交 `render/dom`，WebView 呈现。
6. 用户滚动触发按需 `page()`，流式追加，不阻塞界面。
7. 用户搜索调用 `Doc::search`，命中结果高亮跳转。

CLI 走同一条路径，只是把第 5 步换成 `render/text` 或 `render/json`。MCP 同理。

### 6.2 会话生命周期

- 会话在最后一个消费者释放后关闭；GUI 中关闭标签页即释放。
- sidecar 进程空闲超过可配置时长（默认 5 分钟）后退出，避免常驻占内存。
- 核心持有探测结果的缓存，键为 `(规范化路径, 修改时间, 大小)`，避免重复探测大文件。

### 6.3 保存流程（仅可编辑插件）

1. 用户触发保存 → 核心调 `EditableDoc::serialize(edited)`，插件校验并产出新字节；插件可返回 `Err` 拒绝保存。
2. 核心比对基线 `(mtime, size, hash)` 与磁盘现状，不一致则阻断并提示冲突。
3. 核心执行原子写（临时文件 → `fsync` → 重命名）与备份。
4. 更新基线哈希，清除 dirty 标记，通知其他消费者刷新。

CLI 通过 `vw save <file> --from <stdin>` 走**完全相同的管线**——它既是脚本化编辑入口，也是保存管线的 conformance 驱动器。

---

## 7. 插件发现与 manifest

### 7.1 插件目录

| 平台 | 路径 |
| --- | --- |
| Windows | `%APPDATA%\vw\plugins\` |
| macOS | `~/Library/Application Support/vw/plugins/` |
| Linux | `~/.local/share/vw/plugins/` |

每个插件占一个子目录，内含 `manifest.json` 与随包二进制。

### 7.2 探测与仲裁

`probe` 返回 0–100 的分数。参考评分区间：

| 依据 | 分值区间 |
| --- | --- |
| 内容魔数（magic）匹配 | 70–95 |
| MIME 精确匹配 | 50–70 |
| 扩展名匹配 | 30–60 |
| 内容嗅探（结构合法但无魔数） | 10–50 |
| 兜底（`probe = 0`，永远匹配） | 0 |

仲裁顺序：分数最高者胜出 → 分数相同则比较 manifest 的 `priority` → 仍相同则报错并列出候选，由用户 `--as` 指定。

**「认不出的文件」不做内核逻辑**，而是由一个 `probe = 0` 的 hex 插件承担。内核只负责在没有任何插件匹配时报错。

### 7.3 manifest 草案

```json
{
  "manifest_version": 1,
  "id": "io.example.pdf",
  "name": "PDF Viewer",
  "version": "0.1.0",
  "protocol": { "contract": 1 },
  "engines": { "vw": ">=0.1.0" },
  "transport": {
    "kind": "stdio-jsonrpc",
    "framing": "jsonl",
    "executable": "bin/{platform}/vw-pdf{ext}"
  },
  "contributes": {
    "viewers": [
      {
        "id": "pdf",
        "extensions": ["pdf"],
        "mime": ["application/pdf"],
        "capabilities": ["view"]
      }
    ]
  },
  "permissions": ["fs.read", "tmp.write"],
  "priority": 50,
  "signature": null
}
```

内置插件使用**完全相同**的 schema，只有 `transport.kind` 取 `in-process`：

```json
{ "transport": { "kind": "in-process" } }
```

`signature` 字段为将来公开生态预留，v1 留空。`executable` 支持 `{platform}` 与 `{ext}` 占位符，取值形如 `win32-x64`、`darwin-arm64`、`linux-x64-gnu`。

`capabilities` 取值 `view` 与 `edit`，未声明 `edit` 即只读。**插件不需要 `fs.write` 权限**——它从不写文件，写由核心在用户确认后执行（见 5.4）。这是把写操作收归核心的直接收益。

---

## 8. 插件分发

采用 **GitHub Releases 为源、无中心 registry** 的模型（D5）。

```
vw plugin add github:owner/repo            # 取最新 release
vw plugin add github:owner/repo@v1.2.3     # 指定 tag
vw plugin add ./local/path                 # 本地开发
vw plugin install ./dist/foo.vwpkg         # 离线包
vw plugin list
vw plugin remove io.example.pdf
```

- **发布** = 在 GitHub 打 tag 并上传 release asset。**资产按平台拆分**，命名 `{id}-{version}-{platform}.vwpkg`（如 `io.example.pdf-0.1.0-win32-x64.vwpkg`），包内为 zip：`manifest.json` 位于包根，附**该平台**的二进制——避免装一个插件就下载全平台冗余，对自带 pdfium 这类重依赖的插件尤其重要。
- **安装** = 经 GitHub API 列出资产并按当前 `{platform}` 选取（无需下载其他平台）→ 下载 → 校验 sha256（随 release 一并发布校验文件）→ 解包至插件目录。
- **版本** = `git tag`；**通道** = GitHub Releases。零服务端、零审核、零运营，插件作者可在自己的仓库独立迭代。
- **镜像** = v1 提供 `VW_PLUGIN_MIRROR` 环境变量做前缀替换；后续可引入多源延迟探测（同时 ping 官方与镜像端点，取最先返回 2xx 者，默认 1500 ms 时限、结果缓存 5 分钟；团队内部项目 DSH 已验证过此做法）。
- **签名** = v1 仅做 sha256 完整性校验；`manifest.signature` 字段已预留，将来启用签名不需要改协议。
- **构建门槛是生态的生死线**：用 Rust SDK 写插件，意味着作者要为多平台产出原生二进制。`vw plugin new` 的模板必须内置 GitHub Actions release workflow（多平台交叉编译 → 上传按平台资产 → 生成 sha256），让「打 tag 即发布」成立，否则插件数量会停留在个位数。

除上述命令之外，v1 不提供搜索与推荐；发现插件依靠 GitHub 自身的检索能力。

---

## 9. 错误处理与超时

分四层隔离，任何一层失败都不得击穿上一层：

| 层级 | 失败情形 | 处理方式 |
| --- | --- | --- |
| 无插件匹配 | 没有插件认领该文件 | 明确报错，列出可安装插件建议。**内核不写兜底解析** |
| 插件进程崩溃 | sidecar 异常退出 | 该文档呈现错误页，GUI 与 CLI 不崩；提供重试入口 |
| 单块失败 | 某一块解析出错 | 以 `Block::Error` 局部呈现，其余内容照常显示 |
| 请求超时 | 插件长时间无响应 | 每请求独立超时，先发 `cancel`，超时后强杀进程并标记需重启 |

统一错误类型：

```rust
pub enum ErrorKind {
    NoViewer,
    PluginCrashed,
    Timeout,
    Cancelled,
    Unsupported,
    MalformedSource,
    PermissionDenied,
    /// 保存前检测到文件已被外部修改
    Conflict,
    /// 插件在校验阶段拒绝保存
    SaveRejected,
    Internal,
}
```

**强约束：**

1. `vw-core` 永不 panic。所有插件调用返回 `Result`，所有 `unwrap` 仅限已证明不可失败的场景。
2. **插件永远可有可无。** 一个插件都没安装时，`vw` 本身仍能启动、能列插件、能给出清晰报错。
3. 超时时间可配置，默认：`probe` 500 ms，`open` 10 s，`page` 5 s。

**诊断与日志。** 插件生态的可调试性与崩溃处理同等重要：

- sidecar 的 stderr 全量捕获，按插件 id 与日期落盘到日志目录并滚动清理；GUI 错误页提供「查看日志」入口。
- 核心日志默认精简，`--verbose` 提升级别；日志不得包含文件内容本身。
- 提供 `vw doctor`：输出插件清单、协议版本、transport 自检结果、最近一次插件崩溃的原因，供用户自查与报告问题。
- **无遥测**：任何诊断数据不出本机，与「轻」和自用定位一致。

---

## 10. 安全与权限

| 项 | v1 做法 | 后续 |
| --- | --- | --- |
| 权限声明 | manifest 的 `permissions` 字段（`fs.read`、`tmp.write`、`net.fetch`），安装时向用户展示。`tmp.write` 特指写核心下发的会话临时目录，不是任意临时路径。**写文件不是插件权限**，因为插件从不写文件 | 增加签名校验与权限变更提示 |
| 源文件访问 | 默认**只读**。只有插件声明 `edit` 能力且用户显式触发保存时，才由**核心**执行写入 | 保存前展示差异供用户确认 |
| 写操作的收归 | 插件的职责止于「产出新的文件字节」；冲突检测、原子写、备份、确认全部由核心执行 | — |
| 进程隔离 | sidecar 为独立进程，崩溃不波及主程序 | 叠加 OS 级沙箱：Windows 用 Job Object / AppContainer，macOS 用 `sandbox-exec`，Linux 用 seccomp 或 bubblewrap |
| 路径校验 | 核心校验所有 `Source` 路径，拒绝越界与符号链接逃逸；**插件返回的 blob 路径**同样校验，必须落在会话临时目录内（见 5.3） | — |
| 完整性 | 安装时校验 sha256 | 启用 `manifest.signature` 签名校验 |

**必须诚实说明的边界：** v1 的权限是**声明式**的——安装时告知用户，但不做 OS 级强制。sidecar 作为普通进程，技术上仍可读取用户有权读取的任意文件。要真正强制隔离需要 OS 沙箱支持，列为后续工作。

---

## 11. 性能与缓存

| 指标 | 目标 |
| --- | --- |
| 内核二进制（不含内置插件） | < 5 MB |
| CLI 冷启动（查看 1 MB 文本文件） | < 50 ms |
| GUI 冷启动到可交互 | < 1.5 s |
| 打开 100 MB 文本文件首屏 | < 300 ms |
| 打开 500 页 PDF 首屏 | < 1 s |
| sidecar 首次启动开销 | < 80 ms |
| 保存 10 MB 文本文件（含冲突检测与原子写） | < 300 ms |

缓存策略：

- **探测结果缓存**：键为 `(规范化路径, mtime, size)`。
- **文档元数据缓存**：会话内有效。
- **缩略图缓存**：GUI 用，落磁盘，容量上限可配置。
- **sidecar 进程池**：同一插件的多个文档复用同一进程；空闲超时回收。

---

## 12. 测试策略

按重要性排序：

1. **conformance 一致性套件（最重要）。** 同一份用例，分别以 `in-process` 与 `stdio-jsonrpc` 两种 transport 加载同一插件，输出必须一致（in-process 侧同样走完整 JSON 序列化往返，见 4.2）。**这是把「架构统一」从设计承诺变成结构保证的手段**，也是 D1 / D2 的可执行定义。
2. **IR 降级测试。** 遍历「渲染器 × 块类型」矩阵，每种组合必须有已定义的输出；新增块类型若未实现降级，测试直接失败；未知块 `kind` 的降级路径同样在矩阵内。以此守住第 5.2 节的四条纪律。
3. **畸形输入语料。** 截断文件、超大文件、编码错乱、嵌套归档与超过总解压上限的归档（zip 炸弹）、声明尺寸与实际不符的图片。目标是既不 panic 也不卡死。
4. **协议健壮性。** 插件返回非法 JSON、字段缺失、超长单行、乱序响应、中途退出、返回会话临时目录之外的 blob 路径（必须被拒绝）。目标是核心给出明确错误而非崩溃。
5. **外壳等价性测试。** 同一文件经 CLI `--json`、MCP 工具调用、GUI IPC 三条路径取到的 IR 必须等价。
6. **插件脚手架自检。** `vw plugin new` 生成的模板必须立即通过 conformance 套件。
7. **编辑往返测试。** 对每个声明 `edit` 能力的插件：`snapshot()` 后立即 `serialize()`，产出的字节必须**逐字节等于原文件**。这条验证「打开再保存不会改动文件」，是编辑功能最重要的安全网（能自动抓出缩进、行尾、BOM、尾随换行被意外规范化的问题）。对无法无损往返的文件（无效 UTF-8、无法映射回原编码的内容），插件必须对该文件降级为只读，此行为纳入同一套用例（见 5.4）。
8. **冲突与失败注入。** 保存前从外部篡改磁盘文件，核心必须阻断；模拟原子写中途失败，原文件必须完好无损。

---

## 13. 分期路线

| 期 | 交付内容 | 理由 |
| --- | --- | --- |
| **P1** | `vw-core` + `vw-protocol` + Document IR + **编辑契约（Text 模式）与保存管线** + 内置插件（text / markdown / json / csv / hex）+ `render/text` 与 `render/json` + **transport 抽象与一个最小 sidecar 示例插件** + CLI 外壳（含 `vw save`）+ conformance 套件 | CLI 是最便宜的外壳，且天然是 conformance 的驱动器。**一期必须同时具备两种 transport**，否则「架构统一」只是纸面承诺，无法被测试验证。保存管线一期做完，GUI 二期才能直接接入而不必回头改内核 |
| **P2** | GUI 外壳（Tauri 2 + 极简前端 + `render/dom`）+ **GUI 编辑器接入（文本编辑与 undo/redo）** + 内置插件 dir 与 archive | 建在已稳定的内核上，避免内核返工拖垮 UI；归档浏览与 GUI 的双击交互配合最自然 |
| **P3** | MCP 外壳 | 复用 Session，工作量很小 |
| **P4** | 插件安装与分发（GitHub Releases）+ 插件 SDK + `vw plugin new` 脚手架（含多平台 release CI 模板，见第 8 章） | 先支持目录式安装，协议已预留签名位 |
| **P5** | 重格式插件：PDF → Office → 媒体 → Parquet / SQLite（PDF 走光栅化优先，`Canvas` 暂缓，见 5.2） | 各自独立仓库独立发布，按需下载 |

**P1 是本规格之后第一份实现计划的范围**，P2–P5 各自需要独立的实现计划与独立的设计确认。

**IR 由最强的渲染目标（GUI）驱动设计，用最弱的消费者（text / json）验证其未过度设计。** 顺序颠倒会得到一个「终端形状」的 IR，做 GUI 时被迫推倒重来。

---

## 14. 技术选型与 crate 划分

### 14.1 选型

| 层 | 选型 | 说明 |
| --- | --- | --- |
| 内核 | Rust（edition 2021+） | 单二进制、启动快、体积可控 |
| GUI | Tauri 2 + 系统 WebView | 不打包 Chromium |
| GUI 前端 | 原生 TypeScript + 少量 CSS | 不引入 Vue / React；「查看器」的界面复杂度不需要框架 |
| CLI | `clap` | 事实标准 |
| IR 序列化 | `serde` + `serde_json` | 与 MCP / GUI 共用同一份序列化 |
| 终端渲染 | 自研 IR → 文本 | 逻辑简单，无需第三方 |

### 14.2 crate 划分

```
vw-protocol       IR 与 RPC 消息的共享定义（core 与插件 SDK 共用）—— 必须独立
vw-core           内核：registry / doc / session / policy / render
vw-plugin-host    sidecar 进程管理、消息编解码
vw-plugin-sdk     插件作者使用的 Rust SDK（实现 sidecar 服务端）
vw-cli            CLI 外壳
vw-mcp            MCP 外壳
vw-app            Tauri GUI 外壳（依赖 vw-core，不得反向依赖）
plugins/text      内置插件：纯文本与代码
plugins/markdown  内置插件：Markdown
plugins/json      内置插件：JSON / JSONL
plugins/csv       内置插件：CSV / TSV
plugins/hex       内置插件：十六进制（probe = 0 兜底）
plugins/dir       内置插件：目录列表（P2）
plugins/archive   内置插件：归档列表与成员预览（P2）
```

`vw-protocol` 独立成 crate 是关键：插件 SDK 不必依赖整个内核，第三方用 Rust 写插件时依赖树很小。

---

## 15. 参考实现：dbx

dbx 是**参考实现**，不是 fork、不是依赖、不是上游。**完整的使用方式见 [以 dbx 作为参考实现的使用方式](../../reference/dbx-as-reference.md)**，其中包含分阶段精读清单、五条移植判据、Apache-2.0 合规流程、版本锚定与红线。

一句话规则：**读它，不 fork 它；移植工程资产，不移植领域代码。** 参考锚点为 dbx `051cab8`（2026-10-13）；阅读一律以锚点为准，不追 `main`。

---

## 16. 术语表

| 术语 | 含义 |
| --- | --- |
| 内核（core） | 不含任何格式知识的部分：注册表、文档模型、会话、策略、渲染器 |
| 外壳（shell） | 面向具体使用方式的薄接口层：GUI、CLI、MCP |
| 插件（plugin） | 提供格式能力的单元，分内置（同进程）与外置（sidecar 子进程） |
| 传输（transport） | 消息的物理通道：内存通道或 stdio JSON-RPC |
| 探测（probe） | 插件对某文件宣告「我能处理」并给出 0–100 的分数 |
| IR | Document IR，插件产出的唯一内容格式 |
| 块（Block） | IR 的基本单元，如 Heading、Table、Image |
| conformance 套件 | 强制两种 transport 行为一致的一致性测试 |
| sidecar | 独立子进程形式的插件 |
| 会话临时目录 | 核心在插件 `open` 时下发的本会话唯一可写目录；blob 与序列化产物必须落在其中，会话结束由核心清理 |
| EditCapability | 插件声明的编辑方式，v1 仅 `None` 与 `Text` 两种取值 |
| EditSnapshot | 插件交给编辑器的可编辑内容快照，v1 仅文本一种形态 |
| 保存管线 | 核心统一实现的 serialize → 冲突检测 → 原子写 → 备份流程，插件只参与其中第一步 |
| 往返一致 | 不变量：`snapshot()` 后原样 `serialize()` 必须逐字节等于原文件 |

---

## 17. 修订记录

任何修改必须递增顶部版本号并在下表追加一行。

| 版本 | 日期 | 说明 |
| --- | --- | --- |
| v1.1 | 2026-10-13 | 按评审意见修订：新增会话临时目录与 blob 路径校验（4.2/5.1/5.3/10/12）；IR 未知块前向兼容降级（5.2/12）；消息尺寸上限与超限走 blob 通道（5.3）；编辑逐文件降级要求（5.4/12）；归档成员打开机制与 zip 炸弹总解压上限（5.1/12）；Canvas 暂缓、PDF 光栅化优先（5.2/13）；保存基线哈希后台计算（5.4）；发布资产按平台拆分、脚手架内置 release CI 模板（8/13）；新增诊断与日志（9）；修复 §13 分期表格断裂；in-process transport 明确真实序列化（4.2/12）；协议示例补充 tmp_dir 与在途取消；术语表新增「会话临时目录」 |
| v1.0 | 2026-10-13 | 初版（通过设计评审） |
