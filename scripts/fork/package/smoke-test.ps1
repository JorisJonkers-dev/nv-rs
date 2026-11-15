# Smoke test of a built nv-viewer (and, with -Launcher, of Play.exe) on a PC with no game: runs the real programs against
# synthetic folders and checks what they say. `--check` on the real game's folder is the maintainer's to run (CI has no game
# data and must not); what this proves is that the program starts on a clean Windows PC (no missing runtime, no crash at
# load), parses its arguments and reaches its installation check, which refuses each synthetic installation for the right
# reason with exit status 1. Every file made here is empty or plain text made by this script.
#
#   smoke-test.ps1 -Viewer <nv-viewer.exe or app\FalloutNV.exe> [-Launcher <Play.exe>] -Work <scratch folder>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Viewer,
    [string]$Launcher,
    [Parameter(Mandatory)][string]$Work
)
$ErrorActionPreference = 'Stop'
$Viewer = (Resolve-Path -LiteralPath $Viewer).Path
New-Item -ItemType Directory -Force -Path $Work | Out-Null
$Work = (Resolve-Path -LiteralPath $Work).Path
$script:failures = @()

function Invoke-Program([string]$Program, [string[]]$Arguments) {
    $out = Join-Path $Work 'stdout.txt'
    $err = Join-Path $Work 'stderr.txt'
    $p = Start-Process -FilePath $Program -ArgumentList $Arguments -NoNewWindow -PassThru `
        -RedirectStandardOutput $out -RedirectStandardError $err
    if (-not $p.WaitForExit(120000)) { $p.Kill(); throw "$Program did not finish in 120 s" }
    [pscustomobject]@{ Code = $p.ExitCode; Out = (Get-Content -Raw -LiteralPath $out); Err = (Get-Content -Raw -LiteralPath $err) }
}
function Expect([string]$What, [bool]$Ok, $Result) {
    if ($Ok) { Write-Host "ok   $What" }
    else {
        Write-Host "FAIL $What (exit $($Result.Code))"
        Write-Host "     stdout: $($Result.Out)"
        Write-Host "     stderr: $($Result.Err)"
        $script:failures += $What
    }
}

# Neither "Data" nor a master in it: the folder is refused before anything else.
$empty = Join-Path $Work 'empty'
New-Item -ItemType Directory -Force -Path $empty | Out-Null
$game = Join-Path $Work 'game'
New-Item -ItemType Directory -Force -Path (Join-Path $game 'Data') | Out-Null
New-Item -ItemType File -Force -Path (Join-Path $game 'Data/FalloutNV.esm') | Out-Null

$r = Invoke-Program $Viewer @('--help')
Expect '--help prints the usage and exits 0' ($r.Code -eq 0 -and $r.Out -match '--check') $r

$r = Invoke-Program $Viewer @('--check', $empty)
Expect '--check on a folder with no game exits 1 and names FalloutNV.esm' ($r.Code -eq 1 -and $r.Err -match 'FalloutNV\.esm') $r

$r = Invoke-Program $Viewer @('--check', $game)
Expect '--check on a game folder without FalloutNV.exe exits 1: not found' ($r.Code -eq 1 -and $r.Err -match 'FalloutNV\.exe: not found') $r

Set-Content -LiteralPath (Join-Path $game 'FalloutNV.exe') -Value 'this is not the game program' -Encoding ascii
$r = Invoke-Program $Viewer @('--check', $game)
Expect '--check on a FalloutNV.exe that is not 1.4.0.525 exits 1 with a reason' ($r.Code -eq 1 -and $r.Err -match '(?m)^error:' -and $r.Err -notmatch 'FalloutNV\.exe: not found') $r

if ($Launcher) {
    $Launcher = (Resolve-Path -LiteralPath $Launcher).Path
    $env:NV_RS_NO_WINDOWS = '1'
    $env:NV_RS_NO_STORES = '1'
    $r = Invoke-Program $Launcher @('--check', '--game', ('"{0}"' -f $game))
    $log = Join-Path (Split-Path $Launcher) 'userdata/logs/launcher.log'
    Expect 'Play.exe --check --game <a folder that is not the game> exits non-zero and logs why' `
        ($r.Code -ne 0 -and (Test-Path -LiteralPath $log) -and (Get-Item -LiteralPath $log).Length -gt 0) $r
    if (Test-Path -LiteralPath $log) { Get-Content -LiteralPath $log | ForEach-Object { Write-Host "     launcher.log: $_" } }
}

if ($script:failures) { throw "smoke test failed: $($script:failures -join '; ')" }
Write-Host 'smoke test passed'
