# Rebuilds araso_tsf.dll and puts it where the registered development copy is, without
# registering again (no UAC). A DLL an app has loaded can't be overwritten but can be renamed:
# the old one moves aside (apps that loaded it keep it), and apps that load Araso from now on
# get the new build.
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
Push-Location $root
cargo build --release -p araso-tsf
$code = $LASTEXITCODE
Pop-Location
if ($code) { throw 'build failed' }
$key = 'HKLM:\SOFTWARE\Classes\CLSID\{ae988ccd-6a62-424c-a948-91d4d3b4c13a}\InprocServer32'
$registered = (Get-ItemProperty $key).'(default)'
$dir = Split-Path $registered
# Copies set aside earlier that nothing has open any more go.
Get-ChildItem $dir -Filter '*.old-*' | ForEach-Object { Remove-Item $_.FullName -ErrorAction SilentlyContinue }
if (Test-Path $registered) {
    Move-Item $registered ("{0}.old-{1}" -f $registered, (Get-Date -Format 'HHmmssfff'))
}
Copy-Item (Join-Path $root 'target\release\araso_tsf.dll') $registered
Write-Output "updated $registered"
