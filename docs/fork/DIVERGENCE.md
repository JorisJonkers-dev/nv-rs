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

### Short names off Windows
- Files: `crates/engine-platform/src/files.rs` (`search_finds`, `listing`, `player_entry`, `short_name`, `write_entry`, `find_first_file_a`; the fork's `files/short_names.rs` removed)
- Upstream approach: off Windows upstream lists host files with empty short names, but `search_finds` still matches against a short name a name would have and stops the run on any pattern its first letter does not rule out; on Windows the file system answers (checked on `origin/main`, 2026-11-16)
- Fork approach: off Windows the host keeps no 8.3 short names, as a Windows volume with their generation off: a search matches the long name only and never stops for a short name, and `cAlternateFileName` is empty for host files and the run's files. On Windows the behaviour is unchanged (the first two letters rule out a short name, the plain stop stays)
- Decision: drop when upstream matches long names only off Windows
- Reason: wildcard searches for a master's archives (`<name>*.bsa`) in a folder that holds an archive whose long name has spaces stopped the game in its first frame, once per pattern the short-name rules could not decide
- Evidence: JorisJonkers-dev/nv-rs#331; the test `off_windows_a_search_matches_the_long_name_only`, the save tests in `tests_changed_directories` and `crates/host/tests/linux_saves.rs`

### Direct3D 9 window layer off Windows
- Files: `crates/engine-platform/src/d3d9real.rs`, `crates/engine-platform/src/d3d9real/elsewhere.rs` (deleted), `crates/engine-platform/src/fork_d3d9rec.rs` (fork-owned)
- Upstream approach: since slaterain/nv-rs#635, `d3d9real.rs` builds on every system, the Win32 window moved unchanged into `d3d9real/win32.rs`, and off Windows `d3d9real/elsewhere.rs` stops the run when the game asks for a window ("no window layer"). The real Direct3D 9 parts are `cfg(windows)`, and asking for it off Windows ends the run.
- Fork approach: off Windows the window layer is `fork_d3d9rec.rs`: a fork-window window on X11 or Wayland (off screen without a display or with `NV_D3D9_OFFSCREEN`), its keys and mouse as the game's input, and the wgpu stream with the shader cache; `real` and `real,wgpu` draw on wgpu with a line instead of ending the run. Upstream's split, its `RealDevice` and `QueryBoard` types and its `cfg(windows)` placement replace the fork's own `cfg(windows)` lines and three stand-ins (the device alias, the shared device and `query_without_waiting`).
- Decision: keep until upstream's own window layer off Windows (its "PR 3b") draws the game; then compare and take upstream's where it is equal.
- Reason: the fork runs the game on Linux; upstream's layer refuses a window there.
- Evidence: `d3d9real::fork::tests::a_guest_program_draws_a_frame_through_the_recorder_on_wgpu` (a frame drawn on lavapipe in the Linux job), and the Linux play path in docs/fork/LINUX.md.

### Mods' own files on the native build
- Files: `crates/host/src/mod_files.rs`, `crates/engine-platform/src/mod_files.rs`, `crates/engine-platform/src/files.rs`, `files/files_host.rs` and `fork_platform.rs` (fork-owned)
- Upstream approach: since slaterain/nv-rs#629, mods' own files are kept in `NV_MOD_FILES_OUT`, else beside `NV_SAVE_OUT`'s folder; with neither there is none and their settings last for the session only. Any folder inside Documents or AppData is refused.
- Fork approach: off Windows the native build keeps its saves in the port's own Documents when `NV_SAVE_OUT` is unset (`save_out::native_saves_dir`), so with neither variable set mods' files go beside those saves (`<documents>/My Games/FalloutNV/nv-rs mod files`). That Documents is the port's, so only the game-folder and path checks apply. A set variable goes through upstream's rules unchanged. The folder's copy of a name is found without regard to case inside the folder off Windows (`files_host::host_path_in`), as Windows finds it; upstream's own `files::host_path` gives way to the fork's re-export of the same function. Upstream marks its Windows-path `mod_files` tests Windows only (slaterain/nv-rs#660), and a fork test covers the Unix path.
- Decision: keep until upstream gives the native build a default folder.
- Reason: the Linux play path starts without `NV_SAVE_OUT`, and mods' settings must last between sessions there as on the launcher's Windows path.
- Evidence: `mod_files::tests::the_native_build_keeps_mod_files_beside_its_own_saves` (host) and `fork_platform::tests::mod_files_on_a_unix_host_read_the_game_and_keep_writes_in_the_ports_folder` (engine-platform).

### Profile writes of the game's own settings
- Files: `crates/engine-platform/src/ini.rs` (`write_private_profile_string_a` and its tests), `crates/engine-platform/src/files.rs` (`write_whole`, and `directory_exists` visible in the crate)
- Upstream approach: `WritePrivateProfileStringA` answers 1 and writes nothing, so the choices the game saves from its Settings menu to `FalloutPrefs.ini` are gone at the next start (checked on `upstream/main` at 72037d26, 2026-11-17)
- Fork approach: the call edits the run's copy of the file with the measured text rules of `engine_core::mod_files` (`written_profile`, `removed_key`, `removed_section`) and `files::write_whole`, which takes the player's file into the run and stamps it, so `NV_PREFS_OUT` copies it out; the player's file is never written. Based on Lhomer's nv-rs commit 8f4cc9ba (GPL-3.0, https://github.com/Lhomer/nv-rs/commit/8f4cc9ba7fa66fe5855a828150343ec86e5ef60b)
- Decision: drop when upstream writes the run's copy of the file itself
- Reason: a setting changed in the game's own menu must still be set at the next start
- Evidence: the tests in `ini.rs` (`a_written_value_replaces_the_key_in_place`, `a_new_key_follows_its_sections_last_key`, `a_new_section_is_appended`, `a_null_value_deletes_the_key_and_a_null_key_the_section`, `a_missing_file_in_an_existing_folder_is_made`, `a_profile_write_is_read_back_and_stamped`, `the_players_file_is_never_written`)

## Older table

| Fork-only change | Why | Upstream change that makes it unnecessary |
|---|---|---|
| `engine-platform` answers in `fork_platform.rs`: file times of FindFirstFile, known folders, data folder (memory status is upstream's `machine_table` since slaterain/nv-rs#645); hooks in `files.rs`, `dsound.rs`, `lib.rs`, `host/save_out.rs` (and its `NV_SAVE_OUT` check, which takes a path from `/` off Windows), `host/lib.rs` | The game runs on Linux; upstream answers these only on Windows | Upstream implementing the same calls off Windows (it does not as of the 38-commit sync of 2026-11-15) |
| `engine-platform/src/fork_sound.rs`, hooks in `sound_out.rs`, `dsound.rs`, `lib.rs`, `Cargo.toml` | The game's sound output off Windows: libpulse-simple (PipeWire and PulseAudio) or a null sink behind the mixer's `sound_out` seam; upstream's output is WASAPI only (checked 2026-11-15: no audio work on `slaterain/nv-rs` main) | Upstream giving `sound_out` a non-Windows device |
| `engine-platform/src/files/files_host.rs`, `files.rs` (`mod files_host`), `host/cell_offsets.rs` | Game paths (backslashes) map to Linux paths | A path layer in upstream |
| `engine-platform/Cargo.toml`, `d3d9device.rs`, `d3d9resource.rs` | Native Linux build: cfg gates and d3d9-wgpu on every target | Upstream building these on Linux |
| `d3d9-wgpu/tests/frame.rs` | 8x multisampling tests ignored off Windows (lavapipe offers 4x; #198) | Upstream skipping them when unsupported |
| `host/src/lib.rs` (`not_provided_here` and its tests) | Upstream refuses to start the game on every system but Windows (slaterain/nv-rs#617); the fork runs it on Linux, so the refusal is cfg-gated to systems other than Windows and Linux | Upstream letting Linux start the game |
| `engine-core/src/engine.rs`, `typed/diff.rs`, `x87.rs`, `units/crt.rs` (test builds only) | Test speed (CI.md, "Upstream offers") | Upstream adopting the per-thread template and the control-word note |
| `engine-core/src/lib.rs`, `engine-havok/src/lib.rs`, `host/src/lib.rs`: appended `test_gate` | Real-data gate (TESTING.md); not overlay paths because upstream adds `mod` lines there | Upstream gating real-data tests itself |
| Overlay: `exe.rs`, `units/crt/{cvt,fdiv,math,printf,trig}.rs`, `hkalookatiksolver.rs`, `nvse/commands/math.rs`, `host/native_inflate.rs` | Test-file edits: real-data gate, math test split, tan/tanh ignored off Windows | Upstream splitting or gating those tests |
| `host/Cargo.toml`, `Cargo.toml`, `Cargo.lock`, `host/src/lib.rs`, `nvse/src/plugins.rs`, crates `jg` and `showoff` | Built-in JohnnyGuitar NVSE and ShowOff xNVSE identities and co-save | Upstream registering those plugins (it has not; JIP LN, UI Organizer and xNVSE are upstream's) |
| `d3d9-wgpu/src/window.rs`: `choose_present_mode`, `present_mode`, `refresh_interval` and the display log line | Present mode by `NV_PRESENT` (Mailbox by default off Windows) and the display's refresh as the pacing interval; checked 2026-11-15: upstream has no display or present-mode choice off Windows | Upstream choosing the present mode and the refresh itself |
| `crates/fork-presence` (new), `viewer/src/main.rs` (two hooks), `viewer/Cargo.toml`, `Cargo.toml`, `Cargo.lock` | Discord Rich Presence while the game runs (docs/fork/DISCORD.md); upstream has none (checked 2026-11-15) | Upstream adding presence |
| `engine-platform/src/d3d9backend.rs`, `d3dx.rs`, `d3d9real.rs` and `d3d9real/win32.rs` (start-up and window placement lines): messages through `fork_log` | The real runtime's and D3DX's messages follow the terminal policy (LOGGING.md): a level and the target `d3d9`, the fatal ones always printed | Upstream adopting a leveled logger |
| `d3d9-wgpu/src/fixed.rs` (new), hooks in `device.rs`, `lib.rs`, a test in `tests/frame.rs` | A draw with no shaders of pre-transformed vertices (`D3DFVF_XYZRHW`, TEX1, DIFFUSE), which the Bink movie player uses for every frame, was skipped: the screen stayed blank during a movie | Upstream providing the fixed-function path |
| `d3d9-wgpu/src/cache.rs` (`max_bytes`, `evict_blobs`, `trim_list`), a hook in `device.rs` | A size cap for the pipeline folder (`NV_PIPELINE_CACHE_MAX_MB`, default 256): other adapters' driver cache files and the oldest recorded pipeline lines go first, so the folder does not grow without bound (SHADER-CACHE.md); the eviction line goes through `fork_log` (`info`, target `d3d9`) | Upstream capping its pipeline cache |
| `d3d9-wgpu/src/budget.rs` (new), hooks in `device.rs`, `lib.rs`, `window.rs`, a test in `tests/frame.rs` | A budget for the video memory the layer makes (`NV_GPU_BUDGET_MB`, default 80 % of the first DRM card's VRAM): a texture or buffer past it, or a texture side over 16384, is refused with a `stop:` line, and a window client area under 16 pixels a side gets no swap chain; the amdgpu driver ran out of memory once and took the compositor with it | The game goes on without the object; no upstream equivalent |
| `host/src/fixes.rs`, `docs/PLAYTESTING.md`: `NV_FPS_LIMIT` | A plain name for the play frame limit (`iFrameLimit`, 0: unlimited) that the Linux docs use; `NV_FRAME_LIMIT` is unchanged and wins when both are set | Upstream naming the variable itself |
| `README.md` | Fork banner | None; fork-owned text |

## Redundancy check after the second sync of 2026-11-15

The sync after `cc26637b` brought in 7 upstream commits (JIP LN lane B3, JIP game patches, UI Organizer part 2, the pad's
dead zones, conditions on AI threads, ash piles, split beams). They touch `crates/jip`, `crates/nvse`, `crates/uio` (a new
crate), `crates/engine-core`, `crates/engine-typed` and `crates/host` (the drawing thread). None of them touches audio
output, the window, pointer handling, the present mode, the path layer or the `d3d9-wgpu` Linux build, so no fork-only
code in the table above became redundant. The release-files tool's lock file needed the new `uio` crate.
