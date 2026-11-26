<!-- SPDX-License-Identifier: GPL-3.0-only -->
# The lock-free maps, moved by default (nv-rs#946)

Fork-only. The lockfree family is in its K2 form (objects in host memory, guest bytes freed) when
`NV_TYPED` is unset ([`ENGINE_CRATE.md`](../ENGINE_CRATE.md), "Moved families"). Rollbacks:
`NV_TYPED=lockfree` (stage 1), `lockfree:moved` (K1), `lockfree:dual`, `lockfree:off`.

`crates/engine/tests/typed_lockfree.rs` builds its own maps. Its `forms` module checks that every form
lives alike, that the default is the moved form, that untyped code reads the same bytes as in stage 1,
and that K2 holds under real threads (the caller and three helpers, overlapping buckets).

## Evidence before the default

Keep each phase's evidence under its label; triage a failure before starting the next phase. Take
the lock on `~/Games/Fallout New Vegas RS/game.lock` with `flock -x` around each run and start it
under `gamerun <label> --who <name> -- ...`. Use scratch copies of saves only.

1. **Dual runs.** `docs/fork/scenes/bench-quick.toml` with `NV_TYPED=lockfree:dual` against stage 1
   (`NV_TYPED=lockfree`). The status line says 0 differences.
2. **Handle-renamed stream comparison.** Record the Direct3D 9 stream of the same stepped route
   (`NV_REPEATABLE`, `NV_D3D9_RECORD`) in stage 1 and in K2 and compare them with the
   `record_compare` example of `d3d9-stream`, which renames handles. A difference is a stop.
3. **Save and co-save round trips.** Load a scratch copy of a save, play a few frames, save, reload,
   and compare the save and its co-save with the stage 1 run's.
4. **Shuffled schedules.** The same scene at `NV_THREADS=shuffle:1` through `NV_THREADS=shuffle:20`
   in K2. A pass is exit status 0 and no stop. A race the original has belongs in `fixes`; one the
   port adds is a stop.
5. **Soaks.** `nv-rs-dev scene soak --places docs/fork/scenes/soak-far-moves.toml --out /tmp/lockfree-soak --frames 300 --game-lock "$GAME" --label lockfree-moved`
   and the same with `docs/fork/scenes/soak-rivet-ne.toml`. A pass is every place reached, exit
   status 0, zero stops and zero error lines.
6. **Decision.** The default stays on while phases 1 to 5 pass. A failure is rolled back with
   `NV_TYPED=lockfree` and triaged; stepped mode stays the reference (ADR-0015).
