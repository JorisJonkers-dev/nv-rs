<!-- SPDX-License-Identifier: GPL-3.0-only -->
# The one-family typed test lab

A typed-function change used to rebuild the whole engine facade and, to check one
function, run a suite of thousands of generated cases. The lab builds one family's
differential cases in a crate of their own: the crate links `engine-core` (the
`Diff` harness), `engine-typed` (which registers the typed families) and only the
part crates whose translations the family's cases call. It never links `engine`,
the other typed suites or the program's other dependencies.

## Run one family

```sh
python3 scripts/fork/family_lab.py families
python3 scripts/fork/family_lab.py run fade
python3 scripts/fork/family_lab.py run fade -- --exact culling_fade_on_visible
task family-lab -- fade culling_fade_on_visible
```

On Windows use `python` if `python3` is not on the path. `scripts/fork/family-lab.sh`
is a convenience wrapper for POSIX shells that only delegates to the Python entry
point; it has no machine-specific paths in it. `task family-lab -- <family> ...`
passes a literal `--` to the tool, which removes it before reading the family name;
the `--` after the family separates libtest arguments. One family has one test
target named `typed_<family>`, so the run log holds only that family's cases. The
script generates `target/family-lab/<family>/` (git-ignored) from
`crates/engine/tests/typed_<family>.rs`.

## One build daemon owns cargo

`run` sends the cargo arguments to the client command of the build daemon
(`scripts/fork/build_daemon.py`), which needs only Python and cargo on Windows,
Linux and macOS. The first call starts one long-lived daemon; it keeps a FIFO queue
and one worker thread, so two workers asking at once are served in order and never
start two cargo processes. The daemon sets the limits when it starts, from
configuration rather than paths built into the tool:

```sh
export NV_BUILD_JOBS=2
export NV_BUILD_MEM=10G
export NV_BUILD_LOCK=~/.cache/nv-workers/cargo.lock
export NV_BUILD_TARGET_DIR=~/.cache/cargo-targets/nv-rs-gw
```

`NV_BUILD_CAPPED` and `NV_BUILD_FLOCK` name the wrapper executables (found on PATH
by default, absent on Windows for `flock`). `NV_BUILD_WRAPPER` is the rustc wrapper,
`sccache` by default when installed, or an explicit path; `NV_BUILD_SCCACHE_DIR`
sets its cache. Every worker uses the same target directory, so a repeat of a
family's test target is incremental and the warm compiler work comes from sccache.
`python3 scripts/fork/build_daemon.py status` shows the queue figures and limits;
`stop` ends the daemon after its current build.

Set `NV_FAMILY_LAB_DIRECT=1` to use plain cargo instead. The optional wrapper and
lock below then apply; with neither set there is no lock and no memory cap, which
is the normal portable case:

```sh
export NV_FAMILY_LAB_CARGO='flock ~/.cache/nv-workers/cargo.lock ~/.local/bin/capped 10G cargo'
```

`NV_FAMILY_LAB_CARGO` is a command prefix (parsed as shell words, with `~` and
environment variables expanded). `NV_FAMILY_LAB_LOCK` is an alternative to a
`flock` prefix: a lock file held for the whole cargo run, whose parent directory
is created if needed. Use one of the two locks, not both.

## How the part crates are chosen

The generator (`scripts/fork/family_lab.py`) starts from every `entry!` address in
the suite, then reads the marked translation and typed function at each address and
takes the addresses they call with `e.call(...)` and `typed_call!(...)`, following
helpers beside them in the same file. Data addresses and float bit patterns are not
calls, so they no longer pull in unrelated crates. The generated `assemble!` starts
with `engine_typed::units::funcs`: without it `Engine::new()` applies no typed
family and a by-address typed call silently runs the translation. A suite that uses
`include!` has its included tables copied beside the generated test, and the lights
suite's guard hook is copied from `crates/engine/src/dead_lights.rs` with its two
facade imports rewritten to the part crates.

A callee reached only through a vtable, or one reached deeper than the functions
under test, has no address in that set. If a case stops at such an open function,
name the crate in `NV_FAMILY_LAB_PARTS` (comma separated) and rerun. The lab is for
iteration; the full CI suite remains the proof that a case passes.

## Timings

| What | Before | After |
| --- | ---: | ---: |
| Typed shader CI job (docs/fork/CI-SPEED.md, runs to 2026-11-15) | 1004–1329 s | unchanged |
| `engine::typed_shader` binary (`.fork/test-timings.json`, upstream run 43417602878) | 2445.7 s | unchanged |
| One typed-function iteration: warm lab command after a change to a typed function | about 45 min | **18.6 s** |

The last row was measured on 2026-11-26 in this checkout with the `fade` case
`culling_fade_on_visible`. The exact command was
`NV_FAMILY_LAB_CARGO='flock ~/.cache/nv-workers/cargo.lock ~/.local/bin/capped 10G env CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=$HOME/.cache/cargo-targets/nv-rs-gw cargo' python3 -I scripts/fork/family_lab.py run fade -- --exact culling_fade_on_visible`.
The generated crate was already built and `engine-core`, `engine-typed` and fade's
five part crates were warm in `~/.cache/cargo-targets/nv-rs-gw`; the lock was free.
Cargo reported `Finished in 0.04 s`, the case ran in 18.05 s and the command took
18.6 s of wall time. The first build of a lab crate with its part crates took
1m 51s, and a change to a shared dependency rebuilds more than this one-family
figure; waiting for another worker's cargo process adds its wait time. These are
local measurements, not CI runs. No full-suite run was made for this change, so its
CI timings are recorded as unchanged, with the sources above.

## What stays the same

- The suites in `crates/engine/tests/typed_*.rs` are not moved or edited. Every case
  still lives in one suite, so the full suite still runs each case once.
- The generator copies each suite verbatim, apart from replacing `use engine as _;`
  with the reduced `assemble!` call and, for `lights`, adding the small guard hook
  the facade owns. `scripts/fork/tests/test_family_lab.py` checks that every case,
  every `Diff::new(` and every `.assert_same()` survives the copy, that included
  tables arrive, and that each `// Typed from` address has a registered Diff case.
- The hygiene rule is unchanged: `cargo test -p nvinspect --test repo_hygiene
  typed_functions_are_registered_and_have_a_differential_test` still reads the
  original suites and requires a registered `Diff` case for every `// Typed from`.
  That test is the authority; the Python check covers the address-level part the
  lab relies on.
- CI keeps running the component jobs from `.fork/components.toml`; the lab does not
  add cases or change which job runs them.

## Adding a family

Every `typed_<family>.rs` file is a family. `family-lab.sh --list` prints its derived
assembly. A new suite needs no registration beyond its file; a new callee reached
only through a vtable needs the `NV_FAMILY_LAB_PARTS` entry described above.
