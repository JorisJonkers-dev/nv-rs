<!-- SPDX-License-Identifier: GPL-3.0-only -->
# The lock-free maps, moved by default (nv-rs#946)

Fork-only. The lockfree family is in its K1 form (objects in host memory, the guest heap as in stage 1) when
`NV_TYPED` is unset ([`ENGINE_CRATE.md`](../ENGINE_CRATE.md), "Moved families"). Rollbacks:
`NV_TYPED=lockfree` (stage 1), `lockfree:freed` (K2, guest bytes freed), `lockfree:dual`, `lockfree:off`.

`crates/engine/tests/typed_lockfree.rs` builds its own maps. Its `forms` module checks that every form
lives alike, that the default is the moved form, that untyped code reads the same bytes as in stage 1,
and that both K1 (the default) and K2 (`lockfree:freed`) hold under real threads (the caller and three helpers, overlapping buckets).

A recorded run compared with the original's (`NV_TRACE_OUT`, `NV_TRACE_LIST`) has every family off unless
`NV_TYPED` says otherwise, so such a run says nothing about K1: name the form with `NV_TYPED` there.

The tests `the_default_family_holds_under_real_threads` (K1) and `the_family_in_k2_holds_under_real_threads` (K2)
run the caller and three helpers (real threads, `real_threads::fork_join`) over one map in three joins: first each
thread on keys of its own (the keys meet in buckets, not in keys), then all threads on the same keys, where only what
every interleaving allows is checked (a find that hits holds the stored value, a miss says nothing about the out word,
a remove answers 0 or 1); then one thread removes what is left and the count, a find on every key and the slab are
checked (slab empty, count 0, in each form). The helper pool is one per process, so each test runs the body in a
child process of its own (the same test binary, `race_child`). The allocator, the lock and the compare-and-swap
stand-ins are real atomics. The race census (`NV_RACE_CENSUS`) is not on in that test.

## Evidence before the default

Keep each phase's evidence under its label; triage a failure before starting the next phase. Take
the lock of the game folder you run from (the scene tool's `--game-lock` does, and starts
every run under `gamerun`) and use scratch copies of saves only. Until the crash in the Havok thread
under the stepped scene runs is fixed, add `--env NV_HAVOK_THREAD=0` to every run below.

1. **Dual runs.** `nv-rs-dev scene soak --places docs/fork/scenes/bench-quick.toml --frames 300 --env NV_TYPED=lockfree:dual`:
   every `Dual run:` line says 0 differed.
2. **Handle-renamed stream comparison.** Record the Direct3D 9 stream of one place (`--env NV_D3D9_RECORD=FILE`,
   the scene runner sets `NV_REPEATABLE`) in stage 1 (`NV_TYPED=lockfree`) and by default (K1; `NV_TYPED=lockfree:freed` for K2), and compare with
   `cargo run --release -p d3d9-stream --example record_compare -- A B`. A second run of each form first,
   to know the form is repeatable. Any difference stops the change.
3. **Save and co-save round trips.** The lockfree family does not take part in saves: the maps are the
   loaders' and managers' transient task tables (model, texture, lip sync, pathing, cell loads, queues), which no
   save walker or co-save record names (no save or co-save code in the repository refers to a map, its vtables or
   its entries, and `extender_move_inventory.py --check` finds no command that reads a moved field), so a saved
   world holds no map bytes and loading one makes new maps. What a save could see of a map is its guest block, and
   `the_bytes_untyped_code_reads_stay_as_stage_1_has_them` compares those bytes in K1 and K2 with stage 1 at every
   look through a map's life. The engine save tests (`typed_save`, `save_maps`, `save_changes_map`) run with the
   default. A by-hand save (RELEASE-TEST step 5) stays in the release test.
4. **Shuffled schedules.** `bench-quick.toml`, 200 frames, K1 by default, with `--env NV_THREADS=shuffle:N`
   for N from 1 to 20 (the scene runner's threads are stepped; shuffled orders them). A pass is exit status 0
   and no stop.
5. **Soaks.** `nv-rs-dev scene soak --places docs/fork/scenes/soak-far-moves.toml --out OUT --frames 300 --game-lock "$GAME" --label lockfree-moved`;
   `soak-rivet-ne.toml` needs the Capital Wasteland data (it names `RivetCityMapMarker`) and stops at its first
   place without it. A pass is every place reached, exit status 0, zero stops and zero error lines.
6. **Decision.** The default stays on while phases 1 to 5 pass. A failure is rolled back with
   `NV_TYPED=lockfree` and triaged; stepped mode stays the reference (ADR-0015).

## Where the evidence stands (2026-11-27, nv-rs#946)

Phases 1, 4 and 5 (far moves) pass. Phase 2 (the handle-renamed stream of goodsprings, 200 frames, repeatable):

- **Cause.** The stepped threads take their turns by steps of a time slice (and, after a signal, by the wake delay,
  which counts the same steps, so `NV_SLICE=0` does not remove it): every call by address and every direct
  `typed_call!` is one step. A moved form that does in place what stage 1 calls by address (a link's
  compare-and-swap `004491c0`, the bucket array's vector constructor `00401050` and its elements' constructors, the
  interface count's interlocked increment `0040b460`, the scan's `memcpy`, in K2 `operator new` and `operator delete`)
  took fewer steps, so the threads handed over elsewhere and the game created its resources in another order (first
  difference at command 10629; the `Destroy` of another object at command 16511 with `NV_SLICE=0` was the wake delay,
  the same cause).
- **Fix.** The moved forms take the steps of the calls they skip, in stage 1's order (`Engine::typed_charge`; the lists
  are `SWAP_LINK_STEPS`, `INCREMENT_STEPS`, `MEMCPY_STEPS`, `NEW_STEPS`, `DELETE_STEPS` in `lockfree/store.rs`; a vector
  constructor takes its own step and one for each element). They were found by logging the address of every step of
  stage 1, K1 and K2 for 80 million steps of the run and aligning the logs. `forms` in `typed_lockfree.rs` compares
  the address of every step of four maps' lives in stage 1, K1 and K2.
- **Result.** K1 against stage 1: the same 80 million steps, and the same stream (2,371,320 commands alike, repeatable).
  K2 is not exact: the allocator's own calls under `operator new` and `operator delete` (`00aa3e40`, `00aa4060` and
  what they call: `00ed2cd5`, `00ed17f4`, `00ed176d`) depend on what the guest heap holds, and K2's heap holds fewer
  blocks, so a few hundred steps in 80 million differ; the stream then differs at command 1,363,514 (it did at
  10,629 before). So the default is K1 (`MOVED_BY_DEFAULT`): the objects are in host memory, the guest heap and every
  step of the stepped threads are stage 1's. K2 stays as `NV_TYPED=lockfree:freed` for runs that need the bytes back
  and accept a stream that may differ.

## Census: untyped readers of a moved map (2026-11-27)

What untyped code can see of a K1 map: the guest block holds the vtable, bucket count (+8), count (+0x18) and lock
(+0x20) as stage 1 has them; the moved fields (+4, +0xc, +0x10, +0x14) hold family poison words (`0xFFFFxxxx`, the
never-mapped last 64 KB), and the record's slab handle sits in the spare word +0x1c, which nothing reads. The slab
range (`0x10000000..0x14000000`) is never mapped, so a guest read of a handle (a node, bucket array, interface or
iterator body) stops the run. So the slab handles are not in the moved fields; the fields hold poison.

- **Readers of the moved fields in the translated units.** The destructors read +0xc and +4: `0044c1e0`, `0044cae0`,
  `009064e0`, `00528be0` and the other map destructors are registered by the family at the same addresses, so the typed
  version runs. The constructors write the fields, and are the family's too (`0044b750`, `0044ba10`, `0044c270`,
  `0044c8e0`, `0044c920`, `00665cb0`, `00906160`); an untyped-built map has no record and stays a stage 1 guest map
  (`map_store`). The units that stay untyped (the scalar deleting destructors `009067f0`, `00528e10`, the wrappers
  `0044d8c0`..`0044de20`, the key compare `0044c1c0`, the loaders' own maps held at a loader's +8, +0xc, +0x10)
  call the typed destructors or hold a map pointer without reading the map's body; none reads a moved field or a
  body object.
- **Interface, node and cursor bodies.** `00529000` (clears a cursor's hazard slots) is untyped, and is called only from
  functions the family registers (`00906af0`, `00528ce0` and the like), which run typed; the untyped copies of those
  are replaced at their addresses.
- **Extender commands.** `python3 scripts/fork/extender_move_inventory.py --check` passes: "lock-free maps: none
  found" (no built-in command reads or writes a moved field, directly or through a helper).
- **Save and co-save.** No save or co-save walker refers to the family (see phase 3).
- **Result.** No untyped reader of a moved field or a slab object found, so no accessor change and no field excluded
  from K1. A reader missed by this static look would stop the run at its first read (the slab pages are never mapped;
  the poison words are checked at detach), which the runs below would show.

Run after the census (a fresh `nv-viewer` build of this branch, `bench-quick.toml`, 200 frames, no `NV_TYPED` so the
lockfree family is in K1, `NV_HAVOK_THREAD=0`, under `gamerun` and the game lock): `soak: 5 of 5 places soaked for 200
frames, exit status 0, 0 stops, 0 error lines`. No separate slab-range read guard exists; the guard is the never-mapped
slab pages, so a guest read of a slab handle would have stopped the run, and none did. Recordings were deleted after.
