//! The forms family's maps (`011c54c0`, the form map; `011c54c8`, the editor id map) and the other
//! chained hash tables that share their bodies (nv-rs#951, stage 2 of the forms family): what the
//! family and the translations that read a map share ([`moved`]'s machinery, one K1 stub).
//!
//! A map is an `NiTPointerMap` (+0 vtable, +4 bucket count, +8 bucket table, +0xc item count); an
//! item is next (+0), key (+4), value (+8). Only the bucket table moves: it is one slab block of the
//! family's own, and the map object stays a K1 stub whose +8 holds poison while the map lives. The
//! object has no spare word, so the stub's record (the table's handle at +8) is named by the item
//! count at +0xc, whose value lives in the record at +0xc while the map is moved. The count is the
//! word to give up because every reader of a map by its bytes reads the pair bucket count (+4) and
//! bucket table (+8) (the jip co-save's `map_entry`, the `GetAllForms` walk, the iteration steps
//! `004b9ba0` and `006b7f20`, the lookup and `SetAt` bodies), and none reads the count: it is
//! written by `SetAt`, `RemoveAt` and the removal of every item, all typed by the family, and only
//! the maps with no record keep it in the guest block. Moving +4 instead would put a handle in the
//! word every hash function and every reader divides by; and the table is the word that must be
//! redirected (poison), so it cannot also carry the handle.
//!
//! Untyped code that read the table reads the head of a bucket's chain through [`bucket`].
//! A map without a record (any other map of the same layout) is the guest map the translation worked
//! on, word for word.

use super::moved::{is_handle, Store, K1, POISON_BASE};
use super::FamilyId;
use crate::Engine;

/// The map family's bit (`engine_typed::forms::map::FORM_MAP`).
pub const FAMILY: FamilyId = FamilyId::new(49);

/// The map object's K1 stub: the record is the map's layout in the slab, the moved field the bucket
/// table (+8), the spare word the item count (+0xc).
pub const STUB: K1 = K1 {
    size: 0x10,
    spare: 0x0c,
    moved: &[TABLE],
};

/// The bucket count (+4), bucket table (+8) and item count (+0xc).
pub const BUCKETS: u32 = 0x04;
pub const TABLE: u32 = 0x08;
pub const COUNT: u32 = 0x0c;

/// The tags of the family's slab blocks, for the heap census.
pub mod tag {
    pub const RECORD: u16 = 1;
    pub const TABLE: u16 = 2;
}

/// The release of a bucket table (`cdecl(block)`).
const FREE_TABLE: u32 = 0x00aa_10f0;

/// The family's store for `map`: the family's when `map` is a moved one (its record attached by
/// [`attach`]), stage 1's otherwise.
pub fn store(e: &Engine, map: u32) -> Store {
    let s = e.typed_store(FAMILY);
    let moved = match s {
        Store::Guest => false,
        Store::Moved { .. } => STUB.record(e, map).is_some(),
        Store::Dual { moved, .. } => moved.dual.mirror_of(map.wrapping_add(TABLE)).is_some(),
    };
    if moved {
        s
    } else {
        Store::Guest
    }
}

/// Gives the map at `map` (a block made, not yet constructed) its record, in the stage-2 forms of
/// the family; nothing in stage 1.
pub fn attach(e: &Engine, map: u32) {
    STUB.attach(e, e.typed_store(FAMILY), map, tag::RECORD);
}

/// The bucket table of `map` in store `s` (from [`store`]): the table's handle where the map is
/// moved, its guest address otherwise; read and written through `s`.
pub fn table(e: &Engine, s: Store, map: u32) -> u32 {
    s.load(e, STUB.place(e, s, map, TABLE))
}

/// Sets the bucket table of `map`.
pub fn set_table(e: &Engine, s: Store, map: u32, v: u32) {
    s.store(e, STUB.place(e, s, map, TABLE), v)
}

/// The item count: the record's word where the map is moved (the guest word is the record's
/// handle), the guest word otherwise.
pub fn count(e: &Engine, s: Store, map: u32) -> u32 {
    match (s, STUB.record(e, map)) {
        (Store::Moved { moved, .. }, Some(r)) => moved.slab.load(r.wrapping_add(COUNT)),
        _ => e.mem.u32(map.wrapping_add(COUNT)),
    }
}

/// Sets the item count.
pub fn set_count(e: &Engine, s: Store, map: u32, v: u32) {
    match (s, STUB.record(e, map)) {
        (Store::Moved { moved, .. }, Some(r)) => moved.slab.store(r.wrapping_add(COUNT), v),
        _ => e.mem.set_u32(map.wrapping_add(COUNT), v),
    }
}

/// The head of the chain in bucket `index` of `map`, for code that reads a map by its bytes (the
/// word at `map + 8` is the table's address, or the poison where the map is moved, and the head is
/// read from the table in the slab).
pub fn bucket(e: &Engine, map: u32, index: u32) -> u32 {
    let table = e.mem.u32(map.wrapping_add(TABLE));
    if table < POISON_BASE {
        return e.mem.u32(table.wrapping_add(index.wrapping_mul(4)));
    }
    let moved = e.moved().expect("a poisoned table address needs the slab");
    let record = STUB
        .record(e, map)
        .expect("a poisoned table address names a moved map");
    let block = moved.slab.load(record.wrapping_add(TABLE));
    moved.slab.load(block.wrapping_add(index.wrapping_mul(4)))
}

/// The base destructor's release of the bucket table (`00486a60`, after the removal of every
/// item): the table freed through the map's store, `free` being the call by address of stage 1
/// (`00aa10f0`, with the table); a K2 table skips the call, so its step is charged. Then, last, the
/// moved map's record is taken away.
pub fn release_table(e: &mut Engine, map: u32, free: impl FnOnce(&mut Engine, u32)) {
    let s = store(e, map);
    let block = table(e, s, map);
    if matches!(s, Store::Moved { shadows: false, .. }) && is_handle(block) {
        e.typed_charge(&[FREE_TABLE]);
    }
    s.free(e, block, free);
    STUB.detach(e, s, map);
}
