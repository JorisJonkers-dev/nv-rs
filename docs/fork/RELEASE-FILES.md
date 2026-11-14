# The release files: capabilities.json and release-manifest.json

A fork-only addition (nothing of it is offered to upstream, and nothing upstream owns was changed for it). A
release build writes two machine-readable files next to the binaries, for the installer and manager that
consume nv-rs as a subprocess (nvmod-rs). They hold names, counts and hashes of this repository's own tables;
no game data.

| File | Schema | What it says |
| --- | --- | --- |
| `capabilities.json` | `.fork/schemas/nv-rs-capabilities.v1.json` | per built-in script extender (xNVSE, JIP LN): implemented and total command counts, and the implemented command names |
| `release-manifest.json` | `.fork/schemas/nv-rs-release-manifest.v1.json` | the version, the upstream base commit, the commit and tree the binaries were built from, the expected hashes of the supported `FalloutNV.exe` sections, the play-patch entry, the installation check, and whether a plugins.txt ordering switch exists (`false`) |

## Where the numbers come from

`scripts/fork/release-files` is a separate workspace (like `scripts/launcher`) that depends on `nvse`, `jip` and
`engine-core` by path, so no upstream file changed.

- **xNVSE**: the commands `nvse::commands::all()` registers plus the `con_*` console re-exports of the name table
  (the sum the README states: 650), against the 654 names of `nvse::names::NAMES`.
- **JIP LN**: the real table. The tool builds an engine, installs xNVSE and then JIP as the game would (JIP answering
  "installed"), and counts the slots that hold a command and have an execute or an eval word, against the slots
  that hold a command (1,118 of JIP's 1,150). This includes the commands JIP's identity and hooks register on
  their own, which are not in `commands::ALL`, so the count is two above the README's figure (`GetLNVersion` and
  `GetNVSEVersionFull`, the identity commands). A command JIP registers at two opcodes is counted, and named, twice.
- **Executable**: `engine_core::exe::SECTIONS`, the section table and FNV-1a 64 hashes the engine already checks
  before it maps the player's executable (`.rdata`, `.data`, `.tls`, `CONST` of FalloutNV.exe 1.4.0.525). They
  are hashes of the sections, not of the whole file, so they say the same for the Steam executable and for the same
  executable patched to use more than 2 GB (the patch changes the header, not these sections).
- **Upstream base commit**: the `Upstream-Commit` trailer of the last replayed commit, found as
  `scripts/fork/verify-sync.sh` finds it.

## The play-patch entry

`play-patch` is a crate, not a program: the translations fixed during a play-build run, linked into `nv-viewer`
and registered over the engine's own when the game starts (`host::Host::start`). It takes no base copy and no
instance, and there is no separate step to run; the list is empty on the fork line between play loops. The
manifest therefore says `"standalone_step": false`, `"command": null`. What a manager can run is the installation
check, `nv-viewer <data folder> --check` (exit 0 when `FalloutNV.exe` is the supported build and the run can start),
which the manifest lists as `installation_check`. Any step that takes a base copy and an instance does not exist in
nv-rs today.

## Keeping them true

- `.fork/capabilities.json` is checked in. The `release-files` component runs the tool's tests on every pull
  request that builds: they validate both generated files against the schemas, and fail when the checked-in file
  differs from what the tables generate. After a sync, or any change to a command table, run `task capabilities`
  and commit the result (a sync pull request that moves the tables fails this check until it does).
- The release build (`fork-release-build.yml`, job `release-files`) generates both files, validates them against the
  schemas, refuses a tree whose checked-in capabilities file is out of date, and adds them and the schemas to the
  Linux and Windows artefacts. Those artefacts' `SHA256SUMS` list them, and the build attestation covers them.
- Adding an optional field is not a version change; removing, renaming or retyping one is: add `v2` schemas and keep
  writing `v1` for a release.
