# Consuming the engine crates from another program

The Toolbox is the first consumer of nv-rs as a library. This page
says what a consumer gets, how it pins the code, and what it must repeat from this repository's build, because
Cargo does not carry those settings across a dependency edge.

## 1. What there is

Two fork-owned crates live under `crates/`. Both carry `publish = false`.

| Crate | What it is |
| --- | --- |
| `fork-api` | A typed, in-process facade. It re-exports the readers `fos`, `esm`, `game-profile` and `game-folders`, and offers owned summaries such as `SaveSummary`. It exposes `CAPABILITIES` and `API_VERSION: u32`, currently `1`. The number is bumped by hand on a breaking change of the facade. It has no external dependency and compiles in seconds. |
| `fork-play` | The play-mode entry point, `play_main()` (see PLAY-MODE.md). It is the engine itself and brings the whole engine closure, about 230 to 260 external crates. |

Use `fork-api` when you need data from saves, plugins and game folders. Use `fork-play` only when you need to run the
engine in-process.

## 2. No registry; the tag is the release

Nothing is published to crates.io. A tag `vX.Y.Z` of this repository is the release and the version of the library
crates. A release pull request (release-please) cuts the tag. `fork-api` and `fork-play` exist from the first tag
after v0.8.0.

The compatibility promise is the tag plus `fork_api::API_VERSION`. The lock file and the compiler are the real check.

## 3. How the Toolbox consumes it

The Toolbox uses a path dependency through its git submodule `components/nv-rs`, for example:

```toml
fork-api = { path = "../../components/nv-rs/crates/fork-api" }
```

The path is relative to the dependent crate. The pin is the submodule pointer. A path dependency is recorded in the
lock file without a commit of its own, so the Toolbox has a check that the submodule pointer equals the release tag.
That check keeps the pin and the tag the same.

The engine goes into the GUI workspace only, behind the cargo feature `engine` of `raul-gui`. The umbrella workspace
and its lock file stay engine-free and fast.

## 4. What does not travel

Cargo reads `[profile.*]` and `[patch]` only from the root workspace of the build. A consumer must repeat these
settings itself.

| Setting | Where it is in this repository | Why a consumer needs it |
| --- | --- | --- |
| `[patch.crates-io] wgpu-hal = { path = ".../vendor/wgpu-hal" }`, where the path points at the submodule's `vendor/wgpu-hal` | root `Cargo.toml`, `[patch.crates-io]` | The vendored wgpu-hal 30.0.1 carries the Vulkan pipeline-library work. Without the patch the consumer builds the published wgpu-hal, and pipeline behaviour changes. Copy this repository's entry, adjusting only the path to the consumer's submodule location. |
| `[profile.release.package.engine-core] codegen-units = 1` and `[profile.release.package.engine-platform] codegen-units = 1` | root `Cargo.toml`, the two `[profile.release.package.*]` tables | Without them the translated core is slower per frame. The `play` and `play-fast` profiles are a developer loop and are not copied. |
| `-C target-feature=+crt-static` on Windows | set through `CARGO_ENCODED_RUSTFLAGS` in `fork-release-build.yml` and `fork-package-windows.yml` | Without it the engine runs on a C runtime linkage that nobody has tested. |
| Rust toolchain 1.100.0 | `rust-toolchain.toml` | The crates refuse an older rustc. Keep both repositories on the same version. |
| 7-day crate cooldown | configured in both repositories | Keeps the two lock files choosing the same crate versions. Keep the setting identical. |
| Clippy allows | `.cargo/config.toml` | These are not applied to a consumer. A consumer's `-D warnings` reaches path dependencies unless it caps their lints. |
| Shared package versions in `Cargo.lock` | this repository's `Cargo.lock` | wgpu is pinned `=30.0.1` in `d3d9-wgpu`, `winit` `=0.30.13`, `raw-window-handle` `=0.6.2`. |

On the lock file: the consumer's lock should pick the same versions as this repository's lock for every shared
package. When the cooldown would choose an older version, copy the version with
`cargo update -p NAME --precise VERSION`.

## 5. Build time and memory

Linking `fork-play` pulls in the whole engine closure. A clean build takes a long time, and the link step needs a
great deal of memory. That is why the crate sits behind the `engine` feature. The feature is on in release builds and
off in fast development loops.

A consumer should record its own clean build time and peak memory for the engine feature.

The check job `facade` in `.github/workflows/fork-ci.yml` runs `cargo check --locked -p fork-api` and
`cargo check --locked -p fork-play` on every pull request. Each crate therefore builds alone.

## 6. Keeping the copies in step

When this repository changes any of the following, the consumer makes the matching change in the same bump:

- the `[patch.crates-io]` entry,
- the two `codegen-units` lines of the release profile,
- the Rust toolchain version or the cooldown,
- the `+crt-static` flag.
