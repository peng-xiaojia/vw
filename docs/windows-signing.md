# Windows 签名

本机检查（2026-10-14）：DSH 和 DBX 的 EXE 均为有效 Authenticode 签名。DSH 签署者是杭州深度求索人工智能有限公司，颁发者为 GlobalSign EV CodeSigning CA；DBX 签署者为武汉市洪山区宇界网络工作室（个体工商户），颁发者为 GlobalSign CodeSigning CA。两者都有 DigiCert 时间戳。当前 vw 未签名，本机证书库没有可用的代码签名私钥。

DBX 当前公开发布流程使用 [TrustAsia VSign](https://github.com/t8y2/dbx/blob/main/.github/scripts/vsign-sign.ps1)；安装的旧版本使用的证书与当前流程可以不同。签名需要自己的证书及私钥，其他 EXE 内嵌的公开证书不能用于给 vw 签名。正式证书需要向 CA 或云签名服务申请并完成身份审核。自签证书不等于公开信任，也不能保证消除 SmartScreen 提示；正式签名后信誉仍由 Windows 判断。

## 本机证书

将代码签名证书及私钥配置在 CurrentUser/My（私钥通常由硬件或服务保存），再运行：

```powershell
pwsh -NoProfile -File tools/windows/build-release.ps1 -CertificateThumbprint '<自己的证书指纹>' -RequireSigning
```

脚本检查私钥可用、有效期和 Code Signing EKU，Tauri 使用 signtool 对 EXE/NSIS 签名，SHA-256 和时间戳完成后再次验证产物。

## VSign 云签名

`tools/windows/vsign-sign.ps1` 改编自 DBX 的 Apache-2.0 实现。先按服务商说明安装官方 CLI 和操作员凭据，配置以下环境变量：

| 变量 | 用途 |
| --- | --- |
| VSIGN_CLI | 官方签名工具的本机路径 |
| VSIGN_SERVER | 云签名服务地址 |
| VSIGN_USER | 自己的签名调用身份 |
| VSIGN_CERT_HASH | 自己的代码签名证书标识 |
| VSIGN_KEY_PIN | 私钥授权 PIN |
| SSIGNCODE_OPERATOR_DIR / FILE / STORE | 操作员凭据配置，取值遵循服务商说明 |
| SSIGNCODE_OPERATOR_PWD | 操作员凭据密码 |

密码与 PIN 使用本机临时环境或 CI secret 注入，不能提交到仓库或聊天。然后运行：

```powershell
pwsh -NoProfile -File tools/windows/build-release.ps1 -UseVSign -RequireSigning
```

缺少配置会在构建前退出。签名脚本只在有效 Authenticode 和时间戳验证通过后替换原文件；发布脚本在复制和生成哈希前再次验证。此模式尚未实际签名验收，因为用户还未申请账号/证书。

不配置签名参数时生成未签名版本，manifest.json 如实记录 NotSigned。SHA-256 是完整性校验，不是 Authenticode；插件 manifest.signature 也不是 Windows 程序签名。

## 免费分发路径与限制（2026-10-14 核查）

- **Microsoft Store + MSIX**：微软对通过商店提交的 MSIX 免费重签，用户从商店安装时不遇到 SmartScreen 下载提示。现有 NSIS EXE 不能直接享用这项重签；要另做 MSIX 包、验证文件关联与插件目录在包隔离下的行为，并通过商店审核。[微软签名选项](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options)、[Tauri 的 MSIX 打包指南](https://learn.microsoft.com/en-us/windows/apps/dev-tools/winapp-cli/guides/tauri)
- **SignPath Foundation**：对符合条件的开源项目免费提供代码签名，证书显示 SignPath Foundation，而不是项目作者。项目须公开发布、使用合格开源许可证、建立可核验的源码到二进制构建流程，并由维护者批准每次签名。当前本地仓库没有 Git remote，尚无法提交公开仓库与自动构建证据；申请是否获批由 SignPath 判断。[资格条款](https://signpath.org/terms.html)、[申请入口](https://signpath.org/apply.html)
- **自签名**：本机开发和受管理设备可用，但对普通下载用户仍会显示风险提示。不能通过删掉文件下载标记或关闭 SmartScreen 代替公开可信签名。
- **公开发行的个人签名**：Microsoft Artifact Signing Basic 约 $9.99/月，且个人身份地域限制当前为美国、加拿大；传统 OV 证书通常按年付费。签名建立发布者身份，SmartScreen 信誉仍需积累；EV 证书也不再保证首发免提示。[微软签名选项](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options)、[SmartScreen 信誉说明](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation)
