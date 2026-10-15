# SignPath 云签名（草稿，待 SignPath Foundation 批准后启用）。
# 与 vsign-sign.ps1 相同契约：提交签名请求，验证 Authenticode 与时间戳后原子替换原文件。
# 获批后按 docs.signpath.io 当前文档校准参数名；组织/项目/策略 ID 通过环境变量注入。
[CmdletBinding()]
param([Parameter(Mandatory = $true, Position = 0)][string]$FilePath)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$resolvedPath = (Resolve-Path -LiteralPath $FilePath).Path
$extension = [IO.Path]::GetExtension($resolvedPath).ToLowerInvariant()
if ($extension -notin @('.exe', '.msi')) { throw "Unsupported signing target: $extension" }
$required = @('SIGNPATH_API_TOKEN', 'SIGNPATH_ORGANIZATION_ID', 'SIGNPATH_PROJECT_ID', 'SIGNPATH_SIGNING_POLICY_ID')
foreach ($name in $required) {
    if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($name))) { throw "Missing SignPath configuration: $name" }
}
$signedPath = Join-Path ([IO.Path]::GetDirectoryName($resolvedPath)) "$([IO.Path]::GetFileNameWithoutExtension($resolvedPath)).signpath-$([Guid]::NewGuid().ToString('N'))$extension"
try {
    if (!(Get-Module -ListAvailable -Name SignPath)) { Install-Module -Name SignPath -Force -Scope CurrentUser }
    Import-Module SignPath
    Submit-SigningRequest `
        -ApiToken $env:SIGNPATH_API_TOKEN `
        -Url 'https://app.signpath.io' `
        -OrganizationId $env:SIGNPATH_ORGANIZATION_ID `
        -ProjectId $env:SIGNPATH_PROJECT_ID `
        -SigningPolicyId $env:SIGNPATH_SIGNING_POLICY_ID `
        -InputArtifactPath $resolvedPath `
        -WaitForCompletion `
        -OutputArtifactPath $signedPath
    if (!(Test-Path -LiteralPath $signedPath -PathType Leaf)) { throw 'SignPath did not produce a signed artifact.' }
    $signature = Get-AuthenticodeSignature -LiteralPath $signedPath
    if ($signature.Status -ne 'Valid' -or !$signature.TimeStamperCertificate) { throw "Authenticode or timestamp verification failed ($($signature.Status))." }
    Move-Item -LiteralPath $signedPath -Destination $resolvedPath -Force
} finally {
    if (Test-Path -LiteralPath $signedPath) { Remove-Item -LiteralPath $signedPath -Force }
}
