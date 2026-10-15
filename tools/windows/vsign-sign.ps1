# 源自 t8y2/dbx @ 270a61c .github/scripts/vsign-sign.ps1
# Apache-2.0。本地修改：调用身份改为用户配置；替换原文件前验证签名和时间戳。
# Changes: caller identity is configurable; validate Authenticode and timestamp
# before replacing the original. See tools/windows/licenses/DBX-LICENSE.txt.
[CmdletBinding()]
param([Parameter(Mandatory = $true, Position = 0)][string]$FilePath)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$resolvedPath = (Resolve-Path -LiteralPath $FilePath).Path
$extension = [IO.Path]::GetExtension($resolvedPath).ToLowerInvariant()
if ($extension -notin @('.exe', '.msi')) { throw "Unsupported signing target: $extension" }
$required = @('SSIGNCODE_OPERATOR_DIR', 'SSIGNCODE_OPERATOR_FILE', 'SSIGNCODE_OPERATOR_PWD', 'SSIGNCODE_OPERATOR_STORE', 'VSIGN_CERT_HASH', 'VSIGN_CLI', 'VSIGN_KEY_PIN', 'VSIGN_SERVER', 'VSIGN_USER')
foreach ($name in $required) {
    if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($name))) { throw "Missing VSign configuration: $name" }
}
$signedPath = Join-Path ([IO.Path]::GetDirectoryName($resolvedPath)) "$([IO.Path]::GetFileNameWithoutExtension($resolvedPath)).vsign-$([Guid]::NewGuid().ToString('N'))$extension"
try {
    & $env:VSIGN_CLI vsign -s $env:VSIGN_SERVER --cert_from csign -u $env:VSIGN_USER --cert_hash $env:VSIGN_CERT_HASH -k $env:VSIGN_KEY_PIN --hash sha256 -i $resolvedPath -o $signedPath
    if ($LASTEXITCODE -ne 0 -or !(Test-Path -LiteralPath $signedPath -PathType Leaf)) { throw 'VSign did not produce a signed artifact.' }
    $signature = Get-AuthenticodeSignature -LiteralPath $signedPath
    if ($signature.Status -ne 'Valid' -or !$signature.TimeStamperCertificate) { throw "Authenticode or timestamp verification failed ($($signature.Status))." }
    Move-Item -LiteralPath $signedPath -Destination $resolvedPath -Force
} finally {
    if (Test-Path -LiteralPath $signedPath) { Remove-Item -LiteralPath $signedPath -Force }
}
