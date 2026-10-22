# Registers the release builds for development, 64-bit and 32-bit (32-bit apps such as KakaoTalk
# load only a 32-bit input method): builds araso_tsf.dll for both, copies each under a new name
# (apps keep the DLL they loaded open, so the build output can't be registered in place), and
# registers the copies with the matching regsvr32. The word lists are built into the DLL.
# Asks for administrator rights (UAC): input methods register machine-wide.
#
#   scripts\dev-register.ps1             build, copy, register both
#   scripts\dev-register.ps1 -Unregister unregister every development copy
param([switch]$Unregister, [switch]$NoBuild)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$dir = Join-Path $root 'target\dev-install'
$regsvr64 = Join-Path $env:SystemRoot 'System32\regsvr32.exe'
$regsvr32 = Join-Path $env:SystemRoot 'SysWOW64\regsvr32.exe'

function Invoke-Elevated([string[]]$commands) {
    $script = ($commands | ForEach-Object { "$_; if (`$LASTEXITCODE) { exit `$LASTEXITCODE }" }) -join '; '
    $p = Start-Process powershell.exe -Verb RunAs -Wait -PassThru -ArgumentList '-NoProfile', '-Command', $script
    if ($p.ExitCode -ne 0) { throw "regsvr32 failed: exit $($p.ExitCode)" }
}

if ($Unregister) {
    $commands = @()
    Get-ChildItem $dir -Filter 'araso_tsf-*.dll' -ErrorAction SilentlyContinue | ForEach-Object {
        $regsvr = if ($_.Name -like 'araso_tsf-x86-*') { $regsvr32 } else { $regsvr64 }
        $commands += "& '$regsvr' /s /u '$($_.FullName)'"
    }
    if ($commands.Count -eq 0) { Write-Output 'nothing registered'; return }
    Invoke-Elevated $commands
    Write-Output "unregistered $($commands.Count) copies (files stay until the apps using them quit)"
    return
}

if (-not $NoBuild) {
    $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
    Push-Location $root
    cargo build --release -p araso-tsf
    $code = $LASTEXITCODE
    if (-not $code) {
        cargo build --release -p araso-tsf --target i686-pc-windows-msvc
        $code = $LASTEXITCODE
    }
    Pop-Location
    if ($code) { throw 'build failed (32-bit needs: rustup target add i686-pc-windows-msvc)' }
}

New-Item -ItemType Directory -Force $dir | Out-Null
# Copies no app has open any more can go.
Get-ChildItem $dir -Filter 'araso_tsf-*' | ForEach-Object { Remove-Item $_.FullName -ErrorAction SilentlyContinue }
$stamp = Get-Date -Format 'yyyyMMddHHmmss'
$copy64 = Join-Path $dir "araso_tsf-$stamp.dll"
$copy32 = Join-Path $dir "araso_tsf-x86-$stamp.dll"
Copy-Item (Join-Path $root 'target\release\araso_tsf.dll') $copy64
Copy-Item (Join-Path $root 'target\i686-pc-windows-msvc\release\araso_tsf.dll') $copy32
Invoke-Elevated @("& '$regsvr64' /s '$copy64'", "& '$regsvr32' /s '$copy32'")
Write-Output "registered $copy64"
Write-Output "registered $copy32"
Write-Output 'Add it once: Settings > Time & language > Language & region > Korean > Language options > Add a keyboard > Araso'
