# Registers the release build for development: builds araso_tsf.dll, copies it under a new name
# (apps keep the DLL they loaded open, so the build output can't be registered in place), and
# registers that copy. Data files are found in the source tree, so nothing else is copied.
# Asks for administrator rights (UAC): input methods register machine-wide.
#
#   scripts\dev-register.ps1             build, copy, register
#   scripts\dev-register.ps1 -Unregister unregister every development copy
param([switch]$Unregister, [switch]$NoBuild)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$dir = Join-Path $root 'target\dev-install'

function Invoke-Elevated([string[]]$dlls, [string]$verb) {
    $commands = foreach ($dll in $dlls) { "& regsvr32.exe /s $verb `"$dll`"; if (`$LASTEXITCODE) { exit `$LASTEXITCODE }" }
    $script = ($commands -join '; ')
    $p = Start-Process powershell.exe -Verb RunAs -Wait -PassThru -ArgumentList '-NoProfile', '-Command', $script
    if ($p.ExitCode -ne 0) { throw "regsvr32 failed: exit $($p.ExitCode)" }
}

if ($Unregister) {
    $dlls = @(Get-ChildItem $dir -Filter 'araso_tsf-*.dll' -ErrorAction SilentlyContinue | ForEach-Object FullName)
    if ($dlls.Count -eq 0) { Write-Output 'nothing registered'; return }
    Invoke-Elevated $dlls '/u'
    Write-Output "unregistered $($dlls.Count) copies (files stay until the apps using them quit)"
    return
}

if (-not $NoBuild) {
    $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
    Push-Location $root
    cargo build --release -p araso-tsf
    $code = $LASTEXITCODE
    Pop-Location
    if ($code) { throw 'build failed' }
}

New-Item -ItemType Directory -Force $dir | Out-Null
# Copies no app has open any more can go.
Get-ChildItem $dir -Filter 'araso_tsf-*.dll' | ForEach-Object { Remove-Item $_.FullName -ErrorAction SilentlyContinue }
$copy = Join-Path $dir ("araso_tsf-{0}.dll" -f (Get-Date -Format 'yyyyMMddHHmmss'))
Copy-Item (Join-Path $root 'target\release\araso_tsf.dll') $copy
Invoke-Elevated @($copy) ''
Write-Output "registered $copy"
Write-Output 'Add it once: Settings > Time & language > Language & region > Korean > Language options > Add a keyboard > Araso'
