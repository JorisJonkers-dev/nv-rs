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

### Short names on Windows
- Files: `crates/engine-platform/src/files.rs` (`search_finds_with_short_names`, the soft stops of `find_first_wild`; the fork's save tests in `tests_changed_directories`)
- Upstream approach: since slaterain/nv-rs#712 and slaterain/nv-rs#718 a search off Windows matches long names only (`search_finds_long_names`, with Windows' `*.*` and trailing-dot rewrites) and the run's files have no short names there (`alternate_name`, `short_name_off_windows`), which is what the fork did off Windows: the fork's own arms were dropped for upstream's in the sync to 1b438957. On Windows upstream's matcher rules out a short name by its first letter only and stops on the rest
- Fork approach: on Windows the first two letters of a short name rule it out (`ClassicPack*.bsa` does not stop on `CaravanPack - Main.bsa`); upstream's test of that pattern was changed to expect no stop there and a stop for a pattern whose first two letters agree. A changed folder's `.` or `..` on Windows goes through the fork's soft stop (fork_log) before upstream's stop
- Decision: drop when upstream's Windows matcher rules out a short name by two letters
- Reason: wildcard searches for a master's archives (`<name>*.bsa`) in a folder that holds an archive whose long name has spaces stopped the game in its first frame, once per pattern the short-name rules could not decide
- Evidence: JorisJonkers-dev/nv-rs#331; `the_data_folder_pack_searches_find_long_names_only`, the save tests in `tests_changed_directories` and `crates/host/tests/linux_saves.rs`

### Machine table off Windows
- Files: `crates/host/src/lib.rs` (`adapter_table_text`, `display_table_text`, `load_adapter_table`), `crates/host/src/game_ini.rs`, `crates/engine-platform/src/dsound.rs`, `crates/engine-platform/src/fork_tables.rs` and `fork_d3d9rec.rs` (fork-owned)
- Upstream approach: since slaterain/nv-rs#681 the table off Windows has a display and card part made after the renderer choice (`machine_table::this_display`: winit's monitors, the card from wgpu with a modern card's Direct3D 9 answers captured on Windows 11, the card's sample counts and float blending, a start refused without DXT), DirectSound's fixed answers captured on Windows 11 (`dsound::fixed_table`, a device when the speakers have one), and the game's Documents in `<own folder>/game/Documents`. Since slaterain/nv-rs#687 a play run's first start writes `Fallout.ini` and `FalloutPrefs.ini` from the game's defaults and High preset (`first_profile.rs`)
- Fork approach: the display and the card stay in the fork's adapter table (`fork_tables::adapter_table`: fork-window's monitor, wgpu's identity, the reference Shader Model 3 answers and the profile file's overrides), so `display_table_text` adds nothing; `fork_d3d9rec.rs` answers `monitors()` from fork-window for other callers. The sound table without a file is upstream's captured one (the fork's hand-written reference and no-device tables were removed), with a device only when the output is asked for (`NV_SOUND`), so a run without it stays silent. `Fallout.ini` is read from the fork's Documents (`fork_platform::known_folders`), where the fork's table puts the game's folders. In a play run upstream's first start steps run after `fork_tables::seed_machine_settings`, in the fork's Documents: they add nv-rs's sections and the pending choices to `Fallout.ini`, and in a `FalloutPrefs.ini` the fork made they set only the play lines (`sD3DDevice` from the table's card, `[nv-rs] bWindowSizeChecked=1`); its size keys stay the fork's
- Decision: keep the display and card part until upstream's display part can read a window layer that is not winit (then take its captured card answers and its DXT refusal); the sound table and the Documents folder follow upstream's where it covers them
- Reason: upstream's display part reads winit's monitors, and the fork's window layer off Windows is fork-window (see the next record); the fork's Linux runs already keep their saves and settings under `<data_dir>/userdata`
- Evidence: `fork_tables::tests::the_layer_loads_the_generated_adapter_table`, `dsound::tests::the_fixed_table_answers_the_start_up`, `tests::the_settings_reading_leaves_the_renderer_choice_to_the_start` (host)

### Direct3D 9 window layer off Windows
- Files: `crates/engine-platform/src/d3d9real.rs`, `crates/engine-platform/src/d3d9real/elsewhere.rs` (deleted), `crates/engine-platform/src/fork_d3d9rec.rs` (fork-owned)
- Files (since the sync to a489e36c): also `crates/d3d9-wgpu/src/window.rs` and `device.rs` (Linux arms), `crates/engine-platform/Cargo.toml` (no winit)
- Files (since the sync to 1b438957): `crates/engine-platform/Cargo.toml` also without `xkbcommon-dl` and `raw-window-metal`; `fork_d3d9rec.rs` answers upstream's `monitors` and `run_on_main_thread` (slaterain/nv-rs#681, slaterain/nv-rs#694)
- Upstream approach: since slaterain/nv-rs#635, `d3d9real.rs` builds on every system and the Win32 window lives in `d3d9real/win32.rs`. Since slaterain/nv-rs#640 the window off Windows is upstream's winit layer (`d3d9real/winit_window.rs`, X11 and Wayland; macOS refused until a host hands it the main thread), its keys and mouse through `d3d9real/input.rs`, and d3d9-wgpu draws in a window the layer registers (`GameWindow`, `register_window`). The real Direct3D 9 parts are `cfg(windows)`, and asking for it off Windows ends the run.
- Fork approach: off Windows the window layer is `fork_d3d9rec.rs`: a fork-window window on X11 or Wayland (off screen without a display or with `NV_D3D9_OFFSCREEN`), its keys and mouse as the game's input, and the wgpu stream with the shader cache; `real` and `real,wgpu` draw on wgpu with a line instead of ending the run. Upstream's split, its `RealDevice` and `QueryBoard` types and its `cfg(windows)` placement replace the fork's own `cfg(windows)` lines and three stand-ins (the device alias, the shared device and `query_without_waiting`).
- Decision: keep. Upstream's winit layer now draws the game (slaterain/nv-rs#640) but is not equal: it has no off-screen mode (CI, tests and the replay-bench draw on lavapipe without a display), no `NV_D3D9_OFFSCREEN`, and the fork's display choice and refresh pacing come from fork-window. This build does not compile `winit_window.rs` (nor depend on winit); `input.rs` is built with the Win32 layer. Upstream's macOS window on the main thread (slaterain/nv-rs#694) is part of that layer: here `run_on_main_thread` runs the game on the calling thread, as fork-window needs no main thread. When wgpu finds no hardware card, off Windows the fork still draws on any adapter (lavapipe; upstream ends the run there); when wgpu cannot make the game's device, the run ends with upstream's line. Compare again when upstream's layer can draw off screen; then take upstream's where it is equal.
- Reason: the fork runs the game on Linux; upstream's layer refuses a window there.
- Evidence: `d3d9real::fork::tests::a_guest_program_draws_a_frame_through_the_recorder_on_wgpu` (a frame drawn on lavapipe in the Linux job), and the Linux play path in docs/fork/LINUX.md.

### Mods' own files on the native build
- Files: `crates/host/src/mod_files.rs`, `crates/host/src/save_out.rs` (`known_folders`) and `fork_platform.rs` (fork-owned)
- Upstream approach: since slaterain/nv-rs#629, mods' own files are kept in `NV_MOD_FILES_OUT`, else beside `NV_SAVE_OUT`'s folder; with neither there is none and their settings last for the session only. Any folder inside Documents or AppData is refused.
- Fork approach: off Windows the native build keeps its saves in the port's own Documents when `NV_SAVE_OUT` is unset (`save_out::native_saves_dir`), so with neither variable set mods' files go beside those saves (`<documents>/My Games/FalloutNV/nv-rs mod files`). That Documents is the port's, so only the game-folder and path checks apply. A set variable goes through upstream's rules unchanged (off Windows the save guard's rule, `folder_from_with`, since slaterain/nv-rs#712). The folder's copy of a name is found without regard to case by upstream's `host_paths::host_path_under` since slaterain/nv-rs#697, so the fork's `files_host.rs` was removed in the sync to 1b438957. `save_out::known_folders` off Windows still answers the port's own Documents for this (upstream's answers none). Upstream marks its Windows-path `mod_files` tests Windows only (slaterain/nv-rs#660), and a fork test covers the Unix path.
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
| `engine-platform` answers in `fork_platform.rs`: known folders, data folder (memory status is upstream's `machine_table` since slaterain/nv-rs#645; the file times of FindFirstFile off Windows are upstream's `files.rs` since slaterain/nv-rs#633, and `filetime`, `creation` and `find_fields` are no longer called by upstream's files); hooks in `dsound.rs`, `lib.rs`, `host/save_out.rs` (the native build's own Saves folder when `NV_SAVE_OUT` is unset, `known_folders` off Windows), `host/lib.rs`, `host/game_ini.rs` | The fork keeps the game's folders, tables and saves under `<data_dir>`; upstream's machine table (slaterain/nv-rs#681, slaterain/nv-rs#687) puts them in its own folder (`<own folder>/game`) | Upstream's own folder becoming the fork's (then the fork's folders and the hooks go) |
| `engine-platform/src/fork_sound.rs`, hooks in `sound_out.rs`, `lib.rs`, `Cargo.toml` (the sound table off Windows: "Machine table off Windows") | The game's sound output on Linux: libpulse (PipeWire and PulseAudio) loaded at run time, or a null sink, behind the mixer's `sound_out` seam. Upstream's cpal device (slaterain/nv-rs#637, ALSA on Linux) is built for macOS only here: with it the Linux program needs `libasound.so.2` to start at all, and it has no null sink for headless runs | Upstream's device starting without ALSA's library and offering a sink for headless runs |
| `d3d9-wgpu/tests/frame.rs` | 8x multisampling tests ignored off Windows (lavapipe offers 4x; #198) | Upstream skipping them when unsupported |
| `engine-core/src/engine.rs`, `typed/diff.rs`, `x87.rs`, `units/crt.rs` (test builds only) | Test speed (CI.md, "Upstream offers") | Upstream adopting the per-thread template and the control-word note |
| `host/src/lib.rs`: a clippy allow on `the_expected_stop_off_windows_has_its_own_status` | Its `Ok` arm asserts a constant on Linux, which `clippy -D warnings` refuses; upstream's Linux check does not run clippy on tests | Upstream allowing or rewriting it |
| `engine-core/src/lib.rs`, `engine-havok/src/lib.rs`, `host/src/lib.rs`: appended `test_gate` | Real-data gate (TESTING.md); not overlay paths because upstream adds `mod` lines there | Upstream gating real-data tests itself |
| Overlay: `exe.rs`, `units/crt/{cvt,fdiv,math,printf,trig}.rs`, `hkalookatiksolver.rs`, `nvse/commands/math.rs`, `host/native_inflate.rs` | Test-file edits: real-data gate, math test split, tan/tanh ignored off Windows. Upstream's changes to these are applied to the fork's copies by hand (in the sync to 1b438957: slaterain/nv-rs#706's soft x87 bounds in `fdiv.rs`, `math.rs`, `trig.rs`) | Upstream splitting or gating those tests |
| `host/Cargo.toml`, `Cargo.toml`, `Cargo.lock`, `host/src/lib.rs`, `nvse/src/plugins.rs`, crates `jg` and `showoff` | Built-in JohnnyGuitar NVSE and ShowOff xNVSE identities and co-save | Upstream registering those plugins (it has not; JIP LN, UI Organizer and xNVSE are upstream's) |
| `d3d9-wgpu/src/window.rs`: `choose_present_mode`, `present_mode`, `refresh_interval` and the display log line | Present mode by `NV_PRESENT` (Mailbox by default off Windows) and the display's refresh as the pacing interval; checked 2026-11-15: upstream has no display or present-mode choice off Windows | Upstream choosing the present mode and the refresh itself |
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
