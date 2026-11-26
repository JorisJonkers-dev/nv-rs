# Tests that need the player's game files

Some tests need `FalloutNV.exe` (`NV_EXE`) or the Data folder (`NV_DATA`). The public pipeline has neither, so they must
not pass silently there and a private job must be able to tell a pass from a skip. No game data, and no output derived from
it, is committed: a private run reads the files from outside the repository.

## The gate

[`scripts/fork/test_gate.rs`](../../scripts/fork/test_gate.rs) is the one gate. A test crate pulls it in with `include!` from a
`#[cfg(test)]` module (`engine-core`, `engine-havok` and `host` do, in `lib.rs`), so no manifest changes. A test starts with

```rust
let Some(exe) = crate::test_gate::require_data("NV_EXE") else { return };
```

| `NV_EXE` | `NV_ORACLE_REQUIRE` | Result |
| --- | --- | --- |
| unset or empty | unset | prints `skipped: <test> (NV_EXE not set)` and returns |
| unset or empty | `1` | panics: the test fails |
| set | any | prints `executed: <test> (NV_EXE set)` and returns the path |

A test that is `#[ignore = "needs NV_DATA"]` may instead read the variable with `.expect(..)`, which also fails when it is
unset. Any other read (`let Ok(p) = std::env::var("NV_EXE") else { return }`, `.ok()`, `unwrap_or`) is a silent skip.

## The lint and the counts

- `python3 -I scripts/fork/check_data_gates.py` fails on a silent read of `NV_EXE`, `NV_DATA` or `NV_ORACLE_EXE` in any Rust
  file (the `research/nv-oracle` command line tools report their own skips and are exempt). It runs in `task scripts`, so in
  the Repository checks job; `scripts/fork/tests/test_data_gates.py` proves it fails on a test that returns silently.
- `scripts/fork/gate_counts.py` reads libtest output and prints how many gated tests executed and how many skipped. The
  private job runs the tests with `-- --nocapture` (or `--show-output`) and pipes them in:

  ```sh
  NV_EXE=... NV_ORACLE_REQUIRE=1 cargo test ... -- --nocapture 2>&1 | python3 -I scripts/fork/gate_counts.py --require
  ```

  `--require` exits 1 when a gated test skipped or none executed.

## The private run

[`scripts/fork/private_run.py`](../../scripts/fork/private_run.py) is the entry point for the private run. It runs
the same test commands as public CI, sets `NV_ORACLE_REQUIRE=1` and calls `gate_counts.py --require`, so a single
`skipped:` line, including one printed outside the gate, fails the run. Public CI runs without the flag: the gate
prints `skipped:` and the job stays green.
The workflow [`.github/workflows/fork-private.yml`](../../.github/workflows/fork-private.yml) has only
`workflow_dispatch` and `schedule` triggers, so no pull request reads private material, and it runs on the
maintainer's machine. The paths come from secrets and are never printed.

Before running any test, the private run calls [`scripts/fork/private_integrity.py`](../../scripts/fork/private_integrity.py).
It computes one SHA-256 over the judge (`typed/diff.rs`), its thresholds (the typed module and the x87 control word),
the planted-bug and byte-matching controls (`diff_reference_peer.rs`) and the ledger generator (`scripts/ledger/`), and
compares it with [`.fork/private-integrity.sha256`](../../.fork/private-integrity.sha256). A change to any of those
files without a matching hash update fails the private run. To update it, print the new hash with
`python3 -I scripts/fork/private_integrity.py --print` and store it; `--check` verifies it without writing. Line
endings do not affect the hash.

### What the private run executes

The commands of `scripts/fork/private_run.py` cover every test that uses the data gate: the `require_data` tests in
`engine-core`, `engine-havok`, `host` and `diff_reference_peer.rs`, including the five `#[ignore]` recorded-table
tests of `engine-core` (selected by their shared name with `--ignored`). They also run the ignored private-input
integration tests that need only `NV_EXE` or `NV_DATA`: `engine/tests/{havok_bodies,real_scda,typed_archives,
typed_script_lines,typed_scripts}.rs`, `esm/tests/cell_offsets_real.rs`, `host/tests/{live_player_move,
static_initializers}.rs` and the `nvse` decompile test. Those tests read their variable with `.expect(..)`, so a
private run fails on a missing input instead of skipping it. `scripts/fork/tests/test_private_run.py` checks the
tree against this list, so a new private-input test file must be covered here or listed below.

The private run does **not** execute these private-input tests, because each needs another input that the private
workflow does not carry. They are `#[ignore]`d and read that variable with `.expect(..)`, so they fail loudly when
asked for; they never print a gate `skipped:` line that could be mistaken for a pass:

- `crates/host/tests/live_movement_capture.rs` — needs `NV_CAPTURE`, a recorded movement capture beside the game files.
- `crates/nvse/src/compiler_tests.rs` — needs `NV_COMPILE_PLUGINS`, a folder of plugins compiled with xNVSE.
- `crates/nvse/src/docs_tests.rs` — needs `NV_XNVSE_DLL`, the shipped `nvse_1_4.dll`.
- `crates/nvse/src/newlang/compile_tests.rs` — needs `NV_XNVSE_NEW_COMPILER_TESTS`, a folder of xNVSE's own compiler tests.
- `crates/engine/tests/jip_call_arity.rs` — needs `NV_PROBE`, a sweep's address list rather than game data.
- `crates/engine-platform/src/vorbisfile.rs` — needs `NV_VORBIS_TEST_DIR`, a folder of the game's `.ogg` files.

`scripts/fork/tests/test_private_run.py` builds a small tree from scratch to prove that each group is covered, that a
stored mismatch stops the run before any command, and that `--require` fails on a skip. It needs no game files and
runs in public CI. `task scripts` also runs `private_integrity.py --check` over the real tree in the required
"Pipeline Complete" check, so a stale hash fails a pull request before it merges; the hash file and the four
protected paths are owned by the maintainer in `.github/CODEOWNERS`.

## Recorded results that need no data

The recorded real-game math results (`crates/nvse/src/commands/math.rs`) are split: the cases that do not use `tan` or `tanh`
run on every OS; the four `tan` and `tanh` cases are ignored off Windows, where the host libm's last bit differs, so the
`scripting` component also runs on Windows (`windows = "required"` in `.fork/components.toml`).

## Upstream-owned test files the fork edits

The fork is the product and upstream is an input: it may edit upstream-owned test files. Each one is listed in
`.fork/overlay-paths` (the last block): `verify-sync.sh` leaves it out of the comparison, and the replay keeps the fork's version
of it and lists it in the sync summary ([`KEEPING-IN-SYNC.md`](KEEPING-IN-SYNC.md)). Review that list after a sync: upstream's
change to such a file is not applied and may need porting by hand.
