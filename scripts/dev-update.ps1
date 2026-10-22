# Rebuilds araso_tsf.dll, 64-bit and 32-bit, and puts each where its registered development copy
# is, without registering again (no UAC). A DLL an app has loaded can't be overwritten but can be
# renamed: the old one moves aside (apps that loaded it keep it), and apps that load Araso from
# now on get the new build. A build whose copy isn't registered (run dev-register.ps1) is skipped.
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
Push-Location $root
cargo build --release -p araso-tsf
$code = $LASTEXITCODE
if (-not $code) {
    cargo build --release -p araso-tsf --target i686-pc-windows-msvc
    $code = $LASTEXITCODE
}
Pop-Location
if ($code) { throw 'build failed' }

$clsid = '{ae988ccd-6a62-424c-a948-91d4d3b4c13a}'
$builds = @(
    @{ Key = "HKLM:\SOFTWARE\Classes\CLSID\$clsid\InprocServer32"; Dll = 'target\release\araso_tsf.dll' },
    @{ Key = "HKLM:\SOFTWARE\WOW6432Node\Classes\CLSID\$clsid\InprocServer32"; Dll = 'target\i686-pc-windows-msvc\release\araso_tsf.dll' }
)
foreach ($build in $builds) {
    $registered = (Get-ItemProperty $build.Key -ErrorAction SilentlyContinue).'(default)'
    if (-not $registered) {
        Write-Output "not registered: $($build.Dll) (run scripts\dev-register.ps1)"
        continue
    }
    $dir = Split-Path $registered
    # Copies set aside earlier that nothing has open any more go.
    Get-ChildItem $dir -Filter '*.old-*' | ForEach-Object { Remove-Item $_.FullName -ErrorAction SilentlyContinue }
    if (Test-Path $registered) {
        Move-Item $registered ("{0}.old-{1}" -f $registered, (Get-Date -Format 'HHmmssfff'))
    }
    Copy-Item (Join-Path $root $build.Dll) $registered
    Write-Output "updated $registered"
}
