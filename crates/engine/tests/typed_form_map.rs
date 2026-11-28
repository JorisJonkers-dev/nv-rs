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

mod forms {
    //! The form map's life through the switch in every form (stage 2): made by `create_registries`
    //! (its record attached before the constructor), filled by `SetAt` (`00844700`) with keys that
    //! share buckets, looked up (`00853130`, `004839c0`), read by bucket the way the untyped
    //! readers do (`form_map::bucket`), removed from (`00405430`), cleared (`00438af0`) and destroyed
    //! (`00483b80`, through the destructor chain to `00486a60`). The answers, the bytes untyped code
    //! reads (vtable, bucket count) and the items read by bucket are the same in each form; K1 makes
    //! the guest heap's allocations and takes the time slice's steps stage 1 does; the dual run
    //! compares and finds no difference; the moved forms keep poison in the table word while the map
    //! lives, and nothing of the family is left in the slab after.
    use super::*;
    use engine_core::typed::form_map::{self, BUCKETS, TABLE};
    use engine_core::typed::leaf;
    use engine_core::typed::moved::poison;
    use engine_typed::dispatch::DISPATCH;
    use engine_typed::forms::{array::FORM_ARRAY, FORMS};
    use engine_typed::save::SAVE;

    const FORM_MAP_GLOBAL: u32 = 0x011c_54c0;
    const EDITOR_ID_MAP: u32 = 0x011c_54c8;
    const FORM_LIST: u32 = 0x011c_54c4;
    const OPERATOR_NEW: u32 = 0x0040_1000;
    const OPERATOR_DELETE: u32 = 0x0040_1030;
    const FREE_TABLE: u32 = 0x00aa_10f0;
    const ALLOCATE: u32 = 0x00aa_3e40;
    const DEALLOCATE: u32 = 0x00aa_4060;
    const POINTER_MAP_VTABLE: u32 = 0x0101_ca78;
    const MAP_BASE_VTABLE: u32 = 0x0101_ca98;
    const FORM_BUCKETS: u32 = 0x0002_008d;
    const LOG: u32 = 0x0310_0000;
    const LOG_BYTES: u32 = 0x4_0000;
    const NEW_ITEM: u32 = 0xf010_0014;
    const SET_VALUE: u32 = 0xf010_000c;
    const DESTROY: u32 = 0xf010_0020;
    std::thread_local! {
        static STEPS: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    fn log(e: &mut Engine, kind: u32, size: u32, at: u32) {
        let n = e.mem.u32(LOG);
        let entry = LOG + 4 + n * 12;
        assert!(entry + 12 <= LOG + LOG_BYTES, "the allocator's log is full");
        for (i, v) in [kind, size, at].into_iter().enumerate() {
            e.mem.set_u32(entry + 4 * i as u32, v);
        }
        e.mem.set_u32(LOG, n + 1);
    }

    fn logged(e: &Engine) -> Vec<[u32; 3]> {
        (0..e.mem.u32(LOG))
            .map(|i| {
                let entry = LOG + 4 + i * 12;
                [entry, entry + 4, entry + 8].map(|a| e.mem.u32(a))
            })
            .collect()
    }

    fn engine(spec: &str) -> Engine {
        let mut e = Engine::new_untyped_for_test();
        e.typed_apply_for_test(
            &[&FORM_MAP, &FORM_ARRAY, &FORMS, &DISPATCH, &SAVE, &leaf::LEAVES],
            spec,
        );
        e.map(LOG, LOG_BYTES);
        // The vtables: the pointer map's (the form map's) and the base's, slots as the doubles.
        let mut slots = vec![0; 0x18 / 4 + 1];
        slots[0] = DESTROY;
        slots[1] = HASH;
        slots[2] = EQUAL;
        slots[3] = SET_VALUE;
        slots[4] = CLEAR;
        slots[5] = NEW_ITEM;
        slots[6] = DELETE;
        e.put_vtable(POINTER_MAP_VTABLE, &slots);
        e.put_vtable(MAP_BASE_VTABLE, &slots);
        e.register(DESTROY, |e, a| {
            e.call(0x0048_6960, &[a[0], a[1]]);
            ret(a[0])
        });
        e.register(HASH, |_, a| ret(a[1] % FORM_BUCKETS));
        e.register(EQUAL, |_, a| ret(u32::from(a[1] == a[2])));
        e.register(SET_VALUE, |e, a| {
            e.mem.set_u32(a[1] + 4, a[2]);
            e.mem.set_u32(a[1] + 8, a[3]);
            Ret::default()
        });
        e.register(CLEAR, |e, a| {
            e.mem.set_u32(a[1] + 8, 0);
            Ret::default()
        });
        e.register(NEW_ITEM, |e, _| {
            let item = e.call(OPERATOR_NEW, &[12]).u32();
            ret(item)
        });
        e.register(DELETE, |e, a| {
            e.call(OPERATOR_DELETE, &[a[1]]);
            Ret::default()
        });
        e.register(ALLOCATE, |_, _| ret(0));
        e.register(DEALLOCATE, |_, _| ret(0));
        e.register(OPERATOR_NEW, |e, a| {
            e.call(ALLOCATE, &[]);
            let p = e.mem.alloc(a[0].max(1));
            log(e, 1, a[0], p);
            ret(p)
        });
        e.register(OPERATOR_DELETE, |e, a| {
            e.call(DEALLOCATE, &[]);
            if a[0] != 0 {
                log(e, 2, 0, a[0]);
                e.mem.free(a[0]);
            }
            ret(0)
        });
        e.register(ALLOCATE_BYTES, |e, a| {
            e.call(ALLOCATE, &[]);
            let p = e.mem.alloc(a[0].max(1));
            log(e, 1, a[0], p);
            ret(p)
        });
        e.register(FREE_TABLE, |e, a| {
            e.call(DEALLOCATE, &[]);
            if a[0] != 0 {
                log(e, 2, 0, a[0]);
                e.mem.free(a[0]);
            }
            ret(0)
        });
        e
    }

    struct Life {
        answers: Vec<u32>,
        /// What untyped code reads of the map at each look: vtable and bucket count.
        bytes: Vec<[u32; 2]>,
        /// The items read by bucket (`form_map::bucket`) at each look: bucket, key, value.
        items: Vec<Vec<[u32; 3]>>,
        heap: Vec<[u32; 3]>,
        poison_held: bool,
        dual: Option<(u64, u64)>,
        slab_after: Vec<(u8, u16, u64, u64)>,
        steps: Vec<u32>,
    }

    /// The keys: some alone in a bucket, some sharing one (a multiple of the bucket count apart).
    fn keys() -> Vec<u32> {
        let mut keys = Vec::new();
        for i in 0..120u32 {
            keys.push(0x0100_0000 + i * 0x10);
        }
        for i in 0..12u32 {
            let k = 0x0100_0000 + i * 0x10;
            keys.push(k + FORM_BUCKETS);
            keys.push(k + 2 * FORM_BUCKETS);
        }
        keys
    }

    fn read_items(e: &Engine, map: u32) -> Vec<[u32; 3]> {
        let mut out = Vec::new();
        for b in 0..FORM_BUCKETS {
            let mut item = form_map::bucket(e, map, b);
            while item != 0 {
                out.push([b, e.mem.u32(item + 4), e.mem.u32(item + 8)]);
                item = e.mem.u32(item);
            }
        }
        out
    }

    fn life(spec: &str) -> Life {
        let mut e = engine(spec);
        STEPS.with(|s| s.borrow_mut().clear());
        let moved_form = spec.ends_with(":moved") || spec.ends_with(":freed");
        // The other two registries exist already: only the map is made.
        e.mem.set_u32(EDITOR_ID_MAP, 1);
        e.mem.set_u32(FORM_LIST, 1);
        e.slice_hook = Some(|e, addr| {
            STEPS.with(|s| s.borrow_mut().push(addr));
            e.slice_left = 0;
        });
        e.slice_len = 0;
        e.slice_left = 0;
        let mut answers = Vec::new();
        let mut bytes = Vec::new();
        let mut items = Vec::new();
        let mut poison_held = true;
        e.call(0x0048_3a50, &[]);
        let map = e.mem.u32(FORM_MAP_GLOBAL);
        assert_ne!(map, 0, "the form map is made");
        let mut look = |e: &mut Engine, answers: &mut Vec<u32>| {
            bytes.push([e.mem.u32(map), e.mem.u32(map + BUCKETS)]);
            let s = form_map::store(e, map);
            answers.push(form_map::count(e, s, map));
            items.push(read_items(e, map));
            if moved_form {
                poison_held &= e.mem.u32(map + TABLE) == poison(form_map::FAMILY, TABLE);
            }
        };
        look(&mut e, &mut answers);
        let keys = keys();
        for (i, key) in keys.iter().enumerate() {
            e.call(0x0084_4700, &[map, *key, 0x5000 + i as u32]);
        }
        // A key set again keeps one item and takes the new value.
        e.call(0x0084_4700, &[map, keys[3], 0x7777]);
        look(&mut e, &mut answers);
        let out = e.mem.alloc(4);
        for key in keys.iter().step_by(7).chain(&[0x0200_0000, 5]) {
            e.mem.set_u32(out, 0xdead);
            let found = e.call(0x0085_3130, &[map, *key, out]).u32() & 0xff;
            answers.push(found);
            answers.push(e.mem.u32(out));
            answers.push(e.call(0x0048_39c0, &[*key]).u32());
        }
        // A bucket's head, the middle of a chain, its tail, a key that is not there.
        for key in [
            keys[0x10],
            keys[120 + 2],
            keys[120 + 5],
            0x0300_0000,
            keys[0x10],
        ] {
            answers.push(e.call(0x0040_5430, &[map, key]).u32() & 0xff);
        }
        look(&mut e, &mut answers);
        e.call(0x0043_8af0, &[map]);
        look(&mut e, &mut answers);
        for key in keys.iter().take(5) {
            e.call(0x0084_4700, &[map, *key, 1]);
        }
        look(&mut e, &mut answers);
        e.mem.set_u32(EDITOR_ID_MAP, 0);
        e.mem.set_u32(FORM_LIST, 0);
        e.call(0x0048_3b80, &[]);
        let heap = logged(&e);
        let moved = e.moved();
        Life {
            steps: STEPS.with(|s| std::mem::take(&mut *s.borrow_mut())),
            answers,
            bytes,
            items,
            heap,
            poison_held,
            dual: moved.map(|m| m.dual.counts(form_map::FAMILY)),
            slab_after: moved.map_or(Vec::new(), |m| m.slab.census()),
        }
    }

    fn mine(c: &[(u8, u16, u64, u64)]) -> Vec<(u8, u16, u64, u64)> {
        c.iter()
            .copied()
            .filter(|row| row.0 == form_map::FAMILY.number())
            .collect()
    }

    #[test]
    fn the_form_map_lives_alike_in_every_form() {
        let stage1 = life("formmap");
        assert!(stage1.poison_held);
        assert!(stage1.items[1].len() > 100, "the map holds items");
        // The default: stage 1's store.
        let default = life("");
        assert_eq!(default.answers, stage1.answers);
        assert_eq!(default.items, stage1.items);
        assert_eq!(default.heap, stage1.heap);
        assert_eq!(default.steps, stage1.steps);

        let dual = life("formmap:dual");
        assert_eq!(dual.answers, stage1.answers);
        assert_eq!(dual.bytes, stage1.bytes);
        assert_eq!(dual.items, stage1.items);
        assert_eq!(dual.heap, stage1.heap);
        let (compared, differed) = dual.dual.expect("a dual run counts");
        assert!(compared > 100, "the dual run compared {compared} accesses");
        assert_eq!(differed, 0, "the dual run found differences");

        // K1: the same answers, bytes, items, guest heap (same sizes, same order) and steps; poison
        // while it lives; nothing left in the slab.
        let k1 = life("formmap:moved");
        assert_eq!(k1.answers, stage1.answers);
        assert_eq!(k1.bytes, stage1.bytes);
        assert_eq!(k1.items, stage1.items);
        assert_eq!(
            k1.heap, stage1.heap,
            "K1 makes the guest heap's allocations"
        );
        assert_eq!(k1.steps, stage1.steps, "K1 takes stage 1's steps");
        assert!(k1.poison_held, "the table word held its poison");
        assert!(
            mine(&k1.slab_after).is_empty(),
            "left in the slab: {:?}",
            mine(&k1.slab_after)
        );

        // K2: the same answers, bytes and items; the guest bytes of the table are gone.
        let k2 = life("formmap:freed");
        assert_eq!(k2.answers, stage1.answers);
        assert_eq!(k2.bytes, stage1.bytes);
        assert_eq!(k2.items, stage1.items);
        assert!(k2.heap.len() < stage1.heap.len());
        assert!(k2.poison_held);
        assert!(mine(&k2.slab_after).is_empty());
    }
}
