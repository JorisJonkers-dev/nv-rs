//! The forms family's maps (`engine_typed::forms::map`, nv-rs#951): the differential test of each
//! function against its translation, in stage 1.
//!
//! The differential tests build a chained hash map in guest memory (+0 vtable, +4 bucket count, +8
//! bucket table, +0xc item count; an item is next, key, value); the vtable's slots (hash, equal,
//! value clear, delete) and the bucket table's allocator (`00aa1070`) are doubles.

use engine as _;
use engine_core::prelude::*;
use engine_core::typed::diff::{Diff, Input};
use engine_typed::forms::{map, map::FORM_MAP};

const ALLOCATE_BYTES: u32 = 0x00aa_1070;
const VTABLE: u32 = 0x0300_0000;
const HASH: u32 = 0xf010_0000;
const EQUAL: u32 = 0xf010_0004;
const CLEAR: u32 = 0xf010_0010;
const DELETE: u32 = 0xf010_0018;
const BUCKETS: u32 = 3;
/// The CRT's `memset` wrapper, which the bucket table's zeroing calls and the test does not name.
const MEMSET: u32 = 0x0040_3d30;

fn ret(eax: u32) -> Ret {
    Ret {
        eax,
        ..Ret::default()
    }
}

/// The doubles: the hash is the key modulo the bucket count, two keys are equal when the words are,
/// the value clear zeroes the item's value, the delete its key, and the allocator hands out memory.
fn doubles(e: &mut Engine) {
    let mut slots = vec![0; 0x18 / 4 + 1];
    slots[1] = HASH;
    slots[2] = EQUAL;
    slots[4] = CLEAR;
    slots[6] = DELETE;
    e.put_vtable(VTABLE, &slots);
    e.register(HASH, |_, a| ret(a[1] % BUCKETS));
    e.register(EQUAL, |_, a| ret(u32::from(a[1] == a[2])));
    e.register(CLEAR, |e, a| {
        e.mem.set_u32(a[1] + 8, 0);
        Ret::default()
    });
    e.register(DELETE, |e, a| {
        e.mem.set_u32(a[1] + 4, 0);
        Ret::default()
    });
    e.register(ALLOCATE_BYTES, |e, a| ret(e.mem.alloc(a[0].max(4))));
}

/// A map of three buckets holding `keys`, each in the bucket of its key, the later keys behind the
/// earlier ones in a chain.
fn map_of(e: &mut Engine, keys: &[u32]) -> u32 {
    let this = e.mem.alloc(0x14);
    let table = e.mem.alloc(BUCKETS * 4);
    e.mem.set_u32(this, VTABLE);
    e.mem.set_u32(this + 4, BUCKETS);
    e.mem.set_u32(this + 8, table);
    e.mem.set_u32(this + 0xc, keys.len() as u32);
    let mut tails = [0u32; BUCKETS as usize];
    for key in keys {
        let item = e.mem.alloc(12);
        e.mem.set_u32(item + 4, *key);
        e.mem.set_u32(item + 8, key + 100);
        let bucket = (key % BUCKETS) as usize;
        if tails[bucket] == 0 {
            e.mem.set_u32(table + 4 * bucket as u32, item);
        } else {
            e.mem.set_u32(tails[bucket], item);
        }
        tails[bucket] = item;
    }
    this
}

/// Empty, one item, a chain of three in one bucket, items spread over the buckets.
fn shape(e: &mut Engine, kind: u32) -> u32 {
    match kind {
        0 => map_of(e, &[]),
        1 => map_of(e, &[1]),
        2 => map_of(e, &[3, 6, 9]),
        _ => map_of(e, &[1, 2, 3, 4, 5, 6]),
    }
}

const SHAPES: &[u32] = &[0, 1, 2, 3];

#[test]
fn clear() {
    Diff::new(entry!(0x0043_8af0, map::clear(u32)))
        .family(&FORM_MAP)
        .inputs(&[Input::Of(SHAPES)])
        .setup(|e, case| {
            doubles(e);
            vec![shape(e, case.word(0))]
        })
        .run()
        .assert_same();
}

#[test]
fn remove_at() {
    Diff::new(entry!(0x0040_5430, map::remove_at(u32, u32) -> bool))
        .family(&FORM_MAP)
        .inputs(&[Input::Of(SHAPES), Input::Of(&[0, 1, 2, 3, 4, 6, 9, 12])])
        .setup(|e, case| {
            doubles(e);
            vec![shape(e, case.word(0)), case.word(1)]
        })
        .run()
        .assert_same();
}

#[test]
fn construct_bucket_map() {
    Diff::new(entry!(0x0048_6fa0, map::construct_bucket_map(u32, u32) -> u32))
        .family(&FORM_MAP)
        .goes_by_address(&[MEMSET])
        .inputs(&[Input::Of(&[0, 1, 7, 0x80])])
        .setup(|e, case| {
            doubles(e);
            vec![e.mem.alloc(0x14), case.word(0)]
        })
        .run()
        .assert_same();
}

#[test]
fn construct_pointer_map() {
    Diff::new(entry!(0x0048_6f70, map::construct_pointer_map(u32, u32) -> u32))
        .family(&FORM_MAP)
        .goes_by_address(&[MEMSET])
        .inputs(&[Input::Of(&[0, 1, 7, 0x80])])
        .setup(|e, case| {
            doubles(e);
            vec![e.mem.alloc(0x14), case.word(0)]
        })
        .run()
        .assert_same();
}

#[test]
fn construct_string_map() {
    Diff::new(entry!(0x0048_6f40, map::construct_string_map(u32, u32, u8) -> u32))
        .family(&FORM_MAP)
        .goes_by_address(&[MEMSET])
        .inputs(&[Input::Of(&[0, 1, 7, 0x80]), Input::Of(&[0, 1, 0xff])])
        .setup(|e, case| {
            doubles(e);
            let this = e.mem.alloc(0x14);
            e.mem.write(this, &[0xee; 0x14]);
            vec![this, case.word(0), case.word(1)]
        })
        .run()
        .assert_same();
}

#[test]
fn construct_case_insensitive_map() {
    Diff::new(entry!(0x0048_6ee0, map::construct_case_insensitive_map(u32, u32, u8) -> u32))
        .family(&FORM_MAP)
        .goes_by_address(&[MEMSET])
        .inputs(&[Input::Of(&[0, 1, 7, 0x80]), Input::Of(&[0, 1, 0xff])])
        .setup(|e, case| {
            doubles(e);
            let this = e.mem.alloc(0x14);
            e.mem.write(this, &[0xee; 0x14]);
            vec![this, case.word(0), case.word(1)]
        })
        .run()
        .assert_same();
}

#[test]
fn construct_editor_id_map() {
    Diff::new(entry!(0x0048_6ab0, map::construct_editor_id_map(u32, u32, u8) -> u32))
        .family(&FORM_MAP)
        .goes_by_address(&[MEMSET])
        .inputs(&[Input::Of(&[0, 1, 7, 0x80]), Input::Of(&[0, 1, 0xff])])
        .setup(|e, case| {
            doubles(e);
            let this = e.mem.alloc(0x14);
            e.mem.write(this, &[0xee; 0x14]);
            vec![this, case.word(0), case.word(1)]
        })
        .run()
        .assert_same();
}
