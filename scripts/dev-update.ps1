# Rebuilds the development build, "Araso (개발)", 64-bit and 32-bit, and puts each where its
# registered copy is, without registering again (no UAC). A DLL an app has loaded can't be
# overwritten but can be renamed: the old one moves aside (apps that loaded it keep it), and apps
# that load Araso (개발) from now on get the new build. Run scripts\dev-register.ps1 once first.
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$build = Join-Path $root 'target\dev'
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
Push-Location $root
cargo build --release -p araso-tsf --features dev --target-dir $build
$code = $LASTEXITCODE
if (-not $code) {
    cargo build --release -p araso-tsf --features dev --target-dir $build --target i686-pc-windows-msvc
    $code = $LASTEXITCODE
}
Pop-Location
if ($code) { throw 'build failed' }

# The `dev` feature's class (crates\araso-tsf\src\lib.rs).
$clsid = '{aeca5114-3633-4707-bc7d-67ca80de068c}'
$builds = @(
    @{ Key = "HKLM:\SOFTWARE\Classes\CLSID\$clsid\InprocServer32"; Dll = 'release\araso_tsf.dll' },
    @{ Key = "HKLM:\SOFTWARE\WOW6432Node\Classes\CLSID\$clsid\InprocServer32"; Dll = 'i686-pc-windows-msvc\release\araso_tsf.dll' }
)
foreach ($b in $builds) {
    $registered = (Get-ItemProperty $b.Key -ErrorAction SilentlyContinue).'(default)'
    if (-not $registered) {
        Write-Output "not registered: $($b.Dll) (run scripts\dev-register.ps1)"
        continue
    }
    $dir = Split-Path $registered
    # Copies set aside earlier that nothing has open any more go.
    Get-ChildItem $dir -Filter '*.old-*' | ForEach-Object { Remove-Item $_.FullName -ErrorAction SilentlyContinue }
    if (Test-Path $registered) {
        Move-Item $registered ("{0}.old-{1}" -f $registered, (Get-Date -Format 'HHmmssfff'))
    }
    Copy-Item (Join-Path $build $b.Dll) $registered
    Write-Output "updated $registered"
}
