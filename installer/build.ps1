$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$Root = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$Manifest = Join-Path $Root 'Cargo.toml'
$ManifestText = Get-Content -LiteralPath $Manifest -Raw
$VersionMatch = [regex]::Match($ManifestText, '(?m)^version\s*=\s*"([^"]+)"')
if (-not $VersionMatch.Success) {
    throw 'Could not read package version from Cargo.toml'
}
$AppVersion = $VersionMatch.Groups[1].Value

$IsccCandidates = @(
    (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 7\ISCC.exe'),
    'C:\Program Files (x86)\Inno Setup 7\ISCC.exe',
    'C:\Program Files\Inno Setup 7\ISCC.exe'
)
$Iscc = $IsccCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if (-not $Iscc) {
    throw 'ISCC.exe not found'
}

$Script = Join-Path $Root 'installer\trueVK.iss'

Push-Location -LiteralPath $Root
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build --release failed with exit code $LASTEXITCODE"
    }

    & $Iscc "/DAppVersion=$AppVersion" $Script
    if ($LASTEXITCODE -ne 0) {
        throw "ISCC failed with exit code $LASTEXITCODE"
    }

    $Installer = Join-Path $Root "installer\Output\trueVK-Setup-$AppVersion.exe"
    if (-not (Test-Path -LiteralPath $Installer)) {
        throw "Installer not found: $Installer"
    }

    $Item = Get-Item -LiteralPath $Installer
    $Hash = (Get-FileHash -LiteralPath $Installer -Algorithm SHA256).Hash
    Write-Output "Installer: $($Item.FullName)"
    Write-Output "Size: $($Item.Length)"
    Write-Output "SHA256: $Hash"
}
finally {
    Pop-Location
}
