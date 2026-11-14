# Stages the Windows package from programs that were built before, and zips it. The CI-capable sibling of
# scripts/package-playtest.ps1 (which needs the player's FalloutNV.exe to prove no game bytes are in the programs, so it can
# only run on the maintainer's PC): same layout, same file list, same checks except the game comparison. Nothing built here
# and no game file anywhere. Writes <Out>\nv-rs-preview-<Version>-windows-x64\ (the stage, also the installer's input) and
# <Out>\nv-rs-preview-<Version>-windows-x64.zip.
#
#   package-windows.ps1 -Version 2026-11-07.2 -Programs <folder with release\ and i686-pc-windows-msvc\release\> -Out dist
#       [-ReleaseFiles <folder with capabilities.json, release-manifest.json and the schemas>]
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidatePattern('^[a-zA-Z0-9][a-zA-Z0-9._-]+$')][string]$Version,
    [Parameter(Mandatory)][string]$Programs,
    [Parameter(Mandatory)][string]$Out,
    [string]$ReleaseFiles,
    [string]$Commit = $(git rev-parse HEAD),
    [string]$Repository = $(if ($env:GITHUB_REPOSITORY) { $env:GITHUB_REPOSITORY } else { 'JorisJonkers-dev/nv-rs' })
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$here = $PSScriptRoot
$x64 = Join-Path $Programs 'release'
$x86 = Join-Path $Programs 'i686-pc-windows-msvc/release'
$name = "nv-rs-preview-$Version-windows-x64"
$stage = Join-Path $Out $name
$zip = "$stage.zip"
if ((Test-Path -LiteralPath $stage) -or (Test-Path -LiteralPath $zip)) { throw "This package already exists: $stage" }
New-Item -ItemType Directory -Path (Join-Path $stage 'app/tools') -Force | Out-Null

# Our own programs. app\FalloutNV.exe is nv-viewer under that name (NVIDIA's driver fixes and Windows' per-program sound
# go by the name); the program the release manifest calls `nv-viewer` is that file.
$programs = [ordered]@{
    (Join-Path $x64 'Play.exe')                  = 'Play.exe'
    (Join-Path $x64 'nv-viewer.exe')             = 'app/FalloutNV.exe'
    (Join-Path $x64 'd3d9-capture.exe')          = 'app/tools/d3d9-capture.exe'
    (Join-Path $x86 'nvapi-capture.exe')         = 'app/tools/nvapi-capture.exe'
    (Join-Path $x86 'dsound-capture.exe')        = 'app/tools/dsound-capture.exe'
}
foreach ($src in $programs.Keys) {
    if (-not (Test-Path -LiteralPath $src)) { throw "missing build output: $src" }
    Copy-Item -LiteralPath $src -Destination (Join-Path $stage $programs[$src])
}
# The launchers, the settings tool, the game-folder chooser and the READ MEs.
foreach ($file in @('Play with mods.cmd', 'Import my saves.cmd', 'Settings.cmd', 'Settings.ps1', 'READ ME.txt', 'THIRD-PARTY-NOTICES.txt')) {
    Copy-Item -LiteralPath (Join-Path $root "distribution/$file") -Destination $stage
}
foreach ($file in @('Choose game folder.cmd', 'Choose game folder.ps1', 'WINDOWS READ ME.txt')) {
    Copy-Item -LiteralPath (Join-Path $here $file) -Destination $stage
}
New-Item -ItemType Directory -Path (Join-Path $stage 'mods') | Out-Null
Copy-Item -LiteralPath (Join-Path $root 'distribution/mods/READ ME.txt') -Destination (Join-Path $stage 'mods')
# The licences and notices LEGAL.md asks for.
Copy-Item -LiteralPath (Join-Path $root 'distribution/licenses') -Destination $stage -Recurse
foreach ($file in @('LICENSE', 'LEGAL.md')) { Copy-Item -LiteralPath (Join-Path $root $file) -Destination $stage }
Copy-Item -LiteralPath (Join-Path $root 'crates/vorbis/LICENSE-libvorbis') -Destination (Join-Path $stage 'licenses/LICENSE-libvorbis')
Copy-Item -LiteralPath (Join-Path $root 'crates/nvse/LICENSE-msvc-stl') -Destination (Join-Path $stage 'licenses/LICENSE-msvc-stl')
# Text files for Notepad.
foreach ($file in @('READ ME.txt', 'WINDOWS READ ME.txt', 'mods/READ ME.txt')) {
    $path = Join-Path $stage $file
    $text = [IO.File]::ReadAllText($path) -replace "`r?`n", "`r`n"
    [IO.File]::WriteAllText($path, $text, (New-Object Text.UTF8Encoding($false)))
}
# The two release files and their schemas, when the release build made them: the same bytes the release-files artifact holds
# (the manifest nvmod reads is the one published next to the package, this copy says what the package was built from).
if ($ReleaseFiles) {
    New-Item -ItemType Directory -Path (Join-Path $stage 'release-files') | Out-Null
    foreach ($file in @('capabilities.json', 'release-manifest.json')) {
        $src = Join-Path $ReleaseFiles $file
        if (-not (Test-Path -LiteralPath $src)) { throw "missing release file: $src" }
        Copy-Item -LiteralPath $src -Destination (Join-Path $stage 'release-files')
    }
}
$sha = (Get-FileHash -LiteralPath (Join-Path $stage 'app/FalloutNV.exe') -Algorithm SHA256).Hash
@("nv-rs Windows preview $Version", "Source commit: $Commit", "app\FalloutNV.exe SHA256: $sha", 'Platform: Windows x64', "Source: https://github.com/$Repository/tree/$Commit") |
    Set-Content -LiteralPath (Join-Path $stage 'BUILD.txt') -Encoding ascii

# What must not be in it: the file types first (the release files are .json, so nothing here allows a game or build file).
$forbidden = '\.(esm|esp|bsa|nif|kf|dds|ogg|mp3|wav|lip|bik|fos|sdp|pdb|tsv|ini|log|dll|sym)$'
$bad = Get-ChildItem -LiteralPath $stage -Recurse -File | Where-Object { $_.Name -match $forbidden }
if ($bad) { throw "These must not be in the package: $($bad.FullName -join ', ')" }
# The build machine's traces, in every file and, as the disassembler's names, in the programs. The game comparison of
# package-playtest.ps1 (-Game) needs the player's own FalloutNV.exe and cannot run here: the maintainer runs it on the
# programs artifact before a release is announced.
$needles = @($env:USERNAME, $env:USERPROFILE, $root) | Where-Object { $_ }
$programNeedles = @('nv-re', 'build43', 'fnv_decomp', '\target\release', 'FUN_', 'DAT_', 'LAB_')
$arguments = @($stage) + ($needles | ForEach-Object { '--needle'; $_ }) + ($programNeedles | ForEach-Object { '--program-needle'; $_ })
& (Join-Path $x64 'package-check.exe') @arguments
if ($LASTEXITCODE) { throw 'package-check found something that must not be in the package.' }

Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
# File by file: entry names need forward slashes.
$archive = [IO.Compression.ZipFile]::Open($zip, [IO.Compression.ZipArchiveMode]::Create)
try {
    foreach ($file in Get-ChildItem -LiteralPath $stage -Recurse -File) {
        $entry = $name + '/' + ($file.FullName.Substring($stage.TrimEnd('\', '/').Length + 1) -replace '\\', '/')
        [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, $file.FullName, $entry, [IO.Compression.CompressionLevel]::Optimal)
    }
} finally { $archive.Dispose() }
Write-Host "$zip $((Get-Item -LiteralPath $zip).Length) bytes; stage $stage"
