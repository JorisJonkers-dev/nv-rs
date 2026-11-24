# What the fork changes in upstream-owned code

Upstream is an input the fork imports; the fork wins only on the paths of `.fork/overlay-paths`. Every other difference
is listed in `.fork/allowed-divergence` (and `.fork/allowed-divergence.d/`) and recorded with its reason and the
upstream change that would make it unnecessary: one file per record in [`divergence.d/`](divergence.d/), and the
older table below. A sync or a worker checks these first: when upstream now does the thing, drop the fork's edit, the
list entry and the record. Keep both lists as small as possible.

A reference elsewhere of the form `docs/fork/DIVERGENCE.md, "<Area>"` names the record whose heading is
`### <Area>`: the file of `divergence.d/` named after it (`grep -l '^### <Area>' docs/fork/divergence.d/*.md`).

## Process

Every pull request gets a comment "Divergence from upstream" (`.github/workflows/fork-divergence.yml`,
`scripts/fork/divergence_report.py`). It warns and never fails. It uses `.fork/overlay-paths`, the fork-only crates of
`.fork/components.toml` and the workflows to tell fork-owned files from upstream-owned ones, and reports:

* the upstream-owned files the pull request changes, and their changed lines;
* the total divergence: files and lines of upstream-owned code that differ from upstream at the commit it was last replayed to, at the base, at the head and a week before the base;
* the budget of `.fork/divergence-budget.toml`: a warning when one pull request changes more than 300 lines of upstream-owned code (`pr_changed_lines`) or the total grows more than 5 % in 7 days (`weekly_growth_percent`, `window_days`);
* whether a record of `divergence.d/` (or the older table) covers each upstream-owned file the pull request starts to change (a file that equals upstream's at the base).

A new upstream-owned file needs a record in the same pull request: a new file `docs/fork/divergence.d/<area>.md`
(lower case, words joined by `-`), never an edit of this file, so pull requests do not conflict here. It holds one
`### Area` section with these lines:

```
### <area>
- Files: `crates/x/src/y.rs`, ...
- Upstream approach: what upstream does (or that it does nothing) and how it was checked
- Fork approach: what the fork does instead
- Decision: keep, drop when upstream does X, or offer nothing (upstream is an input)
- Reason: why the fork needs it
- Evidence: the issue, measurement, test or run that shows the need
```

The older table below is frozen (no new rows) and counts as a reason only ("table row only" in the report, no evidence); new entries use the record form. The sync pull request (`sync/upstream`) is not reported: it replays upstream-owned changes by design.

## Older table

| Fork-only change | Why | Upstream change that makes it unnecessary |
|---|---|---|
| `engine-platform` answers in `fork_platform.rs`: known folders, data folder (memory status is upstream's `machine_table` since slaterain/nv-rs#645; the file times of FindFirstFile off Windows are upstream's `files.rs` since slaterain/nv-rs#633, and `filetime`, `creation` and `find_fields` are no longer called by upstream's files); hooks in `dsound.rs`, `lib.rs`, `host/save_out.rs` (the native build's own Saves folder when `NV_SAVE_OUT` is unset, `known_folders` off Windows), `host/lib.rs`, `host/game_ini.rs` | The fork keeps the game's folders, tables and saves under `<data_dir>`; upstream's machine table (slaterain/nv-rs#681, slaterain/nv-rs#687) puts them in its own folder (`<own folder>/game`) | Upstream's own folder becoming the fork's (then the fork's folders and the hooks go) |
| `ds-mix/src/fork_sound.rs` (and `ds-mix/examples/nv_sound_report.rs`), hooks in `ds-mix/src/sound_out.rs`, `lib.rs`, `Cargo.toml` (the sound table off Windows: "Machine table off Windows") | The game's sound output on Linux: libpulse (PipeWire and PulseAudio) loaded at run time, or a null sink, behind the mixer's `sound_out` seam. Upstream's cpal device (slaterain/nv-rs#637, ALSA on Linux) is built for macOS only here: with it the Linux program needs `libasound.so.2` to start at all, and it has no null sink for headless runs | Upstream's device starting without ALSA's library and offering a sink for headless runs |
| `d3d9-wgpu/tests/frame.rs` | 8x multisampling tests ignored off Windows (lavapipe offers 4x; #198) | Upstream skipping them when unsupported |
| `engine-core/src/engine.rs`, `typed/diff.rs`, `x87.rs`, `units/crt.rs` (test builds only) | Test speed (CI.md, "Upstream offers") | Upstream adopting the per-thread template and the control-word note |
| `host/src/lib.rs`: a clippy allow on `the_expected_stop_off_windows_has_its_own_status` | Its `Ok` arm asserts a constant on Linux, which `clippy -D warnings` refuses; upstream's Linux check does not run clippy on tests | Upstream allowing or rewriting it |
| `engine-core/src/lib.rs`, `engine-havok/src/lib.rs`, `host/src/lib.rs`: appended `test_gate` | Real-data gate (TESTING.md); not overlay paths because upstream adds `mod` lines there | Upstream gating real-data tests itself |
| Overlay: `exe.rs`, `units/crt/{cvt,fdiv,math,printf,trig}.rs`, `hkalookatiksolver.rs`, `nvse/commands/math.rs`, `host/native_inflate.rs` | Test-file edits: real-data gate, math test split, tan/tanh ignored off Windows. Upstream's changes to these are applied to the fork's copies by hand (in the sync to 1b438957: slaterain/nv-rs#706's soft x87 bounds in `fdiv.rs`, `math.rs`, `trig.rs`) | Upstream splitting or gating those tests |
| `host/Cargo.toml`, `Cargo.toml`, `Cargo.lock`, `host/src/lib.rs`, `nvse/src/plugins.rs`, crates `jg` and `showoff` | Built-in JohnnyGuitar NVSE and ShowOff xNVSE identities and co-save | Upstream registering those plugins (it has not; JIP LN, UI Organizer and xNVSE are upstream's) |
| `d3d9-wgpu/src/window.rs`: `choose_present_mode`, `present_mode`, `refresh_interval` and the display log line | Present mode by `NV_PRESENT` (Mailbox by default off Windows) and the display's refresh as the pacing interval; checked 2026-11-15: upstream has no display or present-mode choice off Windows | Upstream choosing the present mode and the refresh itself |
| `crates/fork-api` (new), `Cargo.toml`, `Cargo.lock` | The typed in-process facade a program that links the engine's crates depends on (nv-rs#353): readers re-exported, a save summary, the capabilities file and a version number; additive, one workspace member and its lock entry in upstream-owned files | Upstream providing a library facade of its own |
| `crates/fork-play` (new), `viewer/src/main.rs` (`pub fn run` and one stop poll), `Cargo.toml`, `Cargo.lock` | The play-mode entry point of the engine for a program that links it (nv-rs#353): the viewer's loop is included by `#[path]` so there is one loop; `run` becomes `pub` and the frame loop polls `fork_log::stop_requested` (the flag is set by a stop file thread in `fork-play`), a drift-guard test fails when the viewer's dependencies are not `fork-play`'s; a copy of the loop or a shim would conflict with every upstream change to the viewer | Upstream exposing the viewer's loop as a library, or changing the frame loop around the poll |
| `crates/fork-presence` (new), `viewer/src/main.rs` (two hooks), `viewer/Cargo.toml`, `Cargo.toml`, `Cargo.lock` | Discord Rich Presence while the game runs (docs/fork/DISCORD.md); upstream has none (checked 2026-11-15) | Upstream adding presence |
| `engine-platform/src/d3d9backend.rs`, `d3dx.rs`, `d3d9real.rs` and `d3d9real/win32.rs` (start-up and window placement lines): messages through `fork_log` | The real runtime's and D3DX's messages follow the terminal policy (LOGGING.md): a level and the target `d3d9`, the fatal ones always printed | Upstream adopting a leveled logger |
| `d3d9-wgpu/src/cache.rs` (`max_bytes`, `evict_blobs`, `trim_list`), a hook in `device.rs` | A size cap for the pipeline folder (`NV_PIPELINE_CACHE_MAX_MB`, default 256): other adapters' driver cache files and the oldest recorded pipeline lines go first, so the folder does not grow without bound (SHADER-CACHE.md); the eviction line goes through `fork_log` (`info`, target `d3d9`) | Upstream capping its pipeline cache |
| `d3d9-wgpu/src/budget.rs` (new), hooks in `device.rs` (also the sun shadows' own index lists, charged when they fit), `lib.rs`, `window.rs`, a test in `tests/frame.rs` | A budget for the video memory the layer makes (`NV_GPU_BUDGET_MB`, default 80 % of the first DRM card's VRAM): a texture or buffer past it, or a texture side over 16384, is refused with a `stop:` line, and a window client area under 16 pixels a side gets no swap chain; the amdgpu driver ran out of memory once and took the compositor with it | The game goes on without the object; no upstream equivalent |
| `host/src/fixes.rs`, `docs/PLAYTESTING.md`: `NV_FPS_LIMIT` | A plain name for the play frame limit (`iFrameLimit`, 0: unlimited) that the Linux docs use; `NV_FRAME_LIMIT` is unchanged and wins when both are set | Upstream naming the variable itself |
| `.github/workflows/fork-release-build.yml` (`packages` job), `fork-packages-dry-run.yml` | Publishes the Linux and Windows builds as OCI packages on ghcr.io (`ghcr.io/jorisjonkers-dev/nv-rs:<version>-<target>`, Toolbox ADR-0017); fork-only, upstream has no such registry | None; fork-owned release plumbing |
| `README.md` | Fork banner | None; fork-owned text |

## Redundancy check after the second sync of 2026-11-15

The sync after `cc26637b` brought in 7 upstream commits (JIP LN lane B3, JIP game patches, UI Organizer part 2, the pad's
dead zones, conditions on AI threads, ash piles, split beams). They touch `crates/jip`, `crates/nvse`, `crates/uio` (a new
crate), `crates/engine-core`, `crates/engine-typed` and `crates/host` (the drawing thread). None of them touches audio
output, the window, pointer handling, the present mode, the path layer or the `d3d9-wgpu` Linux build, so no fork-only
code in the table above became redundant. The release-files tool's lock file needed the new `uio` crate.

## Redundancy check after the sync to 1b438957 (2026-11-18)

The 44 upstream commits from `a489e36c` brought upstream's own Linux start (slaterain/nv-rs#687), its machine tables off
Windows (slaterain/nv-rs#681), long-name-only searches and no short names off Windows (slaterain/nv-rs#712,
slaterain/nv-rs#718) and relative names found without case (slaterain/nv-rs#697). Retired for upstream's code: the Linux
arms of the start's refusal, the fork's off-Windows search and short-name arms, `files/files_host.rs`, and the hand-written
sound tables of `fork_tables.rs`. Kept: the window layer, the adapter table, the Windows two-letter rule, the mods' files
folder beside the native saves. New records: "Machine table off Windows", `game_ini.rs`.
