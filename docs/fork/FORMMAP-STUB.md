<!-- SPDX-License-Identifier: GPL-3.0-only -->
# The form map's K1 stub (nv-rs#951)

Fork-only. The `formmap` family ([`typed/form_map.rs`](../../crates/engine-core/src/typed/form_map.rs)) moves the bucket
table of the form map `011c54c0` to host memory in its K1 form. Not moved by default: `NV_TYPED=formmap:dual`,
`formmap:moved` (K1) and `formmap:freed` (K2) choose the form; with no switch the stage-1 store is used.

## The stub's layout and the choice of its spare word

The map object is 0x10 bytes: +0 vtable, +4 bucket count, +8 bucket table, +0xc item count. A K1 stub needs one word that no
untyped code reads to hold the record's handle, and one moved field (poison in the guest block, the value in the record).
The object has no spare word, so one field has to give up its guest value.

- The table (+8) is the moved field: it is the word that has to be redirected, since it is what points at the memory that moves.
- The item count (+0xc) is the spare word. Its value lives in the record at +0xc; the guest word is the record's handle.

Evidence (every reader of a map by its bytes, read on 2026-11-28):

| Reader | Reads |
| --- | --- |
| `00853130` / `00a1e380` lookup (`dispatch/refs.rs`), `00844700` set (`save/refs.rs`) | +4 through the hash slot, +8 |
| `004b9ba0` first position, `006b7f20` next entry (`globalfunc.rs`, `navmeshinfomap.rs`); callers `playercharacter_p3.rs` 5871, `tesscriptfunctions_p8.rs` 2732, `tesdatahandler.rs` | +4, +8 |
| `jip` co-save `map_entry`, `ln_fn_game_data.rs` `all_forms` | +4, +8 |
| `tesform.rs` (forms unit), `playercharacter_p3.rs` 5539, `tesscriptfunctions_p8.rs` 599, `tesdatahandler.rs` 288, `nvse/args.rs` 135 | the pointer only, then the calls above or `004839c0` |
| `00405430` remove, `00438af0` clear, `00844700` set, the constructors | the count, written |

Every reader of the bytes reads the pair bucket count (+4) and table (+8); none reads the count (+0xc), which only
`SetAt`, `RemoveAt` and the removal of every item write, all typed in the family (and the constructors, which zero it). Moving
the bucket count instead would put a handle in the word every hash function and every reader divides by. So the table
is the moved field and the count the spare word; readers reach the table through `form_map::bucket`.

A map with no record (every other map of the same layout: the pathing and menu maps, the save's id maps, the string maps) is
the guest map the translations have always worked on, word for word.

## What changed for readers

`form_map::bucket(e, map, index)` reads the head of a bucket's chain whether the map is moved or not. `004b9ba0`, `006b7f20`,
the jip co-save's id map walk and the walk of every form use it. The base destructor `00486a60` frees the table through
`form_map::release_table` and takes the record away. The editor id map `011c54c8` (the string-map chain `00486dc0`,
`00486d60`, `00486ca0`, `00486c80`) is the next step (PR 3).

## Evidence

`crates/engine/tests/typed_form_map.rs`: the differential test of each function against its translation, and the map's life
(made by `create_registries`, filled with keys that share buckets, set again, looked up, read by bucket, removed from, cleared,
destroyed) in stage 1, default, dual, K1 and K2: same answers and same bytes untyped code reads, K1 makes the same guest
allocations and takes the same time-slice steps as stage 1, dual compares with 0 differences, poison held while the map lives,
nothing left in the slab.
