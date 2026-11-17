# Native Linux build: what the fork changes

Goal of this stage (M0 and M1 of the native Linux plan): the whole workspace compiles on
`x86_64-unknown-linux-gnu` and the portable logic is tested there. The window, input, sound output
and the real Direct3D 9 / wgpu device stay Windows-only; nothing here makes the game run on Linux.

How it is verified: the fork's CI, through `.github/workflows/linux-probe.yml`, which runs only on
pushes to `probe/**` branches: `cargo check --locked --workspace --all-targets`, then
`cargo test --locked -p <crate>` for every crate (the engine crate in a job of its own), with a
table of results in the job summary. The Pipeline's Linux jobs run the crates in `.fork/linux-ok.txt`.

Every change below is `cfg`-gated or additive, so Windows code is unchanged. Upstream-owned files
changed are listed in `.fork/allowed-divergence`; the Pipeline's `Verify sync` job fails a pull request that changes an upstream-owned file that is not listed there. The "Offer upstream" column below is history: upstream is an imported source and nothing is offered back. Upstream's issue #268 and CONTRIBUTING reserve
the platform layer (`d3d9real.rs`, the call sites in `d3d9device.rs`, `d3d9resource.rs`,
`d3d9query.rs`, `d3d9.rs`, `window.rs`): none of those call sites and none of `d3d9real.rs` is
touched.

## Changes

| # | Change | Files | Why | Reserved area | Offer upstream as is? | Risk to Windows |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | `d3d9-wgpu` is a dependency of `engine-platform` on every target (it was `cfg(windows)` only) | `crates/engine-platform/Cargo.toml` | `end_process` calls `d3d9_wgpu::loading::finish_output()` unconditionally: this one line stopped `engine-platform`, so `engine`, `host`, `play-patch` and `nv-viewer` | no | yes, one-line move | none: the dependency was already there on Windows; `Cargo.lock` unchanged |
| 2 | The start screen's output capture (`Pipe`, `CAPTURED`) is built on Windows only | `crates/d3d9-wgpu/src/loading.rs` | dead code warnings off Windows (the capture is `cfg(windows)`) | no | yes | none |
| 3 | The argument-word trait `Word` and its import are built on Windows only | `d3d9device.rs`, `d3d9resource.rs` | used only inside `#[cfg(windows)]` code of the `forwarded!` macro; an unused warning off Windows | touches the files, not a call site | yes, but wait for the seam: the files are being retuned | none |
| 4 | An unused binding off Windows, a warning-only attribute and a test constant used only by Windows tests | `d3dx.rs`, `files.rs` (two attributes) | warnings (the Linux clippy job is `-D warnings`) | no | yes | none |
| 5 | `files::host_path` maps a game path: on Windows the path as it is, on Unix `Z:` is the host root (as Wine's), `\` separates, names match without regard to case in their directory; other drive letters have no mapping and are not found | `files/files_host.rs` (new, overlay), two lines in `files.rs` (`mod files_host;` and `pub use files_host::host_path;`), `host/src/cell_offsets.rs` (uses it) | the game's paths are Windows paths; three `host` tests failed on Linux for this reason. The code was in `files.rs` (+149 lines in a file upstream changes often, which stopped the replay of upstream `f97accc25bb4`); it is now in a file upstream does not have, and `files.rs` keeps four lines | no | no (the fork does not offer changes upstream) | none: `host_path` returns upstream's `real_path` unchanged on Windows, and a test of it runs there |
| 6 | Tests that can only pass on Windows: `nvse`'s bit-for-bit math comparison is `#[cfg_attr(not(windows), ignore = "reason")]` (tan and tanh use the host's libm; the game's differ in the last bit: `dTan90`, `dTanh30`); the five `files` tests that need Windows file times or Windows paths are skipped on Linux by name in `.fork/linux-skip.txt`, applied by `scripts/fork/run-component.sh` | `nvse/src/commands/math.rs`, `.fork/linux-skip.txt` | honest failures off Windows, with no attribute in `files.rs` | no | no | none |
| 7 | The probe workflow and its summary script | `.github/workflows/linux-probe.yml`, `scripts/fork/linux_test_summary.py` | CI loop without starting the Windows jobs | fork only | no | none |

Not changed, on purpose: the sound output is as the native Linux plan describes it (the shader compiler: see "Shader compiler off Windows" below).

## Platform answers off Windows

The game's first calls on Linux no longer stop the run. The code is fork-owned and additive:
`crates/engine-platform/src/fork_platform.rs` (in
`.fork/overlay-paths`), reached by one `cfg(unix)` hook each. The Windows arms are the code that was there; the
Windows jobs run the same tests as before (the four `files` tests that were `ignore`d off Windows run on Linux too).

| Question | Windows | Linux |
| --- | --- | --- |
| File times of `FindFirstFileA` / `FindNextFileA` (`files.rs`, `write_find_data`) | the host's own search | the file's own metadata as `FILETIME`: last write, last access, and the birth time from `statx` (`STATX_BTIME`, through the standard library); on a file system that keeps no birth time, the older of the last write and the last status change. Attributes: directory, else archive (as `GetFileAttributesA` answers). A time the file system cannot give stops the run naming the file |
| `GlobalMemoryStatus(Ex)` (`process.rs`) | `GlobalMemoryStatusEx` | upstream's `machine_table::this_memory` (slaterain/nv-rs#645): `/proc/meminfo` on Linux, `sysctl` on macOS; the fork's own parser is retired |
| Real clock's start (`Engine::start_real_clock`) | `GetTickCount` | upstream's `host_tick_count`: the machine's uptime in ms from `CLOCK_BOOTTIME`, wrapped to 32 bits as `GetTickCount` wraps |
| Table paths (`dsound.tsv`, `d3d9_adapter.tsv`, `d3dx-cache`) | `%LOCALAPPDATA%\nv-rs` | `$XDG_DATA_HOME/nv-rs` (an absolute path), else `~/.local/share/nv-rs`; the `NV_*` overrides are unchanged |
| First-run settings (`fork_tables::seed_settings`) | the launcher makes `Fallout.ini` and `FalloutPrefs.ini` | `<userdata>/Documents/My Games/FalloutNV`: `Fallout.ini` from the installation's `Fallout_default.ini`, `FalloutPrefs.ini` with `[Display] sD3DDevice` = the adapter's name; files that exist are never touched; the table's folders are `Z:\...` game paths and every file call goes through `files::host_path` |
| `host::save_out::known_folders()` | `SHGetFolderPathW` | `<data folder>/nv-rs/userdata/{Documents,AppData/Roaming,AppData/Local}` (not created) |

Code pages beyond ASCII and 1252 are not provided: a name's other characters become `?` (see `code_page`); a name
that needs one still stops no run. Tests: `fork_platform::tests` and `unix_tests` (synthetic files with chosen times
through `FindFirstFileA`, the paths under a fake `XDG_DATA_HOME`) and
the clock start is upstream's (`engine::tests`).

## Machine tables of a Linux machine (#182)

Nothing captures the adapter table on Linux, so `engine_platform::fork_tables` writes it. Decision (owner, by time to a
working game): a shared reference profile, not a per-machine capture. The default is the owner's card, an AMD Radeon
RX 7900 XTX on RADV (`1002:744c`, D3D9 driver `aticfx32.dll`, the file the game keys its AMD path on). At run time the
name, vendor id and device id come from wgpu's adapter info (`d3d9_wgpu::identity`) and the driver file from the vendor;
a software adapter (lavapipe) keeps the reference card. The capabilities and format answers are one Shader Model 3 set
written from the Direct3D 9 structures, the same for every vendor; no Windows capture is copied. Folders are under
`userdata_dir()`, where the native build's saves are. The rest of the machine is upstream's machine table
(`engine_platform::machine_table`, slaterain/nv-rs#645): the time zone from the zone file, the memory, the real `CPUID`,
the code pages and Windows' fixed answers (window styles and borders, the Windows directory), no NVIDIA library. The run's
table is the machine table followed by the fork's adapter table, made afresh each run; a later line of the same question
wins, so the fork's folders replace the machine table's. The whole table is also written where `d3d9_adapter.tsv`
belongs (git-ignored), for reading; `NV_D3D9_ADAPTER=FILE` names a whole table instead (a test's or a capture).

Another machine: `linux_profile.txt` in the data folder (or `NV_LINUX_PROFILE`), `key = value` lines: `name`,
`vendor_id`, `device_id` (hex), `driver`, `userdata`, and `mode = WxH@Hz` (first is current); an older file's
`bias_minutes` is taken and ignored. Display
modes are the reference list unless the file gives them; the current monitor is not read (no window exists at this stage).

Sound: see "Sound output" below. Without `NV_SOUND=out` (or `device`) and without `dsound.tsv`, the table is
`fork_tables::no_device_sound_table()`: `DirectSoundCreate8` fails with `DSERR_NODRIVER`, so the game runs silent.

Not yet known: what the game does on the AMD path with this table (stop lines and counts); that needs the owner's run.

## Sound output (#171)

The mixer (DirectSound's mixing, the resamplers, 3D placement) is platform-independent; only the device was
WASAPI. `engine-platform/src/fork_sound.rs` is the device off Windows, behind the same `sound_out::start` and
`device_mix` seam; the Windows code is unchanged.

- **Pulse** (the default): libpulse (threaded main loop, context, stream API) loaded with `dlopen` (`libpulse.so.0`,
  or the file `NV_PULSE` names; when only `NV_PULSE_SIMPLE` is set, as the Nix package does, the `libpulse.so.0` beside
  it). Nothing is linked, so no build prerequisite. Under PipeWire the library talks to `pipewire-pulse`; under
  PulseAudio to the server. Prerequisite at run time: Arch `libpulse`, Debian and Ubuntu `libpulse0`, Fedora
  `pulseaudio-libs`.
- **Layout**: `fork_sound::mix()` probes the default sink's channel map once (`pa_context_get_server_info`,
  then `pa_context_get_sink_info_by_name`), converts it to a WAVE speaker mask and returns it, so `device_mix()` and
  `GetSpeakerConfig` follow it: stereo 0x3, quad 0x33, 5.1 0x3F (side 5.1 0x60F), 7.1 0x63F, mono 0x4. A map that is not
  exactly one of these falls back to the nearest layout whose speakers the sink has (else stereo) with one stderr line,
  and the server remixes the rest. No server: stereo. `NV_SOUND_LAYOUT=auto|stereo|5.1|7.1` overrides (`5.1` keeps a
  side-5.1 sink's side speakers); `auto` is the default. The probe is made once per process.
- **Stream** (#221): opened with `pa_stream_new` and an explicit `pa_channel_map` in WAVE order (FL FR FC LFE
  BL BR SL SR, only the mask's bits), 32-bit float at 48000 Hz, 80 ms of latency (`PA_STREAM_ADJUST_LATENCY`); the
  server resamples and remaps by position. The underflow callback counts underruns and the output thread calls
  `Source::starved(count)`; one stderr line per stream says the first happened.
- **Null**: a thread that pulls the mixer on the wall clock and drops the frames. It is the fallback when no sound
  server answers (a headless machine, CI), said once on stderr.
- `NV_SOUND_BACKEND=auto|pulse|null`: `auto` (default) is Pulse, else Null; `pulse` refuses when there is no server
  (the game is then silent); `null` is always Null.
- Output stays opt-in as on Windows: `NV_SOUND=out` (the game's pace) or `NV_SOUND=device` (the device's pace), and
  `NV_SOUND_RECORD` and the hidden-window mute work as before. With output asked for and no `dsound.tsv`, the sound
  table is `fork_tables::reference_sound_table()`: one default device with a software-only `DSCAPS`, written from
  the DirectSound headers (not captured), so `DirectSoundCreate8` succeeds. A `dsound.tsv` in the data folder wins.
- Tests: `fork_sound` unit tests run on the null sink (no device, no game data). `cargo test -p engine-platform
  fork_sound -- --ignored` plays 0.4 s of a quiet tone on the session's default sink, and 0.4 s on each channel of its
  layout in turn (logging the sink's reported map; `NV_SOUND_LAYOUT=7.1` opens an eight-channel stream). If libpulse is not on the loader path set `NV_PULSE`.

Open: the layout is not re-probed when the default sink changes mid-game (#222), the 3D path has not been listened to in a play run, and the launcher does not
set `NV_SOUND=out` yet.

## Shader compiler off Windows (#181)

The six D3DX shader imports (`D3DXCompileShader`, `D3DXGetShaderConstantTable`, `D3DXGetVertexShaderProfile`,
`D3DXGetPixelShaderProfile`, `D3DXCreateBuffer`, `D3DXAssembleShader`) are registered on every target. On Windows they
are `d3dxreal.rs`, unchanged (the player's own `d3dx9_38.dll`). Elsewhere they are `engine-platform/src/d3dxshader.rs`
over the new crate `crates/d3dx-shader` (fork-owned, additive; the only upstream-owned edits are the lines listed in
`.fork/allowed-divergence`):

1. **Cache first.** One folder per compiler identity under `$NV_D3DX_CACHE/shaders`, else
   `$XDG_CACHE_HOME/nv-rs/d3dx-cache/shaders`, else `~/.cache/nv-rs/d3dx-cache/shaders`. An entry is `<key>.bin`, the
   bytecode byte for byte; the key is SHA-256 over the kind, source, entry, target, flags, macros (name, value, order),
   the include files' names and contents and the identity. Identities are separate key spaces, tried in this order:
   `ms-*` (Microsoft's `d3dx9_38.dll`), `wine-*` (Wine's built-in d3dx9), then the rest (`vkd3d-shader ...`). Nothing
   is committed or shipped: no cache file, no shader, no compiler binary.
2. **libvkd3d-shader behind it, loaded with `dlopen`** at the first miss (`libvkd3d-shader.so.1`, or the file
   `NV_VKD3D_SHADER` names). HLSL (Shader Model 1 to 3 profiles) is compiled to d3dbc, with its constant-table
   comment, and stored under the library's identity. The library is a run-time prerequisite of a Linux build that has
   shaders to compile: Arch `vkd3d`, Debian and Ubuntu `libvkd3d-shader1`, Fedora `vkd3d`; the Nix package carries it
   (`NV_VKD3D_SHADER` set by the wrapper). It must be new enough to produce d3dbc from HLSL (checked on load; the
   development library is vkd3d-shader 1.19). `NV_D3DX_SHADER_BACKEND=off` uses the cache only.
3. **No library, no entry:** the run stops with a named error (request key, target, entry; no source text, no path) or,
   for a missing or too old library, with what to install. A source the compiler refuses answers `D3DXERR_INVALIDDATA`
   and the compiler's text in the error buffer, as D3DX does.

Not provided yet: `D3DXAssembleShader` on a cache miss (libvkd3d-shader has no assembler for d3d assembly text; a
recorded entry is served), and `ID3DXConstantTable`'s reading slots (`GetDesc`, `GetConstantDesc`, `GetConstant*`: only
`GetBufferPointer` and `GetBufferSize`, the `CTAB`, answer); both stop the run with a message that says so. The profile
calls answer `vs_3_0` and `ps_3_0`, the model's caps.

Licence check: upstream `COPYING` of vkd3d (the source of the checked build, vkd3d-shader 1.19) is the GNU
Lesser General Public License, "either version 2.1 of the License, or (at your option) any later version", and the source
headers of `libs/vkd3d-shader` say the same. LGPL-2.1-or-later may be used with a GPL-3.0-or-later program; the fork only
`dlopen`s the library, links nothing and carries none of its code or binaries. `libloading` (ISC) was already in the
dependency tree. Tests: `d3dx-shader` unit tests (key stability and sensitivity per field, cache round trip, named miss,
include contents), `d3dx-shader/tests/converter.rs` (synthetic vs_2_0, vs_3_0, ps_2_0, ps_3_0 through the d3d9-wgpu reader,
converter and naga validation) and `d3dxshader::tests`; the ones that need the library print `skipped:` without it
(`NV_REQUIRE_VKD3D=1` makes that a failure).

## Finding for the owner

`nvse`'s `tan`, `tanh` (and the degree variants) use the host's libm. Their last bit differs from
the game's on Linux (`dTan90`: port -2.2877334e7, game -2.2877332e7). A game run on Linux would
not match Windows there; a Linux port needs its own `tan` that matches the game's (x87 `fptan`).

## Results on Linux (CI, `x86_64-unknown-linux-gnu`, Rust 1.99.1)

- `cargo check --locked --workspace --all-targets`: green (all 36 crates, including `engine`,
  `engine-platform`, `host`, `nv-viewer` and `play-patch`).
- `cargo clippy --locked --workspace --all-targets -- -D warnings` and `cargo fmt --all -- --check`: green.
- `cargo test --locked -p <crate> --no-fail-fast`, per crate (passed / failed / ignored):

| crate | passed | failed | ignored |
| --- | ---: | ---: | ---: |
| bink | 9 | 0 | 0 |
| bsa | 15 | 0 | 0 |
| d3d9-stream | 40 | 0 | 2 |
| d3d9-wgpu | 111 | 0 | 0 |
| d3dx-tex | 41 | 0 | 0 |
| engine | 918 | 0 | 1 |
| engine-bs | 4357 | 0 | 0 |
| engine-core | 1037 | 0 | 21 |
| engine-fallout-ai | 6207 | 0 | 0 |
| engine-fallout-cell | 5288 | 0 | 0 |
| engine-fallout-env | 1200 | 0 | 0 |
| engine-fallout-facegen | 735 | 0 | 0 |
| engine-fallout-forms | 5177 | 0 | 0 |
| engine-fallout-interface | 2965 | 0 | 0 |
| engine-fallout-magic | 1143 | 0 | 0 |
| engine-fallout-misc | 1431 | 0 | 0 |
| engine-fallout-pathfinding | 1868 | 0 | 0 |
| engine-fallout-script | 1841 | 0 | 0 |
| engine-fallout-unplaced-a | 1048 | 0 | 0 |
| engine-fallout-unplaced-b | 1402 | 0 | 0 |
| engine-gamebryo | 5398 | 0 | 0 |
| engine-havok | 12530 | 0 | 0 |
| engine-platform | 304 | 0 | 20 |
| engine-speedtree | 867 | 0 | 0 |
| engine-typed | 26 | 0 | 0 |
| esm | 68 | 0 | 1 |
| fos | 38 | 0 | 0 |
| frame-model | 97 | 0 | 0 |
| host | 165 | 0 | 6 |
| jip | 328 | 0 | 0 |
| mp3 | 35 | 0 | 0 |
| nv-viewer | 2 | 0 | 0 |
| nvinspect | 84 | 0 | 0 |
| nvse | 444 | 0 | 2 |
| play-patch | 0 | 0 | 0 |
| vorbis | 3 | 0 | 0 |

The first Linux run (before change 5 and 6) had eight failures: four `files` tests (Windows file
times), three `host` `cell_offsets` tests (Windows paths) and one `nvse` math test (libm). The
engine's 17 `typed_*` binaries take about 2 hours together with the build on a CI runner, so
`engine` passes but is not in `.fork/linux-ok.txt` (the Pipeline's Linux test job has 60 minutes);
give it a job of its own before listing it.

What remains: everything that needs a window, a Vulkan device, sound output or the machine tables
is not run on Linux (M3 and later in the plan). The ignored tests are the ones that need the player's
game files (`NV_EXE`, `NV_DATA`) or Windows.

## Hooks for the other files in `.fork/allowed-divergence`

| File | The change | Could an overlay hook leave upstream's file as it is? |
| --- | --- | --- |
| `crates/engine-core/src/engine.rs` | per-thread template of a test engine | no: it changes the body of `Engine::new_untyped` |
| `crates/engine-core/src/typed/diff.rs` | second control word only when the first read it | no: it is the harness's run loop |
| `crates/engine-core/src/x87.rs`, `units/crt.rs` | test builds note a read of the control word | no: a line inside each read function |
| `crates/engine-platform/Cargo.toml` | `d3d9-wgpu` on every target | no, a manifest line has no hook |
| `d3d9device.rs`, `d3d9resource.rs`, `d3dx.rs` | a `cfg` on a trait, its import and a binding | no: attributes on upstream's items |
| `crates/host/src/cell_offsets.rs` | calls `files::host_path` | no: one call in the function that builds the path |
| `crates/nvse/src/commands/math.rs` | `ignore` off Windows | yes, by a skip filter as for `files.rs` (`.fork/linux-skip.txt`); not done yet |
| `crates/d3d9-wgpu/src/loading.rs` | pipe list built on Windows only | no: attributes on upstream's items |

## Package, install and launch (the Linux release)

Fork-owned files: `packaging/linux/` (launcher, desktop entry, icon, install script, packaging and smoke-test scripts)
and `flake.nix` with `flake.lock`. Nothing upstream owns changed. The build still cannot play: it checks an
installation and runs what the port runs off Windows. No game data is in the package, in CI or in an artefact, and
nothing copies the player's `FalloutNV.exe`.

**The release asset.** `fork-release-build.yml` (job `linux`) builds `nv-viewer` for `x86_64-unknown-linux-gnu`, and
`packaging/linux/package.sh` writes `nv-rs-linux-x86_64-dev.tar.gz` with the folder `nv-rs-<version>-linux-x86_64/`
inside: `bin/nv-viewer`, `bin/nv-rs-launch`, `install.sh`, `share/applications/nv-rs.desktop`,
`share/icons/hicolor/256x256/apps/nv-rs.png`, `share/doc/nv-rs/README.txt` and `share/nv-rs/` (the capabilities and
release manifest files and their schemas). Beside it, as before: `capabilities.json`, `release-manifest.json`, the
schemas and `SHA256SUMS`, all attested.

**The contract with the installing side.** The Toolbox (`raul`) fetches, installs, checks and launches nv-rs
(the Toolbox's ADR-0007); until its tool supply lands, `nvmod install-nv-rs` does it the same way. It fetches `<release url>/SHA256SUMS` and the asset named
`nv-rs-linux-x86_64-dev.tar.gz` (`DEFAULT_ASSET`), checks the hash and the attestation, accepts only plain files and
folders that stay inside the folder (no link, no absolute path, no `..`), finds `nv-viewer` and `release-manifest.json`
by name anywhere in the tree and runs the manifest's `installation_check`. So the asset name is fixed (it was
versioned before; the version is now the inner folder's name and `release-manifest.json`'s), the tarball holds no
link, and the manifest's schema is unchanged. `scripts/fork/tests/test_packaging.py` checks the name, the member rules
and that exactly one `nv-viewer` and one manifest are in the tree.

**Install and launch.**

| Way | Command |
| --- | --- |
| Tarball | unpack, then `./install.sh` (to `~/.local`, or `--prefix DIR`): `bin/`, the desktop entry with the launcher's full path, the icon |
| Toolbox | `raul` installs and launches it; until the Toolbox's tool supply lands, `nvmod install-nv-rs` |
| Nix | `nix profile install github:JorisJonkers-dev/nv-rs` (or `nix run github:JorisJonkers-dev/nv-rs -- --check` after `nv-rs-launch --set-data`) |
| Launch | `nv-rs-launch --set-data "<game folder>"` once; then `nv-rs-launch --check` or `nv-rs-launch`; or directly `nv-viewer "<game folder>" [--check]` |

`nv-rs-launch` takes the data folder from its first argument, `$NV_RS_DATA` or the folder saved by `--set-data`
(`${XDG_CONFIG_HOME:-~/.config}/nv-rs/data-folder`) and passes the rest to `nv-viewer`. The desktop entry ("nv-rs",
category Game) runs it in a terminal, since the viewer reports on the terminal.

**Nix.** `flake.nix` has `packages.x86_64-linux.{default,nv-viewer}` (the viewer plus the launcher, desktop entry
and icon, built with the toolchain of `rust-toolchain.toml` through rust-overlay and `Cargo.lock`), `apps.default`
(the launcher), `checks.x86_64-linux.smoke` and a dev shell. Tests are not run in the Nix build (`doCheck = false`);
the Pipeline runs them. After a Cargo.lock or toolchain change nothing in the flake needs editing; `nix flake update`
moves nixpkgs and rust-overlay, in a pull request of its own.

**Smoke test.** `packaging/linux/smoke-test.sh <tree>` runs the built artefact: the libraries resolve, `--help`,
`--check` on a missing folder, on a Data folder with no `FalloutNV.exe` and on a wrong `FalloutNV.exe` (each is exit
status 1 with the problem named; the stand-ins are an empty file and a short line of text, not game data), and the
launcher's data-folder rules. The success path (exit 0) needs the player's own executable and is not run in CI. The
release build runs it on the unpacked tarball and on an `install.sh` prefix; the `nix` job runs it on the package
(`nix build .#checks.x86_64-linux.smoke`). Pull requests run the same script against a stand-in viewer
(`task scripts`), so the packaging cannot rot between releases.

Not done: publishing. The workflow never creates a release (the maintainer decides); the installing side's default
URL (`releases/latest/download`) needs the four files above attached to a GitHub Release by hand until that is decided.
macOS can follow the same layout (`packaging/macos/`, a `macos` job) when a native build exists.

## GPU tests on Linux (lavapipe)

The `rendering` component's Linux job installs Mesa's software Vulkan driver (`mesa-vulkan-drivers`, lavapipe), sets
`VK_ICD_FILENAMES` and `NV_WGPU_BACKEND=software`, and runs the tests with `RUST_TEST_NOCAPTURE=1`. The GPU tests in
`crates/d3d9-wgpu/tests/frame.rs` and `ops.rs` print "no GPU adapter ... skipped" and pass when no adapter opens, so
`scripts/fork/gpu_gate.py` reads the log, writes the executed and skipped counts to the job summary and fails the job when
any of them skipped. The driver version is printed in the summary. `window.rs` is Windows only. Replaying a synthetic
recording with `d3d9-replay` against a stored frame hash is not done yet.

## Window and input (stage 4): `crates/fork-window`

A fork-owned crate, additive: Windows keeps `engine-platform`'s window thread and its raw input. Off Windows the device
layer's `surface_for` (`d3d9-wgpu/src/device.rs`, the stub that said the window layer's port was to come) asks
`fork_window::surface_for`; that and the dependency in `d3d9-wgpu/Cargo.toml` (Linux only) are the whole change to a
file upstream owns (both in `.fork/allowed-divergence`).

- A window is an id (the device API passes a bare `isize`). `Window::open(request, on_change)` runs a winit 0.30.13 event
  loop on a thread of its own (`with_any_thread` for X11 and Wayland), creates the window sized from the request, and puts
  its raw display and window handles in a registry. `surface_for` makes the wgpu surface from them (Wayland, Xlib, XCB).
  Wayland's libraries are loaded at run time (`wayland-dlopen`); winit chooses Wayland in a Wayland session, else X11.
- Input is kept in the shape of `d3d9real::Input`: keys by DirectInput key code (`dik.rs`, a table with tests), mouse
  buttons in DirectInput's order, relative motion (`DeviceEvent::MouseMotion`, the pointer's own moves where the
  compositor sends none) and the wheel in 120s. `take_input(id)` and `held_keys(id)` read it once a frame; losing the
  focus releases everything. The close request is a flag (`close_requested`), not a process exit: the host decides.
- Shutdown: `Window::shutdown` (or drop) ends the loop and the thread; drop every surface first.
- Smoke: `cargo run -p fork-window --example clear -- --smoke` opens a window, clears 30 frames through wgpu on Vulkan and
  exits 0. `.github/workflows/fork-window-smoke.yml` runs it under Xvfb on lavapipe (not required, outside Pipeline Complete).
- Wired to the game (`fork_d3d9rec.rs`): `CreateWindowExA` opens a `fork_window` window when `NV_D3D9` is set, a display is
  there (`DISPLAY` or `WAYLAND_DISPLAY`) and `NV_D3D9_OFFSCREEN` is not; its id is what `real_window` answers, so the device
  and `Present` use `Output::Window` on the wgpu surface (a child window shares its parent's). A window without a caption
  (the game's `bFull Screen=1`: exclusive full screen is not offered) is borderless over the monitor
  (`WindowRequest::fullscreen`). Keys, buttons, relative motion and the wheel reach the game through
  `d3d9real::held_keys` and `take_input` (the same calls the Windows build makes in `host/player_move.rs`). A close request
  runs the exit hooks and ends the process, as `WM_CLOSE` does. The swap chain follows the window's size. Without a display
  the run stays off screen. The Xvfb smoke also runs `a_guest_program_draws_to_a_window` (`NV_WINDOW_SMOKE=1`).
- Not done: Wayland pointer lock and cursor hold (#213), title and `SetWindowPos` changes (#214) after the window is
  made, gamepads (#216), IME (#217), exclusive full screen (#218).

## Direct3D 9 off Windows (#196, stage 3a)

The recorder of `d3d9real.rs` (the game's Direct3D 9 calls recorded as `d3d9_stream::Cmd`) is built on every target. Upstream
split the file (slaterain/nv-rs#635): the Win32 window, its messages and raw input are in `d3d9real/win32.rs` (Windows only),
and the real `d3d9.dll` forwarding carries `#[cfg(windows)]`. Off Windows the fork's `fork_d3d9rec.rs` is the window layer in
place of upstream's `d3d9real/elsewhere.rs`, which refuses a window. The recorder's call sites in `d3d9device.rs`, `d3d9resource.rs`, `d3d9query.rs`, `d3d9draw.rs`
and `d3d9.rs` lost theirs, so the diff of every moved arm is the removed attribute line and nothing else. The Linux additions
are in the fork-owned `fork_d3d9rec.rs`: `make_stream` gives the stream the wgpu backend in `Output::Offscreen` (no window;
`NV_D3D9=real` or `real,wgpu` also draw on wgpu, with a line saying so), with the render thread, the occlusion answers and
`NV_D3D9_RECORD` as on Windows. Without a backend (`NV_D3D9` unset) nothing is recorded, as before.

Test: `d3d9real::fork::tests::a_guest_program_draws_a_frame_through_the_recorder_on_wgpu` drives `Engine::new()` through
`Direct3DCreate9`, `CreateDevice`, shaders, a clear, `DrawPrimitiveUP` and `Present` and reads the back buffer from lavapipe
(a cleared corner and the triangle's centre). The Nix CI shell sets `NV_REQUIRE_GPU=1`, so the test fails there when no adapter
opens; elsewhere it skips with a line. It runs in the `platform` job (engine-platform, inside the `ci` shell).

Not done (3b, on a developer machine): no run of the game with `NV_D3D9=wgpu NV_REPEATABLE=<frames>`, `GetRenderTargetData` and the read-backs are
tested only through the recorder's calls here, and the real-device fallbacks stay Windows only.

### Display match

The game's window and rendering follow the display the process started on (`fork-window/src/display.rs`). The monitor is
`NV_MONITOR=<index or output name>` (the window system's order; `DP-1`), else the output the window last landed on (kept in
`$XDG_STATE_HOME/nv-rs/last-monitor`), else the primary, else the first. Wayland has no query for the monitor holding the
focus or the cursor and a client cannot place its window: there the full-screen request names no output, the compositor
places the window (GNOME: on the current output), and the monitor it landed on is logged
(`fork-window: the window is on monitor <name> ...`) and kept for the next run. The size the settings are seeded with is
chosen before the window exists, so the first run may seed another output's size: `NV_RESOLUTION=WxH` or `NV_MONITOR` fixes it. The size and refresh (millihertz, rounded to hertz) are the monitor's own.

- Settings: a new `FalloutPrefs.ini`, and an existing one that lacks the keys, gets `iSize W`, `iSize H` and
  `bFull Screen=1` under `[Display]` (`seed_settings`, `prefs_with_display`). A key already in the file is the player's choice
  and is never changed. Delete the key (or the file) to get the display's size again.
- The game reads the display through the adapter table: the display is the current mode of `GetAdapterDisplayMode` and
  `EnumAdapterModes` (`with_current_mode`), so the engine sees the real size and refresh. A `mode =` line in
  `linux_profile.txt` still replaces it.
- A borderless window (the game's full screen) is made over that monitor, and the swap chain follows the window, so
  `SetWindowPos`, `ChangeDisplaySettings` and `Reset` at another size only change the back buffer, which is scaled to the
  window. A windowed request keeps its size (resizing after creation is not offered).
- Present mode: `NV_PRESENT=fifo|mailbox|immediate`; with none, the game's immediate interval is `Immediate`, anything else
  `Mailbox` where the surface has it, else `Fifo` (Windows keeps `Fifo`). The pacing of a minimised window uses the display's
  refresh. One line says the result: `NV_D3D9 display: monitor <index> <w>x<h> <hz> Hz, present <mode>`.
- No frame limit by default. Under the `fixes` preset the game's own 10 ms step and sleep are replaced (`NV_UNCAP`), the play
  frame limit is 0 (unlimited; `NV_FPS_LIMIT=N` or `FrameLimit=N` in settings.ini sets one) and `Mailbox` presents let the game
  render faster than the display refreshes without tearing. The only limiter left is the menu limit (`NV_MENU_FRAME_LIMIT`,
  120 a second; loading screens and play have none), which keeps the menus from spinning. The high-frame-rate landing and
  grab fixes stay on: they keep physics right at any rate. `NV_PRESENT=immediate` presents without waiting at all (tearing);
  `fifo` only when asked for.

### Alt-tab and a lost device

A window shown again (focus back, `Occluded(false)`, a redraw request) is made not minimised and, for the borderless
window, full screen again, a redraw is requested and the present loop is woken at once (`fork-window`, `restore`). While
no frame is shown the device is waited for each skipped frame (the queue does not grow), and a refused, timed-out,
occluded or outdated frame has the swap chain configured again before the next ask. A device the driver took away
(`radv/amdgpu: Not enough memory for command submission`, a reset) is logged with wgpu's reason and ends the run through the
exit hooks (`d3d9_wgpu::device_lost`) instead of leaving a frozen window and a stream of validation errors.

### One event loop, and the game's size

winit makes one event loop per process: a second is refused ("EventLoop can't be recreated"), which is how a separate
monitor query left the game with no window. `fork-window` now has one loop on one thread, started on first use and kept to the
end of the process; the monitor query (`display()`) and every `Window::open` go through it (a window ends with `shutdown`
or drop, the loop stays). `crates/fork-window/tests/one_loop.rs` (run by the Xvfb job with `NV_WINDOW_SMOKE=1`) queries the
monitors, opens a window, closes it and opens another; the `clear --smoke` step also fails unless it prints a `display:` line.

`NV_RESOLUTION=auto|WxH` sets the game's size in `FalloutPrefs.ini`: `auto` (the default) keeps any size the player has
set but replaces exactly the stock 640x480 with the display's size (a file made before the display was known); `WxH` sets
that size. A line, `settings: the game's size set from the display (<why>)`, says it happened. The game's back buffer and the
adapter's display mode both come from the display, so a run with no window opens at the monitor's size too.

