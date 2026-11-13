# The fork's CI: one job per engine component

Upstream's gate (`.github/workflows/ci.yml`) is one Windows job that compiles and tests the whole workspace one
step after another: about 2.5 hours. Nearly all of it is one crate: the `engine` crate's `typed_*` differential
tests are 93 % of the test time (7,032 of 7,569 s in upstream's log). The fork's pipeline therefore verifies the
same things as **components**: a coherent area of the engine, with its own tests, built without compiling the
rest of the workspace.

The crate layout and every `Cargo.toml` are upstream's and stay untouched. The map of which crates form which
component is fork-owned: [`.fork/components.toml`](../../.fork/components.toml). [`scripts/fork/components.py`](../../scripts/fork/components.py)
checks it against `cargo metadata` and turns it into the job matrix of [`fork-build.yml`](../../.github/workflows/fork-build.yml).

## The component map

| Layer | Component | Crates | What it verifies | Runs on |
| ---: | --- | ---: | --- | --- |
| 0 | `core` | 3 | memory model, Engine, exe mapping, CRT, typed re-implementations, frame model | Windows (and a Linux twin) |
| 0 | `formats` | 7 | plugins, archives, saves, Bink, MP3, Vorbis, D3DX textures (also the software-truncation path) | both |
| 0 | `repo-checks` | 1 | what must never be committed (`nvinspect`); runs on every pull request that builds | both |
| 0 | `launcher-and-ledger` | 0 (own workspaces) | Play.exe with Settings.cmd, the typed-marker ledger | Windows |
| 1 | `engine-base` | 4 | Gamebryo, Havok, SpeedTree, the Bethesda units | both |
| 1 | `rendering` | 2 | the Direct3D 9 call stream and the wgpu backend | Windows (and a Linux twin) |
| 1 | `scripting` | 2 | xNVSE and JIP LN NVSE commands | both |
| 2 | `fallout-world` | 4 | forms, cells, land, sky, FaceGen | both |
| 2 | `platform` | 1 | window, Direct3D 9 / D3DX entry points, input, sound, loader | Windows |
| 3 | `fallout-gameplay` | 4 | scripts, quests, magic, pathfinding, AI | both |
| 4 | `fallout-front` | 4 | menus, dialogue, main loop, audio, unplaced runs | both |
| 5 | `integration` | 4 | the assembled engine: six jobs, see below | Windows |

`python3 -I scripts/fork/components.py graph` prints the same table with each component's closure (what
`cargo test -p` compiles) and an estimate of its compile cost; the `Plan components` job puts it in the run summary.

**`integration`** holds the engine crate, which links all 19 engine crates, and is split inside the component along
the typed families (the test files of `crates/engine/tests` already mirror `engine-typed`'s modules; the jobs group
families by subject, not by size): typed shader; draw, render, lights, particles; scene (culling, dispatch, geometry,
collision, pathing); animation; runtime (crt, process, lock-free, leaves); and one aggregator job (the engine's lib
tests, doctests and `par_render`, plus `host`, `play-patch` and `nv-viewer`). `components.py check` fails when a file
in `crates/engine/tests` is in no job, so a new upstream test file cannot be lost.

The three `fallout-*` components have the same large test closure (18 crates) because every fallout crate
dev-depends on every other (upstream's "Phase A"). That is a property of upstream's crates, not of the map; when upstream
finishes "Phase B" the closures shrink without any change here.

## What a job does

A job is named after its component: `Component: fallout-world (Windows)`, `Component: integration / typed shader family
(Windows)`, `Component: core (Linux, informational)`. It runs `cargo test --locked -p <crates>`, so cargo builds only the
closure. Because `-p` unifies only the selected packages' features, `components.py` adds `--features crate/feature` for
every feature some other workspace member switches on (today `engine-core/test-support`, which `engine-core`'s
`tests/typed_seam.rs` needs); this keeps the tests of a component equal to what `cargo test --workspace` runs.
[`run-component.sh`](../../scripts/fork/run-component.sh) builds first (`--no-run`, so compile time is its own number), groups the
log (`::group::` per compile and per test binary), annotates failed tests, and writes its timings to `$RUNNER_TEMP`
(not into the checkout: `nvinspect`'s hygiene test reads every file git does not ignore).

Where a component runs: Windows is authoritative (upstream's gate). A component marked `runs_on = "both"` also gets a
Linux twin that is **informational** (it cannot fail the run) until every crate of the component is in
[`.fork/linux-ok.txt`](../../.fork/linux-ok.txt); then the component runs on Linux only, required, and its Windows job goes away,
unless the component says `windows = "required"` (it holds `cfg(windows)` code). `Linux probe` lists which crates build on Linux; promote its
artifact to `linux-ok.txt` once the twins pass.

## Adding or moving a crate

A new workspace crate that `.fork/components.toml` does not list **fails the `Plan components` job** (and the unit
tests), unless a `[[rule]]` claims it (`engine-fallout-*` goes to `fallout-front`, `d3d9-*` to `rendering`,
`engine-*` to `engine-base`); a rule-claimed crate is tested, with a warning annotation asking for an entry. To add one:
put the crate name in the `crates` of the component it belongs to, and check that the component's `requires` cover the crate's
dependencies (the check says exactly which are missing). A crate in two components, an unknown component, a cycle in
`requires` and a dependency that no required component provides are errors. Locally: `task components`, or
`python3 -I scripts/fork/components.py check`.

## Which components run

- **Push to `fork`, the nightly schedule (03:17 UTC) and a manual run:** every component.
- **Pull request:** a documentation-only or fork-owned-only change skips the build (the `changes` job). Otherwise a
  component runs when a crate of its **test closure** changed (the crates, their dev-dependencies and all their normal
  dependencies), or one of its `paths` (the launcher and ledger also own `distribution/`); `pr = "always"` components
  (`repo-checks`) always run. Changes to `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/`, a vendored crate's
  users (`vendor/wgpu-hal` reaches the crates that use `wgpu`) or a file that steers the build (`.fork/components.toml`,
  `fork-build.yml` and the scripts it runs) run everything; a file outside every crate and every ignored area runs
  everything too, never nothing.
- Measured on the last 300 upstream commits (simulation of the rule): 253 of 283 code commits reach the five slow typed
  jobs, because `engine` depends on nearly everything. So "affected only" helps the other 11 % and the small components; the slow
  path is shortened by the split, not by selection.

## What is measured and what is estimated

Measured (read from logs and the API): upstream's single job, 147 minutes, of which 117 minutes are the engine
crate's tests (upstream run 43417602878, warm cache); the fork's old cold jobs (every job except `engine` 1 to 10
minutes); the engine closure compiles in 495 s cold; per test binary seconds in [`.fork/test-timings.json`](../../.fork/test-timings.json)
(upstream's runner, warm).

Estimated, labelled as such wherever it appears: the compile seconds of `graph` (a model: about 4.6 s per 100 k lines of
library, 18 s per 100 k lines under test, fitted to the cold runs), the critical path before a run has measured it, and
every number for Linux. The shader job (2,446 s of tests) and the scene job (2,396 s) bound the new layout at about 41 minutes
of tests plus the engine compile (about 8 minutes) plus setup (under 1 minute), times 1.0 to 1.3 for the runner. After a run, the
pipeline summary replaces the estimate with the measured number.

What remains unmeasured on purpose: whether `opt-level` 1 or 2 for `engine-core` and `engine-typed` makes the typed tests several times faster,
the linker, `target/` sizes and the Linux test results. `fork-ci-measure.yml` of the study answers them in one manual run; it is not
part of the pipeline.

## Reading a run

- **`Plan components`** (top of the run summary): why these components run, which are "not affected", and the graph.
- **Each component job's summary:** test binaries, passed, failed, ignored, measured compile minutes, measured test
  minutes and the sum of libtest's own times, the failed tests and the slowest binaries. Failed tests are also annotations.
  A failing binary's group is closed at its `failures:` section, so the panic message is visible.
- **`Clippy (whole workspace)`:** one line per diagnostic in a group, the first ten as annotations, a count per lint
  in the summary.
- **`Pipeline Complete`:** the only required check. It passes when every needed job passed (a skipped job fails, except
  the two pull-request-only jobs and the build when only documentation changed). Its summary lists each component's
  result, the longest job of the component and the duration, then every job with queue and run time, the job that finished
  last (the critical path of this run) and the failed steps. A Linux twin that failed is shown as "failed (informational)".
- **Test-count check (acceptance for a change to the map):** the component summaries together must equal what
  `cargo test --workspace` ran (upstream's log: 57,444 passed, 53 ignored). A feature a job misses would silently skip tests;
  the counts reveal it.

## Where the pieces are

| Piece | File |
| --- | --- |
| the map | `.fork/components.toml` |
| check, graph, matrix | `scripts/fork/components.py` |
| the build workflow | `.github/workflows/fork-build.yml` |
| the log, groups, annotations, timings | `scripts/fork/run-component.sh`, `libtest_summary.py`, `clippy_annotations.py` |
| the run summary | `scripts/fork/pipeline_summary.py` |
| tests of all of the above (`task scripts`) | `scripts/fork/tests/` (a stored graph of the workspace: `fixtures/workspace-graph.json`) |

Refresh the stored graph after upstream changes the workspace: run `cargo metadata --format-version 1 --no-deps --locked`, reduce it as the
fixture's `_note` says (workspace dependencies keep `kind` and `features`, external ones only name and kind) and replace the file.
The pipeline's own check uses the live metadata, so a stale fixture cannot hide a new crate there.

## What upstream could change (not done here: upstream's files)

Finish "Phase B" (drop the fallout crates' dev-dependencies on each other: smaller closures); one `#[cfg(windows)]` at
`engine-platform/src/lib.rs:214` (the engine then builds on Linux); split `crates/engine/tests/typed_shader.rs` along
`engine-typed/src/shader/*` (removes the 41-minute floor); move each `typed_*` test next to its family.
