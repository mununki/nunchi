# Writes the license notices of the Rust crates Nunchi's programs link (nunchi_tsf.dll 64- and
# 32-bit, nunchi-settings.exe), as Cargo.lock has them: each crate with its license, then each
# license text once, with the crates it's for. The installer carries it (licenses\rust-crates.txt).
# Proc-macros and build-only crates aren't in the programs, so they aren't listed.
param([string]$Out = (Join-Path (Split-Path $PSScriptRoot -Parent) 'target\licenses\rust-crates.txt'))

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"

$packages = @{}
$linked = @{}
foreach ($target in 'x86_64-pc-windows-msvc', 'i686-pc-windows-msvc') {
    $json = cargo metadata --format-version 1 --locked --manifest-path (Join-Path $root 'Cargo.toml') --filter-platform $target
    if ($LASTEXITCODE) { throw 'cargo metadata failed' }
    $metadata = $json | ConvertFrom-Json
    $nodes = @{}
    foreach ($node in $metadata.resolve.nodes) { $nodes[$node.id] = $node }
    foreach ($package in $metadata.packages) { $packages[$package.id] = $package }
    $roots = $metadata.packages | Where-Object { $_.name -in 'nunchi-tsf', 'nunchi-settings' }
    $queue = [System.Collections.Generic.Queue[string]]::new()
    foreach ($package in $roots) { $queue.Enqueue($package.id) }
    $seen = @{}
    while ($queue.Count) {
        $id = $queue.Dequeue()
        if ($seen[$id]) { continue }
        $seen[$id] = $true
        $package = $packages[$id]
        if ($package.targets | Where-Object { $_.kind -contains 'proc-macro' }) { continue }
        if ($package.source) { $linked[$id] = $true }
        foreach ($dep in $nodes[$id].deps) {
            if ($dep.dep_kinds | Where-Object { $null -eq $_.kind }) { $queue.Enqueue($dep.pkg) }
        }
    }
}

# License files: at the top of the crate, and a level down the fonts' (egui's fonts: OFL.txt, or
# Hack-Regular.txt beside Hack-Regular.ttf).
$pattern = '^(copying|copyright|notice|unlicense|ofl|ufl)|licen[cs]e'
function LicenseFiles($dir) {
    $top = Get-ChildItem $dir -File | Where-Object { $_.Name -match $pattern -and $_.Extension -notin '.rs', '.toml' }
    $below = Get-ChildItem $dir -Directory | ForEach-Object {
        $files = Get-ChildItem $_.FullName -File
        $fonts = $files | Where-Object { $_.Extension -in '.ttf', '.otf' } | ForEach-Object { $_.BaseName }
        $files | Where-Object { $_.Extension -eq '.txt' -and ($_.Name -match $pattern -or $_.BaseName -in $fonts) }
    }
    @($top) + @($below) | Where-Object { $_ }
}

$crates = $linked.Keys | ForEach-Object { $packages[$_] } | Sort-Object name, version
$texts = [ordered]@{}
$users = @{}
$missing = @()
foreach ($crate in $crates) {
    $label = "$($crate.name) $($crate.version)"
    $files = LicenseFiles (Split-Path $crate.manifest_path -Parent)
    if (-not $files) { $missing += $label }
    foreach ($file in $files) {
        $text = ((Get-Content $file.FullName -Raw -Encoding UTF8) -replace "`r`n", "`n").Trim()
        $key = ($text -replace '\s+', ' ')
        if (-not $texts.Contains($key)) { $texts[$key] = $text; $users[$key] = @() }
        if ($users[$key] -notcontains $label) { $users[$key] += $label }
    }
}

$lines = @(
    "Rust crates in Nunchi's programs (nunchi_tsf.dll, nunchi-settings.exe) and their licenses, then"
    "the license texts the crates carry. A crate offering a choice of licenses is used under a"
    "permissive one (self_cell under Apache-2.0, not GPL-2.0). Generated from Cargo.lock by"
    "tools/crate-licenses.ps1."
    ''
)
foreach ($crate in $crates) {
    $lines += "$($crate.name) $($crate.version): $($crate.license)$(if ($crate.repository) { ", $($crate.repository)" })"
}
# What the note below says has to stay true when Cargo.lock changes.
$unexplained = $crates | Where-Object { "$($_.name) $($_.version)" -in $missing -and $_.license -notmatch 'Apache-2\.0|BSL-1\.0' }
if ($unexplained) { throw "no license file, and not Apache-2.0 or BSL-1.0: $(($unexplained | ForEach-Object { $_.name }) -join ', ')" }
foreach ($needed in 'Apache License\s+Version 2\.0', 'Boost Software License - Version 1\.0') {
    if (-not ($texts.Keys | Where-Object { $_ -match $needed })) { throw "no crate carries the text of $needed" }
}
if ($missing) {
    $lines += '', "These carry no license file of their own and are used under the license named above (the"
    $lines += "Apache License 2.0 or the Boost Software License 1.0, whose texts are below): $($missing -join ', ')"
}
foreach ($key in $texts.Keys) {
    $lines += '', ('=' * 78), "For: $($users[$key] -join ', ')", ('=' * 78), '', $texts[$key]
}

New-Item -ItemType Directory -Force (Split-Path $Out -Parent) | Out-Null
[System.IO.File]::WriteAllText($Out, ($lines -join "`r`n") + "`r`n", [System.Text.UTF8Encoding]::new($true))
"$($crates.Count) crates, $($texts.Count) license texts, no license file: $($missing.Count)"
$Out
