# Builds the release input method (64-bit and 32-bit) and the installer:
# target\installer\Nunchi-Setup-<version>.exe, the version from Cargo.toml.
# Needs Inno Setup 6 (winget install JRSoftware.InnoSetup) and the 32-bit Rust target
# (rustup target add i686-pc-windows-msvc).
param([string]$Iscc)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"

$manifest = Get-Content (Join-Path $root 'Cargo.toml') -Raw
if ($manifest -notmatch '(?m)^version\s*=\s*"([^"]+)"') { throw 'no version in Cargo.toml' }
$version = $Matches[1]

if (-not $Iscc) {
    $Iscc = @(
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $Iscc) { throw 'Inno Setup 6 not found (winget install JRSoftware.InnoSetup)' }

Push-Location $root
try {
    # The release identity: no `dev` feature (that one is for scripts\dev-register.ps1).
    cargo build --release -p nunchi-tsf
    if ($LASTEXITCODE) { throw 'build failed (64-bit)' }
    cargo build --release -p nunchi-tsf --target i686-pc-windows-msvc
    if ($LASTEXITCODE) { throw 'build failed (32-bit)' }
    cargo build --release -p nunchi-settings
    if ($LASTEXITCODE) { throw 'build failed (settings)' }
    # The license notices of the crates these link, for the installer to carry.
    & (Join-Path $root 'tools\crate-licenses.ps1') | Out-Null
    & $Iscc /Qp "/DAppVersion=$version" (Join-Path $root 'installer\nunchi.iss')
    if ($LASTEXITCODE) { throw 'installer build failed' }
} finally {
    Pop-Location
}
Write-Output (Join-Path $root "target\installer\Nunchi-Setup-$version.exe")
