[CmdletBinding()]
param(
    [string]$OutputDirectory = "",
    [string]$CertificateThumbprint = "",
    [string]$TimestampUrl = "http://timestamp.digicert.com",
    [switch]$RequireSigning,
    [switch]$UseVSign,
    [switch]$UseSignPath,
    [switch]$SkipInstaller
)
$ErrorActionPreference = 'Stop'
# Windows PowerShell 5.1 has no utf8NoBOM Set-Content encoding; write bytes directly.
function Write-Utf8NoBom([string]$Path, [string]$Text) {
    [IO.File]::WriteAllText($Path, $Text, [Text.UTF8Encoding]::new($false))
}
$repo = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
if (!$OutputDirectory) { $OutputDirectory = Join-Path $repo 'dist/windows' }
$output = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Path $output -Force | Out-Null
$signingMethods = @($UseVSign, $UseSignPath) | Where-Object { $_ }
if ($RequireSigning -and !$CertificateThumbprint -and !$UseVSign -and !$UseSignPath) { throw 'Signing is required: supply -CertificateThumbprint, -UseVSign or -UseSignPath.' }
if (($CertificateThumbprint -and $signingMethods.Count -gt 0) -or $signingMethods.Count -gt 1) { throw 'Choose only one signing method.' }
$windows = @{}
if ($UseVSign) {
    foreach ($name in @('SSIGNCODE_OPERATOR_DIR','SSIGNCODE_OPERATOR_FILE','SSIGNCODE_OPERATOR_PWD','SSIGNCODE_OPERATOR_STORE','VSIGN_CERT_HASH','VSIGN_CLI','VSIGN_KEY_PIN','VSIGN_SERVER','VSIGN_USER')) {
        if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($name))) { throw "Missing VSign configuration: $name" }
    }
    $windows.signCommand = @{ cmd = 'pwsh'; args = @('-NoProfile','-NonInteractive','-File',(Join-Path $PSScriptRoot 'vsign-sign.ps1'),'%1') }
}
if ($UseSignPath) {
    foreach ($name in @('SIGNPATH_API_TOKEN','SIGNPATH_ORGANIZATION_ID','SIGNPATH_PROJECT_ID','SIGNPATH_SIGNING_POLICY_ID')) {
        if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($name))) { throw "Missing SignPath configuration: $name" }
    }
    $windows.signCommand = @{ cmd = 'powershell'; args = @('-NoProfile','-NonInteractive','-File',(Join-Path $PSScriptRoot 'signpath-sign.ps1'),'%1') }
}
if ($CertificateThumbprint) {
    # Tauri's thumbprint signer uses signtool's CurrentUser/My store.
    # Only public certificate metadata is inspected; private key material is never exported.
    $cert = Get-ChildItem Cert:\CurrentUser\My | Where-Object { $_.Thumbprint -eq $CertificateThumbprint -and $_.HasPrivateKey } | Select-Object -First 1
    if (!$cert) { throw 'The requested certificate or its private key is unavailable in CurrentUser/My.' }
    if ($cert.NotAfter -le (Get-Date)) { throw 'The requested certificate has expired.' }
    if (!($cert.EnhancedKeyUsageList.ObjectId -contains '1.3.6.1.5.5.7.3.3')) { throw 'The requested certificate does not allow code signing.' }
    if (!$TimestampUrl) { throw 'A timestamp URL is required when signing.' }
    $windows.certificateThumbprint = $CertificateThumbprint
    $windows.digestAlgorithm = 'sha256'
    $windows.timestampUrl = $TimestampUrl
}
$config = Join-Path $output 'release-config.json'
Write-Utf8NoBom $config (@{ bundle = @{ windows = $windows } } | ConvertTo-Json -Depth 5)
Push-Location (Join-Path $repo 'app')
try {
    # Tauri CLI also downloads its NSIS and WebView2 bootstrapper dependencies.
    $cli = Get-Command cargo-tauri -ErrorAction SilentlyContinue
    $buildArgs = @('build', '--config', $config)
    if ($SkipInstaller) { $buildArgs += '--no-bundle' } else { $buildArgs += @('--bundles', 'nsis') }
    if ($cli) { & cargo tauri @buildArgs } else { & npm.cmd exec --yes --package '@tauri-apps/cli@2.12.2' -- tauri @buildArgs }
    if ($LASTEXITCODE -ne 0) { throw "Tauri release build failed ($LASTEXITCODE)." }
} finally { Pop-Location }
$target = if ($env:CARGO_TARGET_DIR) { [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR) } else { Join-Path $repo 'target' }
$exe = Join-Path $target 'release/vw-app.exe'
if (!(Test-Path -LiteralPath $exe)) { throw "Release executable not found: $exe" }
if ($SkipInstaller -and $UseVSign) {
    & (Join-Path $PSScriptRoot 'vsign-sign.ps1') -FilePath $exe
} elseif ($SkipInstaller -and $UseSignPath) {
    & powershell -NoProfile -NonInteractive -File (Join-Path $PSScriptRoot 'signpath-sign.ps1') -FilePath $exe
} elseif ($SkipInstaller -and $CertificateThumbprint) {
    # --no-bundle bypasses Tauri's signer, so sign the portable EXE explicitly.
    $signTool = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($signTool) { $signToolPath = $signTool.Source } else {
        $sdkTool = Get-ChildItem -Path "${env:ProgramFiles(x86)}/Windows Kits/10/bin/*/x64/signtool.exe" -ErrorAction SilentlyContinue | Sort-Object FullName -Descending | Select-Object -First 1
        if (!$sdkTool) { throw 'Windows SDK signtool.exe is required for portable signing.' }
        $signToolPath = $sdkTool.FullName
    }
    & $signToolPath sign /fd sha256 /sha1 $CertificateThumbprint /tr $TimestampUrl /td sha256 $exe
    if ($LASTEXITCODE -ne 0) { throw 'Portable executable signing failed.' }
}
function Assert-GuiExecutable([string]$Path) {
    $bytes = [IO.File]::ReadAllBytes($Path)
    if ($bytes.Length -lt 256 -or $bytes[0] -ne 0x4d -or $bytes[1] -ne 0x5a) { throw "Invalid PE executable: $Path" }
    $pe = [BitConverter]::ToInt32($bytes, 0x3c)
    if ([BitConverter]::ToUInt16($bytes, $pe + 24 + 68) -ne 2) { throw "Executable has a console subsystem: $Path" }
}
function Signature-Info([string]$Path) {
    $signature = Get-AuthenticodeSignature -LiteralPath $Path
    if (($CertificateThumbprint -or $UseVSign -or $UseSignPath) -and ($signature.Status -ne 'Valid' -or ($CertificateThumbprint -and $signature.SignerCertificate.Thumbprint -ne $CertificateThumbprint) -or !$signature.TimeStamperCertificate)) {
        throw "Signature verification failed for $Path ($($signature.Status))."
    }
    return @{ status = "$($signature.Status)"; signer = $signature.SignerCertificate.Subject; thumbprint = $signature.SignerCertificate.Thumbprint; timestamp = $signature.TimeStamperCertificate.Subject }
}
Assert-GuiExecutable $exe
$portable = Join-Path $output 'portable'
New-Item -ItemType Directory -Path $portable -Force | Out-Null
Copy-Item -LiteralPath $exe -Destination (Join-Path $portable 'vw-app.exe') -Force
$licenses = Join-Path $portable 'licenses'
New-Item -ItemType Directory -Path $licenses -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $repo 'app/ui/icons/LICENSE.txt') -Destination (Join-Path $licenses 'lucide-LICENSE.txt') -Force
Copy-Item -LiteralPath (Join-Path $repo 'app/ui/vendor/codemirror/LICENSE') -Destination (Join-Path $licenses 'CodeMirror-LICENSE.txt') -Force
Copy-Item -LiteralPath (Join-Path $repo 'app/ui/vendor/prettier/LICENSE') -Destination (Join-Path $licenses 'Prettier-LICENSE.txt') -Force
Get-ChildItem -LiteralPath (Join-Path $repo 'docs/licenses') -File | ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $licenses $_.Name) -Force }
Copy-Item -LiteralPath (Join-Path $repo 'docs/THIRD-PARTY-NOTICES.md') -Destination (Join-Path $licenses 'THIRD-PARTY-NOTICES.md') -Force
Set-Content -LiteralPath (Join-Path $portable 'vw-portable.marker') -Value 'Portable mode: WebView2 data goes beside this executable if writable.' -Encoding Ascii
Write-Utf8NoBom (Join-Path $portable 'README.txt') 'Extract the ZIP into a normal user folder (for example Downloads) before running vw-app.exe. A restricted development workspace can attach a Low integrity label to executables, preventing Windows file-association registration. Microsoft Edge WebView2 Runtime is required. The NSIS installer can provision it. VW_WEBVIEW_DATA overrides the data directory. Unsigned builds may prompt SmartScreen; signing does not guarantee reputation.'
$zip = Join-Path $output 'vw-windows-portable.zip'
# Package only release assets; existing portable WebView data stays on disk.
Compress-Archive -Path @((Join-Path $portable 'vw-app.exe'), (Join-Path $portable 'vw-portable.marker'), (Join-Path $portable 'README.txt'), $licenses) -DestinationPath $zip -Force
$artifacts = @(@{ name = 'vw-app.exe'; path = Join-Path $portable 'vw-app.exe'; signature = (Signature-Info $exe) }, @{ name = 'vw-windows-portable.zip'; path = $zip })
if (!$SkipInstaller) {
    $installer = Get-ChildItem -LiteralPath (Join-Path $target 'release/bundle/nsis') -Filter '*.exe' | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (!$installer) { throw 'Tauri did not produce an NSIS installer.' }
    $installerPath = Join-Path $output $installer.Name
    Copy-Item -LiteralPath $installer.FullName -Destination $installerPath -Force
    Assert-GuiExecutable $installerPath
    $artifacts += @{ name = $installer.Name; path = $installerPath; signature = (Signature-Info $installerPath) }
}
foreach ($artifact in $artifacts) {
    $artifact.sha256 = (Get-FileHash -LiteralPath $artifact.path -Algorithm SHA256).Hash.ToLowerInvariant()
    $artifact.bytes = (Get-Item -LiteralPath $artifact.path).Length
}
$sourceRevision = (& git -C $repo rev-parse HEAD).Trim()
$sourceDirty = [bool](& git -C $repo status --porcelain)
$lockHash = (Get-FileHash -LiteralPath (Join-Path $repo 'Cargo.lock') -Algorithm SHA256).Hash.ToLowerInvariant()
$manifest = @{ generatedUtc = [DateTime]::UtcNow.ToString('o'); architecture = 'x86_64'; sourceRevision = $sourceRevision; sourceDirty = $sourceDirty; cargoLockSha256 = $lockHash; signingRequired = [bool]$RequireSigning; artifacts = $artifacts } | ConvertTo-Json -Depth 7
Write-Utf8NoBom (Join-Path $output 'manifest.json') $manifest
Write-Utf8NoBom (Join-Path $output 'SHA256SUMS.txt') ((($artifacts | ForEach-Object { "$($_.sha256)  $($_.name)" }) -join "`r`n") + "`r`n")
Write-Output "Windows artifacts verified: $output"

