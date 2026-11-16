# What the fork changes in upstream-owned code

Upstream is an input the fork imports; the fork wins only on the paths of `.fork/overlay-paths`. Every other difference
is listed in `.fork/allowed-divergence` and, here, with its reason and the upstream change that would make it
unnecessary. A sync or a worker checks this list first: when upstream now does the thing, drop the fork's edit, the
list entry and the row below. Keep both lists as small as possible.

## Process

Every pull request gets a comment "Divergence from upstream" (`.github/workflows/fork-divergence.yml`,
`scripts/fork/divergence_report.py`). It warns and never fails. It uses `.fork/overlay-paths`, the fork-only crates of
`.fork/components.toml` and the workflows to tell fork-owned files from upstream-owned ones, and reports:

* the upstream-owned files the pull request changes, and their changed lines;
* the total divergence: files and lines of upstream-owned code that differ from upstream at the commit it was last replayed to, at the base, at the head and a week before the base;
* the budget of `.fork/divergence-budget.toml`: a warning when one pull request changes more than 300 lines of upstream-owned code (`pr_changed_lines`) or the total grows more than 5 % in 7 days (`weekly_growth_percent`, `window_days`);
* whether this file records each upstream-owned file the pull request starts to change (a file that equals upstream's at the base).

A new upstream-owned file needs a record in this file, in the same pull request, as a `### Area` section with these lines:

```
### <area>
- Files: `crates/x/src/y.rs`, ...
- Upstream approach: what upstream does (or that it does nothing) and how it was checked
- Fork approach: what the fork does instead
- Decision: keep, drop when upstream does X, or offer nothing (upstream is an input)
- Reason: why the fork needs it
- Evidence: the issue, measurement, test or run that shows the need
```

The older table below stays and counts as a reason only ("table row only" in the report, no evidence); new entries use the record form. The sync pull request (`sync/upstream`) is not reported: it replays upstream-owned changes by design.

## Older table

| Fork-only change | Why | Upstream change that makes it unnecessary |
|---|---|---|
| `engine-platform` answers in `fork_platform.rs`: file times of FindFirstFile, known folders, memory status, data folder; hooks in `files.rs`, `process.rs`, `dsound.rs`, `lib.rs`, `host/save_out.rs`, `host/lib.rs` | The game runs on Linux; upstream answers these only on Windows | Upstream implementing the same calls off Windows (it does not as of the 38-commit sync of 2026-11-15) |
| `engine-platform/src/fork_sound.rs`, hooks in `sound_out.rs`, `dsound.rs`, `lib.rs`, `Cargo.toml` | The game's sound output off Windows: libpulse-simple (PipeWire and PulseAudio) or a null sink behind the mixer's `sound_out` seam; upstream's output is WASAPI only (checked 2026-11-15: no audio work on `slaterain/nv-rs` main) | Upstream giving `sound_out` a non-Windows device |
| `engine-platform/src/files/files_host.rs`, `files.rs` (`mod files_host`), `host/cell_offsets.rs` | Game paths (backslashes) map to Linux paths | A path layer in upstream |
| `engine-platform/Cargo.toml`, `d3d9device.rs`, `d3d9resource.rs` | Native Linux build: cfg gates and d3d9-wgpu on every target | Upstream building these on Linux |
| `d3d9-wgpu/tests/frame.rs` | 8x multisampling tests ignored off Windows (lavapipe offers 4x; nv-linux#189) | Upstream skipping them when unsupported |
| `host/src/lib.rs` (`not_provided_here` and its tests) | Upstream refuses to start the game on every system but Windows (slaterain/nv-rs#617); the fork runs it on Linux, so the refusal is cfg-gated to systems other than Windows and Linux | Upstream letting Linux start the game |
| `engine-core/src/engine.rs`, `typed/diff.rs`, `x87.rs`, `units/crt.rs` (test builds only) | Test speed (CI.md, "Upstream offers") | Upstream adopting the per-thread template and the control-word note |
| `engine-core/src/lib.rs`, `engine-havok/src/lib.rs`, `host/src/lib.rs`: appended `test_gate` | Real-data gate (TESTING.md); not overlay paths because upstream adds `mod` lines there | Upstream gating real-data tests itself |
| Overlay: `exe.rs`, `units/crt/{cvt,fdiv,math,printf,trig}.rs`, `hkalookatiksolver.rs`, `nvse/commands/math.rs`, `host/native_inflate.rs` | Test-file edits: real-data gate, math test split, tan/tanh ignored off Windows | Upstream splitting or gating those tests |
| `host/Cargo.toml`, `Cargo.toml`, `Cargo.lock`, `host/src/lib.rs`, `nvse/src/plugins.rs`, crates `jg` and `showoff` | Built-in JohnnyGuitar NVSE and ShowOff xNVSE identities and co-save | Upstream registering those plugins (it has not; JIP LN, UI Organizer and xNVSE are upstream's) |
| `d3d9-wgpu/src/window.rs`: `choose_present_mode`, `present_mode`, `refresh_interval` and the display log line | Present mode by `NV_PRESENT` (Mailbox by default off Windows) and the display's refresh as the pacing interval; checked 2026-11-15: upstream has no display or present-mode choice off Windows | Upstream choosing the present mode and the refresh itself |
| `crates/fork-presence` (new), `viewer/src/main.rs` (two hooks), `viewer/Cargo.toml`, `Cargo.toml`, `Cargo.lock` | Discord Rich Presence while the game runs (docs/fork/DISCORD.md); upstream has none (checked 2026-11-15) | Upstream adding presence |
| `engine-platform/src/d3d9backend.rs`, `d3dx.rs`: messages through `fork_log` | The real runtime's and D3DX's messages follow the terminal policy (LOGGING.md): a level and the target `d3d9`, the fatal ones always printed | Upstream adopting a leveled logger |
| `d3d9-wgpu/src/fixed.rs` (new), hooks in `device.rs`, `lib.rs`, a test in `tests/frame.rs` | A draw with no shaders of pre-transformed vertices (`D3DFVF_XYZRHW`, TEX1, DIFFUSE), which the Bink movie player uses for every frame, was skipped: the screen stayed blank during a movie | Upstream providing the fixed-function path |
| `d3d9-wgpu/src/cache.rs` (`max_bytes`, `evict_blobs`, `trim_list`), a hook in `device.rs` | A size cap for the pipeline folder (`NV_PIPELINE_CACHE_MAX_MB`, default 256): other adapters' driver cache files and the oldest recorded pipeline lines go first, so the folder does not grow without bound (SHADER-CACHE.md) | Upstream capping its pipeline cache |
| `d3d9-wgpu/src/budget.rs` (new), hooks in `device.rs`, `lib.rs`, `window.rs`, a test in `tests/frame.rs` | A budget for the video memory the layer makes (`NV_GPU_BUDGET_MB`, default 80 % of the first DRM card's VRAM): a texture or buffer past it, or a texture side over 16384, is refused with a `stop:` line, and a window client area under 16 pixels a side gets no swap chain; the amdgpu driver ran out of memory once and took the compositor with it | The game goes on without the object; no upstream equivalent |
| `host/src/fixes.rs`, `docs/PLAYTESTING.md`: `NV_FPS_LIMIT` | A plain name for the play frame limit (`iFrameLimit`, 0: unlimited) that the Linux docs use; `NV_FRAME_LIMIT` is unchanged and wins when both are set | Upstream naming the variable itself |
| `README.md` | Fork banner | None; fork-owned text |

## Redundancy check after the second sync of 2026-11-15

The sync after `cc26637b` brought in 7 upstream commits (JIP LN lane B3, JIP game patches, UI Organizer part 2, the pad's
dead zones, conditions on AI threads, ash piles, split beams). They touch `crates/jip`, `crates/nvse`, `crates/uio` (a new
crate), `crates/engine-core`, `crates/engine-typed` and `crates/host` (the drawing thread). None of them touches audio
output, the window, pointer handling, the present mode, the path layer or the `d3d9-wgpu` Linux build, so no fork-only
code in the table above became redundant. The release-files tool's lock file needed the new `uio` crate.
