# Builds the unsigned installer with Inno Setup from the staged package (package-windows.ps1's stage folder).
#   build-installer.ps1 -Version 2026-11-07.2 -Stage dist\nv-rs-preview-...-windows-x64 -Out dist
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidatePattern('^[a-zA-Z0-9][a-zA-Z0-9._-]+$')][string]$Version,
    [Parameter(Mandatory)][string]$Stage,
    [Parameter(Mandatory)][string]$Out
)
$ErrorActionPreference = 'Stop'
$iscc = @(
    (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source,
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
) | Where-Object { $_ -and (Test-Path -LiteralPath $_) } | Select-Object -First 1
if (-not $iscc) { throw 'Inno Setup (ISCC.exe) is not installed.' }
# YYYY-MM-DD.N becomes 2026.11.7.2 and X.Y.Z becomes X.Y.Z.0 for the file's version resource; any other version text stays 0.0.0.0 there.
$numeric = '0.0.0.0'
if ($Version -match '^(\d+)\.(\d+)\.(\d+)$') { $numeric = '{0}.{1}.{2}.0' -f [int]$Matches[1], [int]$Matches[2], [int]$Matches[3] }
if ($Version -match '^(\d{4})-(\d{2})-(\d{2})\.(\d+)$') { $numeric = '{0}.{1}.{2}.{3}' -f [int]$Matches[1], [int]$Matches[2], [int]$Matches[3], [int]$Matches[4] }
$stageFull = (Resolve-Path -LiteralPath $Stage).Path
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$outFull = (Resolve-Path -LiteralPath $Out).Path
& $iscc "/DVersion=$Version" "/DVersionNumeric=$numeric" "/DStage=$stageFull" "/DOutputDir=$outFull" (Join-Path $PSScriptRoot 'nv-rs.iss')
if ($LASTEXITCODE) { throw 'ISCC failed.' }
$setup = Join-Path $outFull "nv-rs-setup-$Version-windows-x64.exe"
if (-not (Test-Path -LiteralPath $setup)) { throw "the installer was not written: $setup" }
Write-Host "$setup $((Get-Item -LiteralPath $setup).Length) bytes"
