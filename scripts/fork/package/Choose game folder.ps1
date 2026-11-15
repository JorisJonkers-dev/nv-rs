# Lets the player point nv-rs at the game's folder, then starts Play.exe with --game on it (Play.exe checks the folder,
# remembers it in userdata\game-folder.txt and never writes to it). Nothing is written here.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
$play = Join-Path $PSScriptRoot 'Play.exe'
if (-not (Test-Path -LiteralPath $play)) {
    [void][System.Windows.Forms.MessageBox]::Show('Play.exe is not next to this file. Extract the whole package again.', 'nv-rs', 'OK', 'Error')
    exit 1
}
$dialog = New-Object System.Windows.Forms.FolderBrowserDialog
$dialog.Description = "Choose your Fallout: New Vegas folder (the one that holds Data\FalloutNV.esm, or the Data folder itself)."
$dialog.ShowNewFolderButton = $false
if ($dialog.ShowDialog() -ne [System.Windows.Forms.DialogResult]::OK) { exit 0 }
# One argument, quoted whole, so a folder with spaces stays one.
Start-Process -FilePath $play -ArgumentList @('--game', ('"{0}"' -f $dialog.SelectedPath)) -WorkingDirectory $PSScriptRoot
