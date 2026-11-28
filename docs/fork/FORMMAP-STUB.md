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

## The editor id map (`011c54c8`)

The editor id map is a case insensitive string map of 0x14 bytes: the form map's four words and the flag byte at +0x10 (whether
the map owns copies of its keys). `create_registries` attaches the record (the first 0x10 bytes; +0x10 stays in the block) before
the constructor chain `00486ab0` .. `00486fa0`, whose body makes the table in the record's store. The flag byte is not a
spare word candidate: it is read by `00470200` and by the destructor walk `00486ca0`, and it is one byte.

Does the stub-field choice hold for the string map? Yes. Readers and writers of this map by its bytes, read on 2026-11-28:

| Code | Reads or writes |
| --- | --- |
| `00853130` lookup (`GetFormByEditorID` `00483a00`), `00405430` remove, `00438af0` clear | +4, +8 (the count only written) |
| `00470200` `SetAt` (the data handler's `register_loaded_form`, the topic loader `0061a040`) | +4, +8, +0x10, count written (+0xc) |
| `00486ca0` (frees the owned keys), `00486dc0` (frees the table) | +4, +8, +0x10 |
| jip `editor_id_lookup_at` | +4, +8 |
| jip `set_editor_id` (`gl_lights.rs`, the light form's setter) | +8, and the count **written** (+0xc) |
| `tesform.rs`, `tesdatahandler.rs` 4514/8378, `run_00619f20.rs` 140 | the pointer only, then `00470200` or `00853130` |

Nothing reads +0xc to use it (the one other touch is jip's setter incrementing the count, which now goes through
`form_map::count` and `set_count`: a write that would have put the count over the record's handle). So the table is the moved
field and the count the spare word here too; the stub is the form map's (`form_map::STUB`) and the map family is one family.

What changed: `00470200` reads and writes through `form_map::store`, `table`, `count` and `set_count` (maps with no record take
the guest path word for word, so the other users of `00470200`, such as the effect item's setting map, are unchanged);
`00486ca0` reads each bucket's head through `form_map::bucket`; `00486dc0` frees the table through `form_map::release_table`;
jip's lookup reads the head through `bucket`; jip's setter goes through the store. Evidence:
`the_editor_id_map_lives_alike_in_every_form` in `typed_form_map.rs` runs the map's life (set with texts that share buckets, a text
set again in another case, lookups by `00853130` and `00483a00`, removals at the head, middle, tail and absent, clear, refill,
destruction with items by the chain `00486c30` .. `00486dc0` and by `00483b80`) in stage 1, default, dual, K1 and K2 with the
same checks as the form map's.
