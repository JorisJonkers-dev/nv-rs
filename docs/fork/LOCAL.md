# Local build-and-test loop

`scripts/fork/local/nv-rs-dev` builds, runs and smoke-tests the viewer on the maintainer's machine (Linux, Wayland,
Vulkan) without waiting for a CI Release Build. CI stays the merge gate: this loop is for iterating.

## Commands

| Command | What it does |
| --- | --- |
| `nv-rs-dev build [--opt] [--clean]` | `cargo build --profile play -p nv-viewer --bin nv-viewer` only; never the whole workspace. `--opt`: profile `play-opt` (every crate at opt-level 3, no incremental; about 4 minutes warm here) for measuring; `nv-rs-dev bench` runs that binary by default (`NV_DEV_PROFILE=play` for the quick one). Release artifacts (`--release` in the release and package workflows) were already optimised, with engine-core as one code unit. `--clean`: a cold build in a fresh `<target>-cold` dir with no compiler cache, timed (the shared dir is never emptied under another worker); every build appends its time to `~/.local/state/nv-rs/build-times.tsv` |
| `nv-rs-dev matrix [--ref R]... [--baseline R] [--suite S] [flags]` | Builds, tests and benchmarks many refs and writes one comparison report (the Rust tool's `nv-rs-scene matrix`): [Matrix runs](#matrix-runs) |
| `nv-rs-dev compare <baseline> <candidate>` | A two-ref matrix |
| `nv-rs-dev clean [--all]` | Removes this loop's own caches, sizes first: [Clean](#clean) |
| `nv-rs-dev run [args]` | Runs that binary on the Steam game folder as a local wrapper does (`NV_D3D9=wgpu`, Nix-store `LD_LIBRARY_PATH` fallback for wayland, xkbcommon and vulkan, `NV_SOUND=out`, `NV_PULSE_SIMPLE` fallback); log in `~/.local/state/nv-rs/logs/dev-<time>.log` |
| `nv-rs-dev smoke [--seconds N] [--shot]` | Runs N seconds (default 60) on the real display, stops it with SIGTERM, prints a PASS/FAIL checklist parsed from the log |
| `nv-rs-dev bench [clear-halt\|compare A B] ...` | The scene tool's bench, same as `scene bench`: [SCENE.md](SCENE.md), [BENCH.md](BENCH.md) |
| `nv-rs-dev logs [--run ID\|latest\|latest-bad] [--errors\|--hitches\|--tail N\|--grep PAT\|--around FRAME] [--diff A B] [--bundle]` | What happened in a run, in under 40 lines (80 for a bad one): [LOGS.md](LOGS.md). Also `nv-rs-logs` |
| `nv-rs-dev scene <build\|run\|compare\|soak\|sweep\|bench\|bench-compare\|guard\|matrix\|clean\|replay\|smoke-report>` | Pictures of fixed places, variants, soak and bench in Rust (`nv-rs-scene`): [SCENE.md](SCENE.md) |
| `nv-rs-dev replay <build\|run\|info\|pack\|synth\|streams>` | The replay-bench of a recorded Direct3D 9 stream (null, wgpu, raw Vulkan), under the bench lock: [REPLAY.md](REPLAY.md) |
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

## Clean

`nv-rs-dev clean` prints what it would remove with sizes, then asks (or takes `--yes`; `--dry-run` only prints). Without
`--all`: the shared cargo target dir, the cold-build dir, the matrix binary cache and the `matrix-*` worktrees it made.
With `--all`: also the sccache dir, matrix reports, bench runs and their scratch saves, dev logs and remote run dirs. It
removes only paths under `~/.cache/nv-rs` and `~/.local/state/nv-rs` (or `NV_DEV_TARGET` when that has `nv-rs` in its
path), never follows symlinks, never touches your own worktrees (only the `matrix-` prefix), and takes the matrix lock so
it will not run under a matrix in progress.

## Matrix runs

`nv-rs-dev matrix` (the Rust tool's `nv-rs-scene matrix`) answers "is this PR (or commit, branch, tag) better or worse than
`fork`" with one command and one report: for each ref it makes a detached worktree
`~/Projects/<owner>/worktrees/<repo>/matrix-<ref>@<sha>` (a PR number is fetched from `origin`; `working` is this checkout as it
is), builds `nv-viewer` (profile `play`, behind `capped 10G`, into the shared target dir; the binary is kept per commit in
`~/.cache/nv-rs/matrix-bin`), runs the affected components' Linux tests (`ci-lite`, against the baseline commit), benches a
built-in suite (`--suite quick|mojave|soak|capital`, default `quick`) under the exclusive game lock, and compares every
other ref with the baseline: average fps (5% and 1 fps), 1% low (10% and 1 fps) and p99 (10% and 0.3 ms) are `REGRESSION`
when worse than both their limit and their noise floor. The report is `~/.local/state/nv-rs/matrix/<time>/matrix.md` and
`matrix.json`.

```sh
nv-rs-dev matrix --ref fork --ref 93 --ref v0.3.0 --suite quick       # flags; the first ref is the baseline
nv-rs-dev compare fork 93                                            # two refs
nv-rs-dev matrix --ref fork --ref 93 --no-test                        # skip ci-lite
```

Exit status: 0 all fine, 1 a regression, a failed build, failed tests or a bench that did not pass its checks, 2 a request that
cannot be carried out. Not carried over from the Python version: other machines over ssh and rsync (`hosts.toml`), the
configuration axes of the old harness (window sessions and modes), the TOML spec file and the HTML page. Only numbers go in a report.

## Clean

`nv-rs-dev clean` lists what it would remove with sizes: the `matrix-*` worktrees it made and the matrix binary cache; with
`--all` also the shared cargo target dir and the matrix reports. It removes only with `--yes`, and never touches a worktree
without the `matrix-` prefix.

## Matrix runs

`nv-rs-dev matrix` answers "is this PR (or commit, branch, tag) better or worse than `fork`, on these settings, on these
machines" with one command and one report.

```sh
nv-rs-dev matrix --ref fork --ref 93 --ref v0.3.0 --scenario boot --config mode=uncapped      # flags
nv-rs-dev matrix matrix.toml                                                                  # or a spec file
nv-rs-dev compare fork 93                                                                     # two refs, defaults
nv-rs-dev matrix matrix.toml --json                                                           # CI: one JSON line on stdout
```

A spec file (TOML; flags add to it):

```toml
baseline = "fork"                 # default: the first ref; each other ref is judged against it
refs = ["fork", "93", "v0.3.0", "working"]   # commit, branch, tag, PR number (93 or pr:93); "working" = this checkout as it is
scenarios = ["boot"]              # bench scenarios (nv-rs-dev bench list)
hosts = ["local", "box2"]         # "local" or names from ~/.config/nv-rs/hosts.toml
test = true                       # the affected components' Linux tests (ci-lite) per ref, against the baseline commit
bench = true
scale = 0.5                       # bench --scale
thresholds = { fps = 3 }          # bench compare thresholds, percent

[[config]]                        # one bench setting set; default: offscreen capped 1920x1080, preset fixes
name = "lo"
session = "offscreen"             # offscreen | xvfb | nested-wayland
mode = "capped"                   # capped | uncapped (peak fps, immediate present)
resolution = "1280x720"
preset = "fixes"                  # fixes | vanilla
threads = "real"                  # real | stepped
sound = "silent"
vram_limit_pct = 70
env = { NV_SOMETHING = "1" }      # extra environment, e.g. an audio layout switch the build reads
args = ["--seconds", "30"]        # extra `bench run` arguments
```

What it does, per host: resolves each ref (a PR number is fetched from `origin`), makes a detached worktree
`~/Projects/<owner>/worktrees/<repo>/matrix-<ref>@<sha>` (never in the repository), builds `nv-viewer` through
`nv-rs-dev build` (memory-capped, the shared target dir, so dependency builds are shared; sccache is used when installed;
one cargo process at a time, enforced by a lock), keeps the binary per commit in `~/.cache/nv-rs/matrix-bin` so a repeat run
rebuilds nothing, runs `ci-lite` for the tests, then runs every scenario in every config for every ref through
`nv-rs-dev bench run` (the existing runner and its VRAM watchdog). Results go to `~/.local/state/nv-rs/matrix/<time>/`:
`matrix.md`, `matrix.html` (no script), `matrix.json` and the per-run bench directories. Every non-baseline ref is compared
with the baseline by the same `report.compare` and thresholds as `bench compare`; the report lists the deltas and
`REGRESSION` verdicts. Only numbers go in a report: no game data.

Safety: sessions are `offscreen` (no window at all), `xvfb` (a private X server through `xvfb-run`, the window cannot reach
your desktop) and `nested-wayland` (needs the bench runner's `--session` option; a cell is reported `skipped` until the
runner has it). A windowed run on the real display is not available through the matrix.

Exit status: 0 all fine, 1 a regression, a failed build, failed tests or a bench error, 2 a usage error. `--json` prints one
JSON object on stdout (progress goes to stderr) with `ok`, the build and test status of each ref and the comparisons with
the keys that regressed, for CI to read.

### Other machines

`~/.config/nv-rs/hosts.toml` (yours; nothing about hosts is in the repository, and no secrets belong in it: ssh keys come
from your agent):

```toml
[host.box2]
ssh = "me@box2"                  # anything `ssh` accepts, ~/.ssh/config aliases included
repo = "~/src/nv-rs"             # an nv-rs checkout on the host (with this tooling: it runs its own scripts/fork/local)
sync = "git"                     # git: `git fetch origin` there, so any ref pushed to origin works
                                 # rsync: this checkout's tree is copied over first (target/ excluded), so "working" and local commits work
game = "~/Games/Fallout New Vegas"
cache = "~/.cache/nv-rs"         # target dir, binaries and run dirs of the host
```

The whole matrix runs on the host with its own game folder, GPU and cache; only `results.json`, `summary.json`, `report.md`
and `report.html` come back (the logs stay there), and the host appears in the one report next to `local`. Hosts run one at a
time. `--dry-run` prints the resolved plan and the exact ssh and rsync commands without running anything.

## Rules for workers

- Use this loop instead of CI Release Builds while iterating; push when it works. `Pipeline Complete` is still the merge gate.
- Heavy work goes through `capped` (limit first): the script does this with `NV_DEV_MEM` (default 12G, above the measured peak). A runaway once used 14.4 GB and froze the desktop.
- One cargo process at a time on this machine, and no whole-workspace builds. Do not start a second `nv-rs-dev build` or `test` in parallel.
- Parallel workers send builds and test runs to the build daemon: `python3 scripts/fork/build_daemon.py run -- <cargo args>`. It starts on first use, owns the only cargo process and serves requests FIFO in one worker thread. It takes the shared lock (`NV_BUILD_LOCK`, default `~/.cache/nv-workers/cargo.lock`), runs under the memory cap through `capped` (`NV_BUILD_MEM`, default 10G), sets `CARGO_BUILD_JOBS` (`NV_BUILD_JOBS`, default 2) and uses one `NV_BUILD_TARGET_DIR` for every worker. `NV_BUILD_WRAPPER` is `sccache` when installed, so a warm rebuild of one family's test target reuses compiled work; `NV_BUILD_SCCACHE_DIR` places its cache. All limits are environment configuration, not paths in the tool. The cap needs `capped` on PATH (or `NV_BUILD_CAPPED`): without it the daemon warns loudly on every build and there is no cap. The `flock` lock is POSIX-only (not on Windows or stock macOS); the daemon's FIFO still serialises cargo there. Only the variables cargo, its build scripts and the typed test targets need are forwarded; other worker variables, including other `NV_*` ones, are not. `status` reports the queue figures, applied limits and warnings; `stop` ends the daemon after its current build. The one-family lab (`FAMILY-LAB.md`) calls this client command too.
- `smoke` opens a window on the maintainer's display: run it only when the maintainer's session is free for a minute, and never run two.
- Output holds numbers only. Never copy game data, logs of game content or screenshots into a repository. `--shot` saves to `~/.local/state/nv-rs/shots/` with grim or spectacle, local only.

## Smoke checklist

Parsed from the log by `nv-rs-scene smoke-report` (`crates/fork-scene/src/smoke.rs`): real window (no "could not be made" or off-screen line), display line with size and
Hz, frame counters advancing, no wgpu error lines, no `stopped:` line, no "not enough memory" or device-lost line, a sound
line, no panic. Exit status is 0 only if all pass; the viewer exit status 124 is `timeout` stopping it, as intended. The patterns follow the viewer's current log wording; adjust them in
that script when the wording changes.

First run on `fork` at #66 (2026-11-15, 45 s, AMD RDNA 3 (RADV) on Wayland): 8 of 8 checks pass, 1920x1080 at 144 Hz, frame counter 1 to about 8800, 0 wgpu errors.

## GPU memory safety

The amdgpu driver once ran out of memory during a New Game and took the compositor with it. Two guards now stand in front of that:

- A local wrapper samples VRAM and GTT every 2 s into `<log>.gpumem` and stops the game at `NV_VRAM_LIMIT_PCT` (default 90; use 50 while debugging).
- `d3d9-wgpu` counts the bytes of every texture and buffer it makes (`budget.rs`) and refuses what goes past `NV_GPU_BUDGET_MB` (default 80 % of the first DRM card's `mem_info_vram_total`, at most 8 GiB; 0 switches it off) or a texture side over 16384, with one `NV_D3D9 wgpu: stop: ...` line. Every 600th present prints the budget and wgpu's allocator report (`gpu memory made by the layer: ...; driver allocator ...`). A window client area under 16 pixels a side gets no swap chain (`stop: the window's client area is WxH`), and each (re)made set of back buffers is logged (`back buffers WxH`).
- A managed texture level the game gives whole, in a format the card holds as given (not a 16-bit one widened on upload), keeps no host copy: `ReadSurface` reads it back from the card, block-compressed levels as whole blocks. Other kept levels (system memory, 16-bit formats, a texture that could not be made, a level last given in part) keep their host bytes. `NV_D3D9_HOST_COPIES=1` keeps a host copy of every kept level, as before (for comparing).
- Texture uploads are submitted every 64 MB written (an empty submit carries wgpu's pending writes), so a load that writes hundreds of textures in one frame does not hold all their staging copies at once (on a 142-plugin far move: 2.7 GiB of GTT before). `NV_GPU_UPLOAD_SUBMIT_MB` sets the size, 0 turns it off.

To test without risking the desktop, run with `NV_D3D9_OFFSCREEN=1 NV_HUD=0 NV_VRAM_LIMIT_PCT=50`; there is no window, so nothing can reach the compositor.
