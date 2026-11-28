//! The chained hash tables of the forms (`NiTPointerMap` and the string maps over it): the form map
//! (`011c54c0`) and the editor id map (`011c54c8`) and every other map of the same layout. A map is
//! +0 vtable, +4 bucket count, +8 bucket table, +0xc item count; an item is next (+0), key (+4),
//! value (+8). Stage 1 of nv-rs#951: the removal of one key (`00405430`), the removal of every item
//! (`00438af0`) and the editor id map's constructor chain (`00486ab0` through `00486fa0`), each making
//! the calls by address (the hash, equality, value clear and delete slots) its translation makes, in
//! its order.

use engine_core::typed::form_map::FAMILY;
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
/// The allocator of the bucket table (`cdecl(bytes)`).
const ALLOCATE_BUCKETS: u32 = 0x00aa_1070;

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
    let mut bucket = 0u32;
    while bucket < e.mem.u32(this.wrapping_add(4)) {
        loop {
            let table = e.mem.u32(this.wrapping_add(8));
            let slot = table.wrapping_add(bucket.wrapping_mul(4));
            let item = e.mem.u32(slot);
            if item == 0 {
                break;
            }
            let next = e.mem.u32(item);
            e.mem.set_u32(slot, next);
            e.vcall(this, CLEAR_SLOT, &args![item]);
            e.vcall(this, DELETE_SLOT, &args![item]);
        }
        bucket = bucket.wrapping_add(1);
    }
    e.mem.set_u32(this.wrapping_add(0xc), 0);
}

// Typed from 00405430
/// `RemoveAt(key)`: the key's bucket (slot 4); the item whose key the equality (slot 8) accepts is
/// unlinked, its value cleared and the item deleted (slots 0x10, 0x18) and counted out. Whether an
/// item was removed.
pub fn remove_at(e: &mut Engine, this: u32, key: u32) -> bool {
    let index = e.vcall(this, HASH_SLOT, &args![key]).u32();
    let table = e.mem.u32(this.wrapping_add(8));
    let node = e.mem.u32(table.wrapping_add(index.wrapping_mul(4)));
    if node == 0 {
        return false;
    }
    let node_key = e.mem.u32(node.wrapping_add(4));
    if e.vcall(this, EQUAL_SLOT, &args![key, node_key]).u8() != 0 {
        let table = e.mem.u32(this.wrapping_add(8));
        let next = e.mem.u32(node);
        e.mem
            .set_u32(table.wrapping_add(index.wrapping_mul(4)), next);
        release_item(e, this, node);
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
    release_item(e, this, current);
    true
}

/// The tail both removals share: the value clear, the delete and the item count down.
fn release_item(e: &mut Engine, this: u32, node: u32) {
    e.vcall(this, CLEAR_SLOT, &args![node]);
    e.vcall(this, DELETE_SLOT, &args![node]);
    let count = e.mem.u32(this.wrapping_add(0xc));
    e.mem.set_u32(this.wrapping_add(0xc), count.wrapping_sub(1));
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
    e.mem.set_u32(this, BUCKET_MAP_VTABLE);
    e.mem.set_u32(this.wrapping_add(4), buckets);
    e.mem.set_u32(this.wrapping_add(0xc), 0);
    let table = e
        .call(ALLOCATE_BUCKETS, &args![buckets.wrapping_shl(2)])
        .u32();
    e.mem.set_u32(this.wrapping_add(8), table);
    let bytes = e.mem.u32(this.wrapping_add(4)).wrapping_shl(2);
    typed_call!(
        crt::FAMILY,
        0x0040_3d30,
        crt::mem::memset_wrapper(e, Ptr::new(table), 0, bytes)
    );
    this
}

/// `NV_TYPED=formmap`: the maps' functions (a family of its own beside `forms`, so a test of the
/// forms' unit may keep its doubles at these addresses). On unless `NV_TYPED` says otherwise; stage 1.
pub static FORM_MAP: Family = Family {
    name: "formmap",
    id: FAMILY,
    past_gate: true,
    threads: Threads::Both,
    stage2: false,
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
