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

Not changed, on purpose: `D3DXCompile*` and the sound output are as the native Linux plan describes them.

## Platform answers off Windows (stage 2 of `docs/research/nv-rs-linux-playable.md` in nv-linux)

The game's first calls on Linux no longer stop the run. The code is fork-owned and additive:
`crates/engine-platform/src/fork_platform.rs` and `crates/engine-core/src/engine_unix.rs` (both in
`.fork/overlay-paths`), reached by one `cfg(unix)` hook each. The Windows arms are the code that was there; the
Windows jobs run the same tests as before (the four `files` tests that were `ignore`d off Windows run on Linux too).

| Question | Windows | Linux |
| --- | --- | --- |
| File times of `FindFirstFileA` / `FindNextFileA` (`files.rs`, `write_find_data`) | the host's own search | the file's own metadata as `FILETIME`: last write, last access, and the birth time from `statx` (`STATX_BTIME`, through the standard library); on a file system that keeps no birth time, the older of the last write and the last status change. Attributes: directory, else archive (as `GetFileAttributesA` answers). A time the file system cannot give stops the run naming the file |
| `GlobalMemoryStatus(Ex)` (`process.rs`) | `GlobalMemoryStatusEx` | `/proc/meminfo`: `MemTotal`, `MemAvailable` (`MemFree` without it), swap added for the page file; no `/proc` means no answer, as before |
| Real clock's start (`Engine::start_real_clock`) | `GetTickCount` | the machine's uptime in ms from `/proc/uptime`, wrapped to 32 bits as `GetTickCount` wraps (0 if `/proc/uptime` is missing) |
| Table paths (`dsound.tsv`, `d3d9_adapter.tsv`, `d3dx-cache`) | `%LOCALAPPDATA%\nv-rs` | `$XDG_DATA_HOME/nv-rs` (an absolute path), else `~/.local/share/nv-rs`; the `NV_*` overrides are unchanged |
| `host::save_out::known_folders()` | `SHGetFolderPathW` | `<data folder>/nv-rs/userdata/{Documents,AppData/Roaming,AppData/Local}` (not created) |

Code pages beyond ASCII and 1252 are not provided: a name's other characters become `?` (see `code_page`); a name
that needs one still stops no run. Tests: `fork_platform::tests` and `unix_tests` (synthetic files with chosen times
through `FindFirstFileA`, the `/proc/meminfo` parser on fixtures, the paths under a fake `XDG_DATA_HOME`) and
`engine_unix::tests` (the uptime parser and the clock start).

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

**The contract with nvmod.** `nvmod install-nv-rs` fetches `<release url>/SHA256SUMS` and the asset named
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
| nvmod | `nvmod install-nv-rs` |
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

Not done: publishing. The workflow never creates a release (the maintainer decides); nvmod's default URL
(`releases/latest/download`) needs the four files above attached to a GitHub Release by hand until that is decided.
macOS can follow the same layout (`packaging/macos/`, a `macos` job) when a native build exists.

## GPU tests on Linux (lavapipe)

The `rendering` component's Linux job installs Mesa's software Vulkan driver (`mesa-vulkan-drivers`, lavapipe), sets
`VK_ICD_FILENAMES` and `NV_WGPU_BACKEND=software`, and runs the tests with `RUST_TEST_NOCAPTURE=1`. The GPU tests in
`crates/d3d9-wgpu/tests/frame.rs` and `ops.rs` print "no GPU adapter ... skipped" and pass when no adapter opens, so
`scripts/fork/gpu_gate.py` reads the log, writes the executed and skipped counts to the job summary and fails the job when
any of them skipped. The driver version is printed in the summary. `window.rs` is Windows only. Replaying a synthetic
recording with `d3d9-replay` against a stored frame hash is not done yet (tracked in nv-linux).

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
- Not done (nv-linux issues): the game's call sites do not use the registry yet (`engine-platform` window and
  `player_move.rs` input merge), Wayland pointer lock and cursor hold, the borderless refit and refresh rate, gamepads, IME.
