# SignPath Foundation 申请材料（草稿）

日期：2026-10-15。目标：用 SignPath Foundation 的免费开源签名给 vw 的 Windows 产物签名。
证书显示的发布者将是 "SignPath Foundation"，不是个人或公司名称。

## 1. 资格对照（对照 [资格条款](https://signpath.org/terms.html)）

| 要求 | 现状 |
| --- | --- |
| 公开发布的开源项目 | 已满足：https://github.com/peng-xiaojia/vw 公开仓库 |
| 合格开源许可证 | 已满足：Apache-2.0（根目录 LICENSE，Cargo 工作区声明一致） |
| 公开的源码仓库 | 已满足：GitHub，单一 main 分支 |
| 可核验的源码到二进制构建流程 | 已满足：`.github/workflows/windows-release.yml` 在 `v*` 标签触发，GitHub Actions 构建 Windows 便携包与 NSIS 安装包，生成 manifest（记录提交哈希、Cargo.lock 摘要、dirty 状态）与 SHA256SUMS |
| 每次签名由维护者批准 | 获批后在 SignPath 后台配置签名策略（默认策略已要求提交者身份） |
| 第三方组件合规 | 已满足：`docs/THIRD-PARTY-NOTICES.md` 登记来源与许可证 |

## 2. 待人工完成（需要仓库所有者身份）

> **状态更新（2026-10-15）：申请已提交。** 表单于当日通过 HubSpot 表单提交（申请内容与下述草稿一致，维护者按 GitHub 账号登记为 Xiaojia Peng，通知邮箱 jjiaper@qq.com），页面返回 "Form submitted"。等待 SignPath Foundation 人工审核结果。

1. 访问 [申请入口](https://signpath.org/apply.html)，用能代表项目的 GitHub 账号提交，同意条款。
2. 申请表可参考以下内容填写：
   - **Project name**：vw
   - **Public repository**：https://github.com/peng-xiaojia/vw
   - **License**：Apache-2.0
   - **Description**：vw is a plugin-based local file viewer (Tauri 2 desktop app, CLI and MCP frontends). All format support is provided by plugins that produce a shared Document IR. Windows artifacts are built by GitHub Actions on tag push.
   - **Artifact verification**：`.github/workflows/windows-release.yml` builds the portable EXE and NSIS installer from the tagged commit; manifest.json records the source revision, Cargo.lock digest and artifact SHA-256 hashes.
   - **Maintainer contact**：填写你自己的姓名与邮箱。
   - **Signing users**：先只填仓库所有者。
3. 等待审核（人工判定，周期由 SignPath 决定；被拒不产生费用）。

## 3. 获批后的接线步骤

1. 在 SignPath 后台创建项目 `vw`，记录 Organization ID、Project ID、Signing Policy ID。
2. 按其指引安装 **SignPath GitHub App** 并绑定 `peng-xiaojia/vw`（来源验证：签名请求须来自 GitHub Actions 构建）。
3. 在仓库 Settings → Secrets and variables → Actions 添加：
   `SIGNPATH_API_TOKEN`、`SIGNPATH_ORGANIZATION_ID`、`SIGNPATH_PROJECT_ID`、`SIGNPATH_SIGNING_POLICY_ID`。
4. 推一个 `v*` 标签触发流水线；工作流检测到 `SIGNPATH_API_TOKEN` 后自动加 `-UseSignPath -RequireSigning`，
   由 `tools/windows/signpath-sign.ps1` 提交签名请求并在验证 Authenticode 与时间戳后原子替换文件。
5. 核对 Release 中 manifest.json 的 `artifacts[].signature`，应为 Valid 且带时间戳。
6. `tools/windows/signpath-sign.ps1` 是按 SignPath PowerShell 模块写的草稿；获批后按 [docs.signpath.io](https://docs.signpath.io) 当前文档核对参数名与轮询行为再启用。

## 4. 限制说明

- 审核是否批准由 SignPath Foundation 判定，本文档不构成承诺。
- 即使获批，SmartScreen 首次仍可能提示：签名建立发布者身份，信誉需随分发量积累。
- 获批前所有发布产物为未签名版本（manifest 记录 NotSigned）。
