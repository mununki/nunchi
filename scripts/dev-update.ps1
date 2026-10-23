# Rebuilds the development build, "눈치 (개발)", 64-bit and 32-bit, and puts each where its
# registered copy is, without registering again (no UAC). A DLL an app has loaded can't be
# overwritten but can be renamed: the old one moves aside (apps that loaded it keep it), and apps
# that load "눈치 (개발)" from now on get the new build. The settings window goes beside them.
# Run scripts\dev-register.ps1 once first.
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$build = Join-Path $root 'target\dev'
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
Push-Location $root
cargo build --release -p nunchi-tsf --features dev --target-dir $build
$code = $LASTEXITCODE
if (-not $code) {
    cargo build --release -p nunchi-tsf --features dev --target-dir $build --target i686-pc-windows-msvc
    $code = $LASTEXITCODE
}
if (-not $code) {
    cargo build --release -p nunchi-settings --target-dir $build
    $code = $LASTEXITCODE
}
Pop-Location
if ($code) { throw 'build failed' }

# The `dev` feature's class (crates\nunchi-tsf\src\lib.rs).
$clsid = '{aeca5114-3633-4707-bc7d-67ca80de068c}'
$builds = @(
    @{ Key = "HKLM:\SOFTWARE\Classes\CLSID\$clsid\InprocServer32"; Dll = 'release\nunchi_tsf.dll' },
    @{ Key = "HKLM:\SOFTWARE\WOW6432Node\Classes\CLSID\$clsid\InprocServer32"; Dll = 'i686-pc-windows-msvc\release\nunchi_tsf.dll' }
)
foreach ($b in $builds) {
    $registered = (Get-ItemProperty $b.Key -ErrorAction SilentlyContinue).'(default)'
    if (-not $registered) {
        Write-Output "not registered: $($b.Dll) (run scripts\dev-register.ps1)"
        continue
    }
    $dir = Split-Path $registered
    # Readable by Store apps (sandboxed) too, as dev-register.ps1 makes it.
    icacls $dir /grant '*S-1-15-2-1:(OI)(CI)(RX)' '*S-1-15-2-2:(OI)(CI)(RX)' | Out-Null
    # Copies set aside earlier that nothing has open any more go.
    Get-ChildItem $dir -Filter '*.old-*' | ForEach-Object { Remove-Item $_.FullName -ErrorAction SilentlyContinue }
    if (Test-Path $registered) {
        Move-Item $registered ("{0}.old-{1}" -f $registered, (Get-Date -Format 'HHmmssfff'))
    }
    Copy-Item (Join-Path $build $b.Dll) $registered
    Write-Output "updated $registered"
}

# The settings window, where the taskbar menu's "설정…" looks for it: beside the input method.
$registered = (Get-ItemProperty $builds[0].Key -ErrorAction SilentlyContinue).'(default)'
if ($registered) {
    $settings = Join-Path (Split-Path $registered) 'nunchi-settings.exe'
    if (Test-Path $settings) {
        Move-Item $settings ("{0}.old-{1}" -f $settings, (Get-Date -Format 'HHmmssfff'))
    }
    Copy-Item (Join-Path $build 'release\nunchi-settings.exe') $settings
    Write-Output "updated $settings"
}
