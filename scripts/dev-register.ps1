# Registers a development build, "Araso (개발)", beside the installed Araso: its own class and
# profile (the `dev` feature), so it never touches the installed one. Builds araso_tsf.dll for
# 64-bit and 32-bit apps into target\dev, copies each under a new name (apps keep the DLL they
# loaded open, so the build output can't be registered in place), registers the copies with the
# matching regsvr32, and puts "Araso (개발)" in your keyboard list (Win+Space to choose it).
# Asks for administrator rights (UAC) once: input methods register machine-wide.
#
#   scripts\dev-register.ps1             build, copy, register both, add to the keyboard list
#   scripts\dev-register.ps1 -Unregister take it out of the list and unregister every copy
param([switch]$Unregister, [switch]$NoBuild)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$build = Join-Path $root 'target\dev'
$dir = Join-Path $root 'target\dev-install'
$regsvr64 = Join-Path $env:SystemRoot 'System32\regsvr32.exe'
$regsvr32 = Join-Path $env:SystemRoot 'SysWOW64\regsvr32.exe'
$rundll = Join-Path $env:SystemRoot 'System32\rundll32.exe'

function Invoke-Elevated([string[]]$commands) {
    $script = ($commands | ForEach-Object { "$_; if (`$LASTEXITCODE) { exit `$LASTEXITCODE }" }) -join '; '
    $p = Start-Process powershell.exe -Verb RunAs -Wait -PassThru -ArgumentList '-NoProfile', '-Command', $script
    if ($p.ExitCode -ne 0) { throw "regsvr32 failed: exit $($p.ExitCode)" }
}

if ($Unregister) {
    $copies = @(Get-ChildItem $dir -Filter 'araso_dev-*.dll' -ErrorAction SilentlyContinue)
    if ($copies.Count -eq 0) { Write-Output 'nothing registered'; return }
    $any64 = $copies | Where-Object { $_.Name -notlike 'araso_dev-x86-*' } | Select-Object -First 1
    if ($any64) { & $rundll "$($any64.FullName),RemoveFromKeyboardList" }
    $commands = $copies | ForEach-Object {
        $regsvr = if ($_.Name -like 'araso_dev-x86-*') { $regsvr32 } else { $regsvr64 }
        "& '$regsvr' /s /u '$($_.FullName)'"
    }
    Invoke-Elevated $commands
    Write-Output "unregistered $($copies.Count) copies (files stay until the apps using them quit)"
    return
}

if (-not $NoBuild) {
    $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
    Push-Location $root
    cargo build --release -p araso-tsf --features dev --target-dir $build
    $code = $LASTEXITCODE
    if (-not $code) {
        cargo build --release -p araso-tsf --features dev --target-dir $build --target i686-pc-windows-msvc
        $code = $LASTEXITCODE
    }
    if (-not $code) {
        cargo build --release -p araso-settings --target-dir $build
        $code = $LASTEXITCODE
    }
    Pop-Location
    if ($code) { throw 'build failed (32-bit needs: rustup target add i686-pc-windows-msvc)' }
}

New-Item -ItemType Directory -Force $dir | Out-Null
# Store apps run sandboxed and load only what all app packages may read, as Program Files allows.
icacls $dir /grant '*S-1-15-2-1:(OI)(CI)(RX)' '*S-1-15-2-2:(OI)(CI)(RX)' | Out-Null
# Copies no app has open any more can go.
Get-ChildItem $dir -Filter 'araso_*' | ForEach-Object { Remove-Item $_.FullName -ErrorAction SilentlyContinue }
$stamp = Get-Date -Format 'yyyyMMddHHmmss'
$copy64 = Join-Path $dir "araso_dev-$stamp.dll"
$copy32 = Join-Path $dir "araso_dev-x86-$stamp.dll"
Copy-Item (Join-Path $build 'release\araso_tsf.dll') $copy64
Copy-Item (Join-Path $build 'i686-pc-windows-msvc\release\araso_tsf.dll') $copy32
# The settings window, beside them: the taskbar menu's "설정…" opens it from there.
$settings = Join-Path $dir 'araso-settings.exe'
Get-ChildItem $dir -Filter 'araso-settings.exe.old-*' | ForEach-Object { Remove-Item $_.FullName -ErrorAction SilentlyContinue }
if (Test-Path $settings) { Move-Item $settings ("{0}.old-{1}" -f $settings, (Get-Date -Format 'HHmmssfff')) }
Copy-Item (Join-Path $build 'release\araso-settings.exe') $settings
Invoke-Elevated @("& '$regsvr64' /s '$copy64'", "& '$regsvr32' /s '$copy32'")
# The keyboard list is yours: changed as you, not as the administrator.
& $rundll "$copy64,AddToKeyboardList"
Write-Output "registered $copy64"
Write-Output "registered $copy32"
Write-Output 'Win+Space: "Araso (개발)" is in the list beside the installed Araso.'
