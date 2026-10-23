# Types every case in crates\araso-typing-test\cases with real keys into each kind of text field,
# in 64-bit and 32-bit apps and inside the sandbox Store apps run in: the regression check to run
# before a commit. Ends with a summary, and exit code 1 when anything failed.
#
# It types real keys for about a minute: keep the keyboard on this PC (a network KVM would send
# them to another machine) and leave it alone until it ends.
#
#   scripts\typing-tests.ps1             the development build, rebuilt and put in place first
#   scripts\typing-tests.ps1 -Installed  the Araso registered under the release class (installed)
#   scripts\typing-tests.ps1 -Browser    Chrome (or Edge) too, typing with the input method chosen there
#   scripts\typing-tests.ps1 -NoSandbox  not inside the sandbox (its DLL isn't where Store apps can read)
#   scripts\typing-tests.ps1 -Yes        no question first (CI)
param([switch]$Installed, [switch]$Browser, [switch]$NoSandbox, [switch]$NoBuild, [switch]$Yes)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"

if (-not $NoBuild) {
    Push-Location $root
    cargo build --release -p araso-typing-test
    $code = $LASTEXITCODE
    if (-not $code) {
        cargo build --release -p araso-typing-test --target i686-pc-windows-msvc
        $code = $LASTEXITCODE
    }
    Pop-Location
    if ($code) { throw 'build failed' }
    if (-not $Installed) { & (Join-Path $PSScriptRoot 'dev-update.ps1') }
}

if (-not $Yes) {
    Read-Host 'Real keys are typed for about a minute. Keep the keyboard on this PC, press Enter, and leave it alone' | Out-Null
}

$tests = @{
    '64' = Join-Path $root 'target\release\araso-typing-test.exe'
    '32' = Join-Path $root 'target\i686-pc-windows-msvc\release\araso-typing-test.exe'
}
$build = if ($Installed) { @() } else { @('--dev') }
$runs = @()
foreach ($bits in '64', '32') {
    foreach ($field in @('', '--imm', '--richedit', '--richedit-imm')) {
        $runs += , @($bits, @($field | Where-Object { $_ }))
    }
    if (-not $NoSandbox) { $runs += , @($bits, @('--appcontainer', '--richedit-imm')) }
}
if ($Browser) { $runs += , @('64', @('--browser')) }

$out = Join-Path $env:TEMP "araso-typing-tests-$PID.out"
$err = Join-Path $env:TEMP "araso-typing-tests-$PID.err"
$failed = 0
foreach ($run in $runs) {
    $bits, $flags = $run
    $start = @{ FilePath = $tests[$bits]; NoNewWindow = $true; Wait = $true; PassThru = $true
        RedirectStandardOutput = $out; RedirectStandardError = $err }
    $arguments = @($build) + @($flags)
    if ($arguments.Count) { $start.ArgumentList = $arguments }
    $p = Start-Process @start
    $lines = @(Get-Content $out -Encoding UTF8)
    $summary = $lines | Where-Object { $_ -match 'cases as expected' } | Select-Object -Last 1
    $where = "$bits-bit $($flags -join ' ')".Trim()
    if ($p.ExitCode -eq 0) {
        Write-Output "ok    $where  $summary"
    } else {
        $failed++
        Write-Output "FAIL  $where  $(if ($summary) { $summary } else { "exit $($p.ExitCode)" })"
        $lines | Where-Object { $_ -match '^FAIL' } | ForEach-Object { Write-Output "        $_" }
        Get-Content $err -Encoding UTF8 | Where-Object { $_ -notmatch '^\[' } | ForEach-Object { Write-Output "        $_" }
    }
}
Remove-Item $out, $err -ErrorAction SilentlyContinue

if ($failed) {
    Write-Output "$failed of $($runs.Count) runs failed"
    exit 1
}
Write-Output "all $($runs.Count) runs as expected"
