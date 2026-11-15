# The Windows build, package and launcher

Fork-only. What a player or a manager gets for Windows, how CI builds and tests it, and why it is shaped this way.

## What is built

| Artefact | Made by | Contents |
| --- | --- | --- |
| `nv-rs-programs-<version>-windows-x64` | `fork-release-build.yml`, job `windows` | the programs laid out like a cargo target directory, plus the release files; unchanged, and what `scripts/package-playtest.ps1 -UseBuilt` takes |
| `nv-rs-<version>-windows-x64-package` | `fork-package-windows.yml` (called by the release build, job `package`) | `nv-rs-preview-<version>-windows-x64.zip` (the portable package), `nv-rs-setup-<version>-windows-x64.exe` (the installer), `capabilities.json`, `release-manifest.json`, the two schemas and `SHA256SUMS`, all attested |

The Windows build is native: `windows-latest`, the pinned toolchain, one set of flags for every program (static C runtime,
build-machine paths remapped), the same programs `scripts/package-playtest.ps1` builds. Linux and Windows both stay in the
fork's CI; macOS would be one more job beside them.

`fork-package-windows.yml` also runs on a pull request that changes the packaging (`scripts/fork/package/`, `distribution/`,
`scripts/launcher/`, `viewer/`, itself) and by hand. There its own `build` job builds the programs first (about the time
of the release build's `windows` job) and there are no release files. Nothing publishes a release: the owner does, from the
artefacts.

## The portable ZIP

The same layout and file list as `package-playtest.ps1` (`Play.exe`, `app\FalloutNV.exe` which is `nv-viewer` under the
name NVIDIA's driver fixes go by, `app\tools`, the launchers and settings tool, `READ ME.txt`, the licences), plus
`Choose game folder.cmd`, `WINDOWS READ ME.txt`, `BUILD.txt` and, when the release build made them, `release-files\`.
`scripts/fork/package/package-windows.ps1` stages it and runs `package-check` for the forbidden file types and the build
machine's traces. It cannot run package-check's comparison with the player's own `FalloutNV.exe` (`-Game`), because CI has no
game data and must not: that comparison stays the maintainer's, on the programs artefact, before a release is announced.

## The installer: Inno Setup, not an MSI

Decided here; signing is the owner's call and the installer ships unsigned.

The deciding fact is how nv-rs lives on a PC. It writes its saves, settings and logs in a `userdata` folder next to
`Play.exe`, never touches the game's folder, Documents or AppData, and `Play.exe` refuses to run from Program Files (cannot
write), Documents or AppData. An installer must therefore put the files in a user-writable folder the player chooses, with
no administrator rights, and must not own `userdata`.

| | Inno Setup (chosen) | WiX MSI |
| --- | --- | --- |
| Per-user, no admin, default `C:\Games\nv-rs`, folder chooser | built in (`PrivilegesRequired=lowest`, directory page) | possible (`Scope=perUser`, a custom UI set), but a per-user MSI into a free folder is not what Windows Installer is for and needs custom actions |
| Refuse Program Files, Documents and AppData, also on a silent install | a few lines of Pascal (`nv-rs.iss`, `BadFolder`), tested in CI | a custom action |
| Uninstall leaves `userdata` (the saves) | it only removes what it installed | a component rule per file; repair and major upgrades can fight a folder the app writes in |
| Build | one `ISCC` call, free for any use, preinstalled on the hosted Windows image (no download) | the WiX toolset (v3 or the .NET tool) plus a `.wxs` file list that has to follow every file |
| Silent install for CI and managers | `/VERYSILENT /DIR=...` | `msiexec /qn` |
| What an MSI would add | group policy and Intune deployment | |

An MSI can still be added later for managed PCs if somebody needs it; the staged folder is the input either way.

## The launcher

`Play.exe` (`scripts/launcher`) already is the launcher: it finds Steam, GOG and Epic installs, checks `FalloutNV.exe` is
1.4.0.525, keeps everything it writes in its own folder and starts the game. It already takes `--game FOLDER`, the game's
folder or its `Data` folder, and remembers it in `userdata\game-folder.txt`. What the package adds is a way for a player to
use that without a command line: `Choose game folder.cmd` (a folder picker that starts `Play.exe --game <folder>`), a Start
menu entry for it, and a section in `WINDOWS READ ME.txt`. Nothing in `scripts/launcher` or `viewer` changed.

## The smoke test

`scripts/fork/package/smoke-test.ps1` runs on a hosted Windows runner against three copies: the built `nv-viewer.exe`, the
extracted ZIP and the silently installed copy (then it uninstalls it, and checks the installer refuses Program Files). With no
game data, `nv-viewer <folder> --check` cannot reach its success exit, so the test checks what a program that starts on a
clean PC does: `--help` exits 0; a folder with no game exits 1 naming `FalloutNV.esm`; a synthetic game folder (an empty
`FalloutNV.esm`) without `FalloutNV.exe` exits 1 with `FalloutNV.exe: not found`; a text file named `FalloutNV.exe` exits 1
with the version refusal, not "not found" (the executable check ran); and `Play.exe --check --game <that folder>` exits non-zero
and writes `userdata\logs\launcher.log`. Every file is empty or one line of text made by the script. A run of `--check` on a
real installation is for the owner's PC.

## The release manifest contract

Unchanged. `capabilities.json`, `release-manifest.json` and the schemas are the release build's files, published next to the
package (flat, with `SHA256SUMS`, as the Linux artefact does) and also inside the ZIP under `release-files\`. The manifest's
`installation_check.command` is `nv-viewer`; in the ZIP and the installed copy that program is `app\FalloutNV.exe`, the same
bytes (the package job compares the installed one with the built one). nvmod's `install-nv-rs` keeps consuming the
`windows-x64` programs artefact's `nv-viewer.exe` and the manifest; neither moved.

## SmartScreen and antivirus (unsigned)

Neither the installer nor the programs are signed. Windows SmartScreen shows "Windows protected your PC" on first run: choose
"More info", then "Run anyway", after comparing the file's SHA256 with `SHA256SUMS` from the same release. Windows 11 with
Smart App Control on blocks unsigned programs outright, with no such button; nv-rs cannot run there. An antivirus may flag
`Play.exe` (new, starts other programs). `READ ME.txt` and `WINDOWS READ ME.txt` in the package say this to the player.
A code-signing certificate (Azure Trusted Signing or an OV/EV certificate, each a yearly cost) would remove the SmartScreen
step after the file builds reputation; whether to pay is the owner's decision, and the workflow would only need a signing
step before `Gather the files to publish`.
