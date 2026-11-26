<!-- SPDX-License-Identifier: GPL-3.0-only -->
# The one-family typed test lab

A typed-function change used to rebuild the whole engine facade and, to check one
function, run a suite of thousands of generated cases. The lab builds one family's
differential cases in a crate of their own: the crate links `engine-core` (the
`Diff` harness), `engine-typed` (the typed families) and only the part crates that
hold the translations that family reaches. It never links `engine`, the other typed
suites or the program's other dependencies.

## Run one family

```sh
scripts/fork/family-lab.sh --list
scripts/fork/family-lab.sh fade
scripts/fork/family-lab.sh fade -- --exact culling_fade_on_visible
task family-lab -- fade culling_fade_on_visible
```

The script generates `target/family-lab/<family>/` (git-ignored) from
`crates/engine/tests/typed_<family>.rs`, then runs one cargo process under the shared
lock and memory cap. Its arguments after `--` go to libtest, so one case can be
selected with `--exact` or a name filter. Each family has one test target named
`typed_<family>`; the run log holds only that family's cases.

The generator (`scripts/fork/family_lab.py`) finds the part crates from the
provenance markers: the addresses in the suite, in the translation files of the
functions under test and in the typed files that hold their `// Typed from` markers.
A callee reached through a vtable has no address in any of those texts. If a case
stops at an open function, name the crate in `NV_FAMILY_LAB_PARTS` (comma separated)
and rerun. The lab is for iteration; the full CI suite remains the proof that a
case passes.

## Timings

| What | Before | After |
| --- | ---: | ---: |
| Typed shader CI job (docs/fork/CI-SPEED.md, runs to 2026-11-15) | 1004–1329 s | unchanged |
| `engine::typed_shader` binary (`.fork/test-timings.json`, upstream run 43417602878) | 2445.7 s | unchanged |
| One typed-function iteration: warm lab command after a change to a typed function | about 45 min | **24.8 s** |

The last row was measured on 2026-11-26 with the shared cargo wrapper on this checkout:
the `fade` case `culling_fade_on_visible`, after touching
`crates/engine-typed/src/culling/fade.rs`. Cargo rebuilt `engine-typed` and the lab
target in 3.30 s, ran two matching cases in 21.00 s, and the command took 24.755 s of
wall time. An exact single case took 2.22 s to build and 20.76 s to run. The lock was
free for that run; waiting for another worker's cargo process adds the wait time.
The first build of a lab crate, with its part crates, took 1m 51s. These are local
measurements, not CI runs. No full-suite run was made for this change, so its CI
timings are recorded as unchanged, with the sources above.

## What stays the same

- The suites in `crates/engine/tests/typed_*.rs` are not moved or edited. Every case
  still lives in one suite, so the full suite still runs each case once.
- The generator copies each suite verbatim, apart from replacing `use engine as _;`
  with the reduced `assemble!` call and, for `lights`, adding the small guard hook
  the facade owns. `scripts/fork/tests/test_family_lab.py` checks that every case,
  every `Diff::new(` and every `.assert_same()` survives the copy.
- The hygiene rule is unchanged: `cargo test -p nvinspect --test repo_hygiene
  typed_functions_are_registered_and_have_a_differential_test` still reads the
  original suites and requires a registered `Diff` case for every `// Typed from`.
- CI keeps running the component jobs from `.fork/components.toml`; the lab does not
  add cases or change which job runs them.

## Adding a family

Every `typed_<family>.rs` file is a family. `family-lab.sh --list` prints its derived
assembly. A new suite needs no registration beyond its file; a new callee reached
only through a vtable needs the `NV_FAMILY_LAB_PARTS` entry described above.
