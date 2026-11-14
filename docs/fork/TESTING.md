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

## Recorded results that need no data

The recorded real-game math results (`crates/nvse/src/commands/math.rs`) are split: the cases that do not use `tan` or `tanh`
run on every OS; the four `tan` and `tanh` cases are ignored off Windows, where the host libm's last bit differs, so the
`scripting` component also runs on Windows (`windows = "required"` in `.fork/components.toml`).

## Upstream-owned test files the fork edits

The fork is the product and upstream is an input: it may edit upstream-owned test files. Each one is listed in
`.fork/overlay-paths` (the last block): `verify-sync.sh` leaves it out of the comparison, and the replay keeps the fork's version
of it and lists it in the sync summary ([`KEEPING-IN-SYNC.md`](KEEPING-IN-SYNC.md)). Review that list after a sync: upstream's
change to such a file is not applied and may need porting by hand.
