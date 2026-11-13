# Native Linux build: what the fork changes

Goal of this stage (M0 and M1 of the native Linux plan): the whole workspace compiles on
`x86_64-unknown-linux-gnu` and the portable logic is tested there. The window, input, sound output
and the real Direct3D 9 / wgpu device stay Windows-only; nothing here makes the game run on Linux.

How it is verified: the fork's CI, through `.github/workflows/linux-probe.yml`, which runs only on
pushes to `probe/**` branches: `cargo check --locked --workspace --all-targets`, then
`cargo test --locked -p <crate>` for every crate (the engine crate in a job of its own), with a
table of results in the job summary. The Pipeline's Linux jobs run the crates in `.fork/linux-ok.txt`.

Every change below is `cfg`-gated or additive, so Windows code is unchanged. Upstream-owned files
changed are listed in `.fork/allowed-divergence`. Upstream's issue #268 and CONTRIBUTING reserve
the platform layer (`d3d9real.rs`, the call sites in `d3d9device.rs`, `d3d9resource.rs`,
`d3d9query.rs`, `d3d9.rs`, `window.rs`): none of those call sites and none of `d3d9real.rs` is
touched.

## Changes

| # | Change | Files | Why | Reserved area | Offer upstream as is? | Risk to Windows |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | `d3d9-wgpu` is a dependency of `engine-platform` on every target (it was `cfg(windows)` only) | `crates/engine-platform/Cargo.toml` | `end_process` calls `d3d9_wgpu::loading::finish_output()` unconditionally: this one line stopped `engine-platform`, so `engine`, `host`, `play-patch` and `nv-viewer` | no | yes, one-line move | none: the dependency was already there on Windows; `Cargo.lock` unchanged |
| 2 | The start screen's output capture (`Pipe`, `CAPTURED`) is built on Windows only | `crates/d3d9-wgpu/src/loading.rs` | dead code warnings off Windows (the capture is `cfg(windows)`) | no | yes | none |
| 3 | The argument-word trait `Word` and its import are built on Windows only | `d3d9device.rs`, `d3d9resource.rs` | used only inside `#[cfg(windows)]` code of the `forwarded!` macro; an unused warning off Windows | touches the files, not a call site | yes, but wait for the seam: the files are being retuned | none |
| 4 | An unused binding and a warning-only attribute off Windows; a test constant used only by Windows tests | `d3dx.rs`, `files.rs` | warnings (the Linux clippy job is `-D warnings`) | no | yes | none |
| 5 | `files::real_path` is public and, on Unix, maps a game path: `Z:` is the host root (as Wine's), `\` separates, names match without regard to case in their directory; other drive letters have no mapping and are not found | `files.rs`, `host/src/cell_offsets.rs` (uses it) | the game's paths are Windows paths; three `host` tests failed on Linux for this reason. Written against the current code, not taken from the earlier Linux file-layer branch | no (file layer, approved in principle as a small PR on #268) | yes, after an issue | none: the Windows arm is the old body |
| 6 | Tests that can only pass on Windows are `#[cfg_attr(not(windows), ignore = "reason")]`: four `files` tests (they need the file times that are only provided on Windows) and `nvse`'s bit-for-bit math comparison (tan and tanh use the host's libm; the game's differ in the last bit: `dTan90`, `dTanh30`) | `files.rs`, `nvse/src/commands/math.rs` | honest failures off Windows | no | yes | none |
| 7 | The probe workflow and its summary script | `.github/workflows/linux-probe.yml`, `scripts/fork/linux_test_summary.py` | CI loop without starting the Windows jobs | fork only | no | none |

Not changed, on purpose: file times off Windows still stop the run with a message (the game
reads times measured on NTFS; inventing them is the stand-in the project forbids), and
`D3DXCompile*`, the sound output and the machine tables are as the native Linux plan describes them.

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
engine's 17 `typed_*` binaries take about 1.5 hours together on a CI runner (`typed_shader` alone
is the longest).

What remains: everything that needs a window, a Vulkan device, sound output or the machine tables
is not run on Linux (M3 and later in the plan). The ignored tests are the ones that need the player's
game files (`NV_EXE`, `NV_DATA`) or Windows.
