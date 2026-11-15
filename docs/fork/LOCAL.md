# Local build-and-test loop

`scripts/fork/local/nv-rs-dev` builds, runs and smoke-tests the viewer on the owner's machine (Linux, Wayland,
Vulkan) without waiting for a CI Release Build. CI stays the merge gate: this loop is for iterating.

## Commands

| Command | What it does |
| --- | --- |
| `nv-rs-dev build` | `cargo build --profile play -p nv-viewer --bin nv-viewer` only; never the whole workspace |
| `nv-rs-dev run [args]` | Runs that binary on the Steam game folder as `nv-rs-run` does (`NV_D3D9=wgpu`, Nix-store `LD_LIBRARY_PATH` fallback for wayland, xkbcommon and vulkan, `NV_SOUND=out`, `NV_PULSE_SIMPLE` fallback); log in `~/.local/state/nv-rs/logs/dev-<time>.log` |
| `nv-rs-dev smoke [--seconds N] [--shot]` | Runs N seconds (default 60) on the real display, stops it with SIGTERM, prints a PASS/FAIL checklist parsed from the log |
| `nv-rs-dev bench <run\|list\|report\|compare>` | Scripted playthrough runs with frame-time, memory and stability reports: [BENCH.md](BENCH.md) |
| `nv-rs-dev test <crate> [args]` | `cargo test -p <crate>`, same cap and target dir |
| `nv-rs-dev check [crate...]` | `cargo fmt --check` and `cargo clippy --all-targets -D warnings` for the named crates, default the crates changed against `origin/fork` |
| `nv-rs-dev affected [base]` | the crates a diff changes and the components CI would test for it (`components.py affected`; "EVERYTHING" when the diff touches `Cargo.lock`, the toolchain, workflows, the map or a shared file) |
| `nv-rs-dev ci-lite [base]` | `fmt --check`, then the Linux test commands of the affected components, as CI runs them; Windows-only jobs (the typed integration families) are left to CI |

`~/.local/bin/nv-rs-dev` is a thin wrapper, not committed: it runs `scripts/fork/local/nv-rs-dev` from the checkout
you are in (or `NV_DEV_REPO`, or the main clone). Recreate it with:

```sh
printf '%s\n' '#!/usr/bin/env bash' 'top="${NV_DEV_REPO:-$(git rev-parse --show-toplevel 2>/dev/null)}"' \
 'for r in "$top" "$HOME/Projects/JorisJonkers-dev/nv-rs"; do [ -x "$r/scripts/fork/local/nv-rs-dev" ] && exec "$r/scripts/fork/local/nv-rs-dev" "$@"; done' \
 'echo "cd into an nv-rs worktree" >&2; exit 2' > ~/.local/bin/nv-rs-dev && chmod +x ~/.local/bin/nv-rs-dev
```

## How it is fast

- Profile `play` (already in `Cargo.toml`): engine crates and host at opt-level 1, no debug info, incremental, many
  code-generation units. No new profile was needed.
- One persistent target dir, `~/.cache/nv-rs/target` (`NV_DEV_TARGET`), shared by every worktree, so rebuilds are
  incremental. The binary is `~/.cache/nv-rs/target/play/nv-viewer`. Cargo locks the dir: two builds queue.
- The toolchain's default linker works here; no `CARGO_ENCODED_RUSTFLAGS` was needed (set `NV_DEV_RUSTFLAGS` to try one).

Measured on this machine (16 threads, 31 GB): see the table below.

| Build | Time | Peak memory (cargo, rustc and linker together) |
| --- | --- | --- |
| Cold (empty target dir) | 233 s (3 m 53 s) | about 5.8 GB (sampled every 2 s), so 12G leaves room |
| Warm, nothing changed | 0 s (no-op); an incremental rebuild after a one-crate change depends on the crate, not measured | |

## Rules for workers

- Use this loop instead of CI Release Builds while iterating; push when it works. `Pipeline Complete` is still the merge gate.
- Heavy work goes through `capped` (limit first): the script does this with `NV_DEV_MEM` (default 12G, above the measured peak). A runaway once used 14.4 GB and froze the desktop.
- One cargo process at a time on this machine, and no whole-workspace builds. Do not start a second `nv-rs-dev build` or `test` in parallel.
- `smoke` opens a window on the owner's display: run it only when the owner's session is free for a minute, and never run two.
- Output holds numbers only. Never copy game data, logs of game content or screenshots into a repository. `--shot` saves to `~/.local/state/nv-rs/shots/` with grim or spectacle, local only.

## Smoke checklist

Parsed from the log by `smoke_report.py`: real window (no "could not be made" or off-screen line), display line with size and
Hz, frame counters advancing, no wgpu error lines, no `stopped:` line, no "not enough memory" or device-lost line, a sound
line, no panic. Exit status is 0 only if all pass; the viewer exit status 124 is `timeout` stopping it, as intended. The patterns follow the viewer's current log wording; adjust them in
that script when the wording changes.

First run on `fork` at #66 (2026-11-15, 45 s, RX 7900 XTX on Wayland): 8 of 8 checks pass, 1920x1080 at 144 Hz, frame counter 1 to about 8800, 0 wgpu errors.

## GPU memory safety

The amdgpu driver once ran out of memory during a New Game and took the compositor with it. Two guards now stand in front of that:

- `nv-rs-run` samples VRAM and GTT every 2 s into `<log>.gpumem` and stops the game at `NV_VRAM_LIMIT_PCT` (default 90; use 50 while debugging).
- `d3d9-wgpu` counts the bytes of every texture and buffer it makes (`budget.rs`) and refuses what goes past `NV_GPU_BUDGET_MB` (default 80 % of the first DRM card's `mem_info_vram_total`; 0 switches it off) or a texture side over 16384, with one `NV_D3D9 wgpu: stop: ...` line. Every 600th present prints the budget and wgpu's allocator report (`gpu memory made by the layer: ...; driver allocator ...`). A window client area under 16 pixels a side gets no swap chain (`stop: the window's client area is WxH`), and each (re)made set of back buffers is logged (`back buffers WxH`).

To test without risking the desktop, run with `NV_D3D9_OFFSCREEN=1 NV_HUD=0 NV_VRAM_LIMIT_PCT=50`; there is no window, so nothing can reach the compositor.
