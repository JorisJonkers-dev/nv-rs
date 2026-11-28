//! The chained hash tables of the forms (`NiTPointerMap` and the string maps over it): the form map
//! (`011c54c0`) and the editor id map (`011c54c8`) and every other map of the same layout. A map is
//! +0 vtable, +4 bucket count, +8 bucket table, +0xc item count; an item is next (+0), key (+4),
//! value (+8). Stage 1 of nv-rs#951: the removal of one key (`00405430`), the removal of every item
//! (`00438af0`) and the editor id map's constructor chain (`00486ab0` through `00486fa0`), each making
//! the calls by address (the hash, equality, value clear and delete slots) its translation makes, in
//! its order.
//!
//! Stage 2 (nv-rs#951): the map object is `engine_core::typed::form_map::STUB` (a K1 stub): the
//! vtable, bucket count and the chains' items stay in guest memory, the bucket table (+8) is a slab
//! block of the family's own, and the item count (+0xc, the spare word) lives in the record. Only
//! the map that was given a record ([`engine_core::typed::form_map::attach`], from
//! `create_registries`) is moved; every other map these functions are handed has none and is the
//! guest map the translation worked on, word for word. Each function makes the calls by address its
//! translation makes, in its order, so the stepped threads' time slice is stage 1's.

use engine_core::typed::form_map::{self, tag, BUCKETS, FAMILY};
use engine_core::typed::moved::is_handle;
use engine_core::typed::{Family, Threads};
use engine_core::{args, typed_call, Engine, Ptr};
use engine_core::{entry, AbiFn};

use crate::crt;

/// The vtable slots (byte offsets): the hash of a key, the equality of two keys, the clearing of an
/// item's value and the deletion of the item.
const HASH_SLOT: u32 = 4;
const EQUAL_SLOT: u32 = 8;
const CLEAR_SLOT: u32 = 0x10;
const DELETE_SLOT: u32 = 0x18;
/// The allocator of the bucket table (`cdecl(bytes)`) and the CRT's `memset` wrapper that zeroes it.
const ALLOCATE_BUCKETS: u32 = 0x00aa_1070;
const MEMSET_WRAPPER: u32 = 0x0040_3d30;

const BUCKET_MAP_VTABLE: u32 = 0x0101_cb18;
const POINTER_MAP_VTABLE: u32 = 0x0101_caf8;
const STRING_MAP_VTABLE: u32 = 0x0101_cad8;
const CASE_INSENSITIVE_MAP_VTABLE: u32 = 0x0101_cb38;
const EDITOR_ID_MAP_VTABLE: u32 = 0x0101_cab8;

// Typed from 00438af0
/// Removes every item: for each bucket in turn (the bucket count read again each round) the chain's
/// head is unlinked and handed to the value clear (slot 0x10) and the delete (slot 0x18), the table
/// read again each time; the item count is then zero.
pub fn clear(e: &mut Engine, this: u32) {
    let s = form_map::store(e, this);
    let mut bucket = 0u32;
    while bucket < e.mem.u32(this.wrapping_add(BUCKETS)) {
        loop {
            let table = form_map::table(e, s, this);
            let slot = table.wrapping_add(bucket.wrapping_mul(4));
            let item = s.load(e, slot);
            if item == 0 {
                break;
            }
            let next = e.mem.u32(item);
            s.store(e, slot, next);
            e.vcall(this, CLEAR_SLOT, &args![item]);
            e.vcall(this, DELETE_SLOT, &args![item]);
        }
        bucket = bucket.wrapping_add(1);
    }
    form_map::set_count(e, s, this, 0);
}

// Typed from 00405430
/// `RemoveAt(key)`: the key's bucket (slot 4); the item whose key the equality (slot 8) accepts is
/// unlinked, its value cleared and the item deleted (slots 0x10, 0x18) and counted out. Whether an
/// item was removed.
pub fn remove_at(e: &mut Engine, this: u32, key: u32) -> bool {
    let s = form_map::store(e, this);
    let index = e.vcall(this, HASH_SLOT, &args![key]).u32();
    let table = form_map::table(e, s, this);
    let node = s.load(e, table.wrapping_add(index.wrapping_mul(4)));
    if node == 0 {
        return false;
    }
    let node_key = e.mem.u32(node.wrapping_add(4));
    if e.vcall(this, EQUAL_SLOT, &args![key, node_key]).u8() != 0 {
        let table = form_map::table(e, s, this);
        let next = e.mem.u32(node);
        s.store(e, table.wrapping_add(index.wrapping_mul(4)), next);
        release_item(e, s, this, node);
        return true;
    }
    let mut previous = node;
    let mut current = e.mem.u32(previous);
    while current != 0 {
        let current_key = e.mem.u32(current.wrapping_add(4));
        if e.vcall(this, EQUAL_SLOT, &args![key, current_key]).u8() != 0 {
            break;
        }
        previous = current;
        current = e.mem.u32(current);
    }
    if current == 0 {
        return false;
    }
    let next = e.mem.u32(current);
    e.mem.set_u32(previous, next);
    release_item(e, s, this, current);
    true
}

/// The tail both removals share: the value clear, the delete and the item count down.
fn release_item(e: &mut Engine, s: engine_core::typed::moved::Store, this: u32, node: u32) {
    e.vcall(this, CLEAR_SLOT, &args![node]);
    e.vcall(this, DELETE_SLOT, &args![node]);
    let count = form_map::count(e, s, this);
    form_map::set_count(e, s, this, count.wrapping_sub(1));
}

// Typed from 00486ab0
/// The editor id map's constructor `(buckets, flag)`: the case insensitive map's, then its vtable.
pub fn construct_editor_id_map(e: &mut Engine, this: u32, buckets: u32, flag: u8) -> u32 {
    case_insensitive_body(e, this, buckets, flag);
    e.mem.set_u32(this, EDITOR_ID_MAP_VTABLE);
    this
}

// Typed from 00486ee0
/// The case insensitive map's constructor: the string map's, then its vtable.
pub fn construct_case_insensitive_map(e: &mut Engine, this: u32, buckets: u32, flag: u8) -> u32 {
    case_insensitive_body(e, this, buckets, flag)
}

/// [`construct_case_insensitive_map`]'s body, which the translation's constructors above it run as
/// a Rust call (no step); so do the three below.
fn case_insensitive_body(e: &mut Engine, this: u32, buckets: u32, flag: u8) -> u32 {
    string_body(e, this, buckets, flag);
    e.mem.set_u32(this, CASE_INSENSITIVE_MAP_VTABLE);
    this
}

// Typed from 00486f40
/// The string map's constructor: the pointer map's, its vtable, and the flag byte (+0x10 only).
pub fn construct_string_map(e: &mut Engine, this: u32, buckets: u32, flag: u8) -> u32 {
    string_body(e, this, buckets, flag)
}

fn string_body(e: &mut Engine, this: u32, buckets: u32, flag: u8) -> u32 {
    pointer_body(e, this, buckets);
    e.mem.set_u32(this, STRING_MAP_VTABLE);
    e.mem.set_u8(this.wrapping_add(0x10), flag);
    this
}

// Typed from 00486f70
/// The pointer map's constructor: the bucket map's, then its vtable.
pub fn construct_pointer_map(e: &mut Engine, this: u32, buckets: u32) -> u32 {
    pointer_body(e, this, buckets)
}

fn pointer_body(e: &mut Engine, this: u32, buckets: u32) -> u32 {
    bucket_body(e, this, buckets);
    e.mem.set_u32(this, POINTER_MAP_VTABLE);
    this
}

// Typed from 00486fa0
/// The bucket map's constructor: its vtable, the bucket count (+4), no items (+0xc) and a zeroed
/// table of `buckets` words (+8, the count read again after the allocation).
pub fn construct_bucket_map(e: &mut Engine, this: u32, buckets: u32) -> u32 {
    bucket_body(e, this, buckets)
}

fn bucket_body(e: &mut Engine, this: u32, buckets: u32) -> u32 {
    construct_body(e, this, BUCKET_MAP_VTABLE, buckets)
}

/// The body every map base constructor shares (`00486fa0`, and `00486990` of the form map, which
/// stores another vtable): the vtable, the bucket count, no items and a zeroed table of `buckets`
/// words, made in the map's store (a slab block where the map is moved: it is zero already, and
/// the call that zeroes it is charged as the step it is; a K2 table also skips the allocator's
/// call, charged too).
pub(crate) fn construct_body(e: &mut Engine, this: u32, vtable: u32, buckets: u32) -> u32 {
    let s = form_map::store(e, this);
    e.mem.set_u32(this, vtable);
    e.mem.set_u32(this.wrapping_add(BUCKETS), buckets);
    form_map::set_count(e, s, this, 0);
    if matches!(
        s,
        engine_core::typed::moved::Store::Moved { shadows: false, .. }
    ) {
        e.typed_charge(&[ALLOCATE_BUCKETS]);
    }
    let bytes = buckets.wrapping_shl(2);
    let table = s.alloc(e, bytes, tag::TABLE, |e| {
        e.call(ALLOCATE_BUCKETS, &args![bytes]).u32()
    });
    form_map::set_table(e, s, this, table);
    let bytes = e.mem.u32(this.wrapping_add(BUCKETS)).wrapping_shl(2);
    if is_handle(table) {
        e.typed_charge(&[MEMSET_WRAPPER]);
    } else {
        typed_call!(
            crt::FAMILY,
            0x0040_3d30,
            crt::mem::memset_wrapper(e, Ptr::new(table), 0, bytes)
        );
        s.resync(e, table, bytes >> 2);
    }
    this
}

/// `NV_TYPED=formmap`: the maps' functions (a family of its own beside `forms`, so a test of the
/// forms' unit may keep its doubles at these addresses). On unless `NV_TYPED` says otherwise; stage 2
/// for the form map: `formmap:dual`, `formmap:moved` (K1), `formmap:freed` (K2).
pub static FORM_MAP: Family = Family {
    name: "formmap",
    id: FAMILY,
    past_gate: true,
    threads: Threads::Both,
    stage2: true,
    funcs,
};

/// The family's functions, by exe address.
fn funcs() -> Vec<(u32, AbiFn)> {
    vec![
        entry!(0x0040_5430, remove_at(u32, u32) -> bool),
        entry!(0x0043_8af0, clear(u32)),
        entry!(0x0048_6ab0, construct_editor_id_map(u32, u32, u8) -> u32),
        entry!(0x0048_6ee0, construct_case_insensitive_map(u32, u32, u8) -> u32),
        entry!(0x0048_6f40, construct_string_map(u32, u32, u8) -> u32),
        entry!(0x0048_6f70, construct_pointer_map(u32, u32) -> u32),
        entry!(0x0048_6fa0, construct_bucket_map(u32, u32) -> u32),
    ]
}
