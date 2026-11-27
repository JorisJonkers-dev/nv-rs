<!-- SPDX-License-Identifier: GPL-3.0-only -->
# The lock-free maps, moved by default (nv-rs#946)

Fork-only. The lockfree family is in its K2 form (objects in host memory, guest bytes freed) when
`NV_TYPED` is unset ([`ENGINE_CRATE.md`](../ENGINE_CRATE.md), "Moved families"). Rollbacks:
`NV_TYPED=lockfree` (stage 1), `lockfree:moved` (K1), `lockfree:dual`, `lockfree:off`.

`crates/engine/tests/typed_lockfree.rs` builds its own maps. Its `forms` module checks that every form
lives alike, that the default is the moved form, that untyped code reads the same bytes as in stage 1,
and that K2 holds under real threads (the caller and three helpers, overlapping buckets).

A recorded run compared with the original's (`NV_TRACE_OUT`, `NV_TRACE_LIST`) has every family off unless
`NV_TYPED` says otherwise, so such a run says nothing about K2: name the form with `NV_TYPED` there.

The test `the_family_in_k2_holds_under_real_threads` runs the caller and three helpers (real threads,
`real_threads::fork_join`) over one map in three joins: first each thread on keys of its own (the keys
meet in buckets, not in keys), then all threads on the same keys, where only what every interleaving
allows is checked (a find that hits holds the stored value, a miss leaves the out word alone, a remove
answers 0 or 1); then one thread removes what is left and the count, a find on every key and the slab
are checked. The allocator, the lock and the compare-and-swap stand-ins are real atomics. The race
census (`NV_RACE_CENSUS`) is not on in that test.

## Evidence before the default

Keep each phase's evidence under its label; triage a failure before starting the next phase. Take
the lock on `~/Games/Fallout New Vegas RS/game.lock` (the scene tool's `--game-lock` does, and starts
every run under `gamerun`) and use scratch copies of saves only. Until the crash in the Havok thread
under the stepped scene runs is fixed, add `--env NV_HAVOK_THREAD=0` to every run below.

1. **Dual runs.** `nv-rs-dev scene soak --places docs/fork/scenes/bench-quick.toml --frames 300 --env NV_TYPED=lockfree:dual`:
   every `Dual run:` line says 0 differed.
2. **Handle-renamed stream comparison.** Record the Direct3D 9 stream of one place (`--env NV_D3D9_RECORD=FILE`,
   the scene runner sets `NV_REPEATABLE`) in stage 1 (`NV_TYPED=lockfree`) and in K2 (`NV_TYPED` unset), and compare with
   `cargo run --release -p d3d9-stream --example record_compare -- A B`. A second run of each form first,
   to know the form is repeatable. Any difference stops the change.
3. **Save and co-save round trips.** By hand (RELEASE-TEST step 5): there is no scripted save. The engine tests
   `typed_save`, `save_maps`, `save_changes_map` and `typed_lockfree`'s save check run with the default.
4. **Shuffled schedules.** `bench-quick.toml`, 200 frames, K2 by default, with `--env NV_THREADS=shuffle:N`
   for N from 1 to 20 (the scene runner's threads are stepped; shuffled orders them). A pass is exit status 0
   and no stop.
5. **Soaks.** `nv-rs-dev scene soak --places docs/fork/scenes/soak-far-moves.toml --out OUT --frames 300 --game-lock "$GAME" --label lockfree-moved`;
   `soak-rivet-ne.toml` needs the Capital Wasteland data (it names `RivetCityMapMarker`) and stops at its first
   place without it. A pass is every place reached, exit status 0, zero stops and zero error lines.
6. **Decision.** The default stays on while phases 1 to 5 pass. A failure is rolled back with
   `NV_TYPED=lockfree` and triaged; stepped mode stays the reference (ADR-0015).

## Where the evidence stands (2026-11-27, nv-rs#946)

Phases 1, 4 and 5 (far moves) pass. Phase 2 does not: the stream of K1 and of K2 differs from stage 1's
at command 10629 of one place, repeatably (stage 1 against itself and K2 against itself are alike). The
stepped threads' time slice counts calls by address; a moved form does in place what stage 1 calls by
address (a link's compare-and-swap, a bucket array's vector constructor, the node and entry constructors,
the allocator in K2), so the same slice ends at other calls and the stepped threads take their turns at
other points. K1 against stage 1: the first differing call is the compare-and-swap `004491c0` with the
call it makes (`0040fbe0`), absent from K1. K2 also frees the guest bytes, so the allocator's own calls differ.
With slicing off (`NV_SLICE=0`) K2 still differs from stage 1 (at command 16511, a `Destroy` of another object),
so slicing is not the only cause there.
