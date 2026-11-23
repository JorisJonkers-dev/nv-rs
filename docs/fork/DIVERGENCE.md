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

### The in-game graphics overlay
- Files: `Cargo.toml` (workspace member `fork-overlay`), `crates/d3d9-wgpu/Cargo.toml` (`fork-overlay`, `fork-gfx`), `crates/d3d9-wgpu/src/lib.rs` (`pub mod gfx_overlay;`, a fork-only file), `crates/d3d9-wgpu/src/bitmap_font.rs` (`ascii()`), `crates/d3d9-wgpu/src/frame_rate.rs` (`window_device_present()`), `crates/d3d9-wgpu/src/device.rs` (`draw_gfx_overlay` at the present, after the window's capture and before the counter), `crates/engine-platform/src/d3d9real/win32.rs` and `input.rs` (F10, and keys going down while the overlay is open, go to `gfx_overlay::key`)
- Upstream approach: upstream has no settings overlay; its only overlay is the F11 frame-rate counter drawn with the bitmap font at the present (`frame_rate.rs`), whose key is taken the same way; checked at the base
- Fork approach: the graphics overlay (ADR-0019 as amended for nv-rs#730) is an egui panel from the graphics modules' schemas, drawn by egui-wgpu at the same point as the counter; its font is made from the counter's glyphs; closed, only F10 is taken; open, keys going down are its own and keys coming up still reach the game
- Decision: keep; offer nothing (the overlay edits fork-only settings)
- Reason: the maintainer's graphics modules (nv-rs#591, nv-rs#592) need an overlay that switches and tunes them in the game
- Evidence: `crates/d3d9-wgpu/tests/gfx_overlay.rs` (the panel's picture, the keys, the file written), `text_is_laid_out_in_whole_cells`, `the_keyboard_turns_a_setting_on`; the standard views with the overlay closed against the build before, mean 0.000 (pull request)

### Graphics settings for the port's own effects
- Files: `crates/d3d9-wgpu/src/scene_light.rs` (`Quality`, `set_quality`, `shader_at`, `set_live_strength`), `crates/d3d9-wgpu/src/sun_shadow.rs` (`MAP_SIZE`, `set_map_size`, `map_size`), `crates/d3d9-wgpu/src/device.rs` (the step takes the live strength and is made again at another quality), `crates/host/src/lib.rs` (`fork_graphics::start` before the rows are installed), `crates/host/Cargo.toml` (`fork-gfx`)
- Upstream approach: upstream switches ambient occlusion and sun shadows once at start (`bAmbientOcclusion`, `fAmbientOcclusionStrength`, `bSunShadows`) and fixes their quality by constants (`SLICES`, `STEPS`, `SIZE`); checked in the files above at the base
- Fork approach: the graphics settings (ADR-0019, nv-rs#592) set the same switches from the profile's `nv-rs-graphics.toml`, a preset or `NV_GFX`, and choose a level: the occlusion's search (low 1 x 4, medium 2 x 4 as before, high 3 x 6, changeable while the game runs, with its strength) and the shadow maps' size (1024, 2048 as before, 4096, at start). With no settings, nothing changes: the defaults are upstream's constants
- Decision: keep; drop the level code when upstream offers levels of its own
- Reason: the maintainer's graphics modules (nv-rs#591) need quality levels and live changes of the effects the engine has
- Evidence: `quality_levels_change_only_the_search`, `a_live_strength_is_clamped_and_can_be_taken_back`, `the_map_size_is_one_of_the_three_levels`, `every_map_extent_follows_the_run_map_size`, `host::fork_graphics` tests; the standard views with no settings against the build before, difference 0 (pull request)

### The light balance: the game's ambient share
- Files: `crates/engine-fallout-env/src/units/fallout_shared_sky/sun.rs` (`set_light_colors` takes the share; `AmbientShare`, `AmbientShares`, `want_ambient_share`), `crates/host/src/lib.rs` (`want_ambient_share` with `fork_graphics::ambient_shares` at start)
- Upstream approach: the sun light takes the sky's ambient colour (the weather's outdoors, the cell's indoors) whole every frame, as the exe does; checked in `sun.rs` at the base
- Fork approach: the graphics settings' `light.ambient_exterior` and `light.ambient_interior` (nv-rs#768) hand the light that share of the sky's ambient colour; the sky's own colours are not changed. At 1 (the default and the `original` preset) the code is the exe's, call for call
- Decision: keep; offer upstream the seam if it wants a fill-light setting for its own lighting
- Reason: the maintainer's `modern` preset (nv-rs#591) adds shade and the light from the sky; the game's flat fill light on top of it washes the picture out
- Evidence: `the_ambient_share_at_one_is_the_exes_code`, `the_ambient_share_scales_the_lights_ambient_only`, `an_ambient_share_that_is_not_usable_counts_as_one` (`sun.rs`), `the_light_balance_reaches_the_sun_lights_shares` (`host::fork_graphics`); the standard views under `original` against the build before, difference 0 (pull request)

### Far-object block with several shapes
- Files: `crates/engine-fallout-env/src/units/fallout_shared_distant_terrain_system/bgsdistantobjectblock.rs` (`prepared_meshes`, `prepare_mesh`), `crates/engine-fallout-env/src/units/fallout_shared_distant_terrain_system/bgsterrainnode.rs` (`later_block_shapes`, `bgs_terrain_node_update_block_visibility`)
- Upstream approach: upstream does nothing here; the exe expects one shape per block file: the load task (`006f71e0`) keeps child 0 of the file's root node as the block's mesh, the block's prepare (`006f66e0`) gives only that mesh its shader and registers it with the renderer, and `BGSTerrainNode::UpdateBlockVisibility` (`006fee90`) sets the segments of only that mesh's segmented shape. A segmented shape loads with every segment off, so any later shape draws nothing
- Fork approach: every leaf child of the root node is prepared, and has its segments set, the same way as the first
- Decision: keep; the exe's rule gives a file with several shapes only its first shape
- Reason: far-object files made by xLODGen and lodwright hold one shape per atlas texture (or more, split at the 16-bit limits); the later shapes were not drawn, so the Strip's towers, in the third shape of one block, were missing
- Evidence: `prepare_reaches_every_shape_of_the_root_node`, `block_visibility_reaches_every_shape_of_the_block`; a modded profile with an xLODGen far-object mod draws the Strip's towers after the change and not before; the plain game's pictures at three views are unchanged (difference 0)

### Modern DDS files (DX10 header, BC7)
- Files: `crates/d3dx-tex/src/lib.rs`, `load.rs`, `fork_modern_dds.rs` (new), `crates/d3d9-wgpu/src/format.rs`, `crates/engine-platform/src/d3dx.rs`, `d3d9.rs`, `d3d9device.rs`
- Upstream approach: the D3DX translation refuses every DDS whose FourCC the 2010 library does not know (`DX10`, `ATI1`, `ATI2`), as the DLL does (`D3DXERR_INVALIDDATA`); the Direct3D 9 layer has no BC7 format
- Fork approach: with `NV_DDS_MODERN=1`, and only while wgpu draws, those files are read: the blocks of BC4, BC5 and BC7 (with or without the sRGB flag) go to the device as they are, with the file's own levels, as `ATI1`, `ATI2` and the private FourCC `BC7U` (wgpu's `Bc7RgbaUnorm`); the layer's format check and block sizes know them. Without the option nothing differs from upstream
- Decision: keep; drop when upstream reads DX10-header files
- Reason: remastered and modern texture sets ship BC7 and DX10 headers; the game cannot show them otherwise
- Evidence: nv-rs#750; `fork_modern_dds::tests`, `d3dx::tests_modern_dds`, `frame::a_bc7_texture_is_sampled_as_the_block_decodes`

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
- Decision: keep. Upstream's winit layer now draws the game (slaterain/nv-rs#640) but is not equal: it has no off-screen mode (CI, tests and the replay-bench draw on lavapipe without a display), no `NV_D3D9_OFFSCREEN`, and the fork's display choice and refresh pacing come from fork-window. This build does not compile `winit_window.rs` (nor depend on winit); `input.rs` is built with the Win32 layer. Upstream's macOS window on the main thread (slaterain/nv-rs#694) is part of that layer. Here fork-window has its own macOS module (nv-rs#379): on macOS `run_on_main_thread` gives the main thread to fork-window's loop (AppKit runs windows there only) and the game to a thread of its own, each window gets a `CAMetalLayer` made on that thread, and `fork_d3d9rec.rs` registers the window with d3d9-wgpu's window registry (`register_window`), whose macOS arm makes the surface on that layer; on Linux `run_on_main_thread` still runs the game on the calling thread. When wgpu finds no hardware card, off Windows the fork still draws on any adapter (lavapipe; upstream ends the run there); when wgpu cannot make the game's device, the run ends with upstream's line. Compare again when upstream's layer can draw off screen; then take upstream's where it is equal.
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

### Toolchain
- Files: `rust-toolchain.toml`, the `rust-version` of `Cargo.toml`, `crates/game-profile/Cargo.toml` and the research crates (`research/nv-oracle`, `research/nv-oracle/nv-pure`, `research/nv-trace`); `.cargo/config.toml` and `renovate.json` (fork-owned)
- Upstream approach: Rust 1.99.1 (`rust-toolchain.toml` on `origin/main`, 1b438957); `rust-version` 1.75 for the workspace, 1.88 for game-profile, 1.82 and 1.87 for the research crates
- Fork approach: Rust 1.100.0 and `rust-version = "1.100"` everywhere outside `vendor/` (the vendored wgpu-hal keeps its published 1.87). Cargo 1.100 reads `[registry] global-min-publish-age = "7 days"` (`.cargo/config.toml`), so a crate version younger than a week is not taken on resolution; Renovate waits 7 days for crates too. CI installs the toolchain `rust-toolchain.toml` names (`rustup toolchain install`). Clippy 1.100's ten new lints (`manual_c_str_literals` in Windows-only code, `manual_is_multiple_of`, `chunks_exact_to_as_chunks`, `unnecessary_map_or`, `needless_bool`, `approx_constant`, `manual_repeat_n`, `needless_range_loop`, `manual_noop_waker`, `manual_isolate_lowest_one`), about 300 places in upstream's code, are allowed in `.cargo/config.toml` (`rustflags`), so no upstream file changes for them; `approx_constant` there is the game's own constants written as the binary has them
- Decision: keep; drop the `rust-toolchain.toml` and `rust-version` lines when upstream moves to 1.100 or later, and each allowed lint when upstream's code meets it
- Reason: a supply-chain cooldown for new crate versions, which needs Cargo 1.100
- Evidence: `cargo build` and `cargo clippy --workspace --all-targets -D warnings` on 1.100.0 in the pull request that made the change

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

### Holder tests ask for a tenth of the wait as processor time
- Files: `crates/engine-core/src/real_threads.rs` (the tests `main_s_long_wait_for_the_kernel_lock_names_the_holder` and `main_s_long_wait_for_a_section_names_its_owner`)
- Upstream approach: the tests assert that the named holder thread used at least half of main's wait as processor time
- Fork approach: the same tests, with a tenth; the holder is still named (call, thread, start, wait) and, where its processor time can be read, must have worked (a sleeping or blocked holder has none); the clock table is keyed by run and thread, so a thread of another test with the same key can take the entry over and make the time unreadable on a loaded machine
- Decision: keep; drop when upstream's tests no longer depend on the machine's load
- Reason: upstream's half fails on a loaded two-core machine (the coverage job, `taskset -c 0,1`: 78 ms of 252 ms), whatever the code does
- Evidence: the two tests, run on two cores (failed before, pass after) and on all cores

### Typed family summary in the engine crate doc
- Files: `docs/ENGINE_CRATE.md` (the "Typed code" paragraph's "by default" enumeration)
- Upstream approach: the summary names 22 typed families on by default; it omits five that the code registers and turns on by default; checked against the code in this worktree
- Fork approach: the enumeration also names `scripts`, `hud`, `script_lines`, `facegen` and `save`, on after their tests and reviews, with no gate dates or measurements added
- Decision: keep; drop this record when upstream's summary includes the five families
- Reason: the summary must match the typed families the code registers and has on by default
- Evidence: `crates/engine-typed/src/lib.rs` registers `scripts`, `hud`, `script_lines`, `facegen` and `save` and has them on by default (past their gate, on with no `NV_TYPED` switch)
