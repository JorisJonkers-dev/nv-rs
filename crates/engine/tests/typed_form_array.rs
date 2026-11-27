//! The forms family's registry array (`engine_typed::forms::array`, nv-rs#951): the differential test
//! of each function against its translation, then the array's life in every form of the family
//! (stage 1, the dual run, K1 `forms:moved`, K2 `forms:freed`).
//!
//! The differential tests build a large array of words in guest memory (an `NiTLargeArray`: +4 element
//! block, +8 capacity, +0xc used size, +0x10 count, +0x14 grow-by); the allocator (`00aa1070`) and the
//! release (`004ede70`) are doubles, the other callees run as the translations have them.

use engine as _;
use engine_core::prelude::*;
use engine_core::typed::diff::{Diff, Input};
use engine_core::typed::form_array::{self, BASE, CAPACITY, COUNT, GROW_BY, SIZE};
use engine_core::typed::moved::poison;
use engine_typed::forms::{array, FORMS};

const ALLOCATE_BYTES: u32 = 0x00aa_1070;
const FREE_BLOCK: u32 = 0x004e_de70;
const ALLOCATE: u32 = 0x00aa_3e40;
const DEALLOCATE: u32 = 0x00aa_4060;
const OPERATOR_NEW: u32 = 0x0040_1000;
const FORM_MAP: u32 = 0x011c_54c0;
const EDITOR_ID_MAP: u32 = 0x011c_54c8;
const FORM_LIST: u32 = 0x011c_54c4;

fn ret(eax: u32) -> Ret {
    Ret {
        eax,
        ..Ret::default()
    }
}

/// The allocator's and the release's doubles, as the translations call them.
fn doubles(e: &mut Engine) {
    e.register(ALLOCATE_BYTES, |e, a| ret(e.mem.alloc(a[0].max(4))));
    e.register(FREE_BLOCK, |_, _| Ret::default());
}

/// An array object with `capacity` words in its element block, the first `items.len()` used, and
/// the counts as they are.
fn array_with(e: &mut Engine, capacity: u32, items: &[u32], grow_by: u32) -> u32 {
    let this = e.mem.alloc(0x18);
    let block = e.mem.alloc(capacity.max(1) * 4);
    for (i, v) in items.iter().enumerate() {
        e.mem.set_u32(block + 4 * i as u32, *v);
    }
    e.mem.set_u32(this, 0x0101_9a3c);
    e.mem.set_u32(this + BASE, block);
    e.mem.set_u32(this + CAPACITY, capacity);
    e.mem.set_u32(this + SIZE, items.len() as u32);
    e.mem.set_u32(
        this + COUNT,
        items.iter().filter(|v| **v != 0).count() as u32,
    );
    e.mem.set_u32(this + GROW_BY, grow_by);
    this
}

/// The arrays the cases pick from: empty with room, full, with a hole, with room left, nothing.
fn shape(e: &mut Engine, kind: u32) -> u32 {
    match kind {
        0 => array_with(e, 3, &[], 1),
        1 => array_with(e, 2, &[5, 6], 1),
        2 => array_with(e, 3, &[5, 0, 6], 2),
        3 => array_with(e, 4, &[5, 6], 1),
        4 => array_with(e, 0, &[], 1),
        _ => array_with(e, 3, &[0, 0, 0], 3),
    }
}

const SHAPES: &[u32] = &[0, 1, 2, 3, 4, 5];

#[test]
fn construct_base() {
    Diff::new(entry!(0x0047_05e0, array::construct_base(u32, u32, u32) -> u32))
        .family(&FORMS)
        .inputs(&[Input::Of(&[0, 1, 2, 0x20]), Input::Of(&[0, 1, 5, u32::MAX])])
        .setup(|e, case| {
            doubles(e);
            let this = e.mem.alloc(0x18);
            vec![this, case.word(0), case.word(1)]
        })
        .run()
        .assert_same();
}

#[test]
fn construct() {
    Diff::new(entry!(0x0047_01d0, array::construct(u32, u32, u32) -> u32))
        .family(&FORMS)
        .inputs(&[Input::Of(&[0, 1, 2, 0x20]), Input::Of(&[0, 1, 5, u32::MAX])])
        .setup(|e, case| {
            doubles(e);
            let this = e.mem.alloc(0x18);
            vec![this, case.word(0), case.word(1)]
        })
        .run()
        .assert_same();
}

#[test]
fn destructor() {
    Diff::new(entry!(0x0047_0110, array::destructor(u32)))
        .family(&FORMS)
        .inputs(&[Input::Of(SHAPES)])
        .setup(|e, case| {
            doubles(e);
            vec![shape(e, case.word(0))]
        })
        .run()
        .assert_same();
}

#[test]
fn add() {
    Diff::new(entry!(0x0047_0140, array::add(u32, u32) -> u32))
        .family(&FORMS)
        .inputs(&[Input::Of(SHAPES), Input::Of(&[0, 0x1234, u32::MAX])])
        .setup(|e, case| {
            doubles(e);
            let this = shape(e, case.word(0));
            let item = e.mem.alloc(4);
            e.mem.set_u32(item, case.word(1));
            vec![this, item]
        })
        .run()
        .assert_same();
}

#[test]
fn set_at_grow() {
    Diff::new(entry!(0x0086_4240, array::set_at_grow(u32, u32, u32) -> u32))
        .family(&FORMS)
        .inputs(&[
            Input::Of(SHAPES),
            Input::Of(&[0, 1, 2, 3, 4, 7]),
            Input::Of(&[0, 0x77]),
        ])
        .setup(|e, case| {
            doubles(e);
            let this = shape(e, case.word(0));
            let item = e.mem.alloc(4);
            e.mem.set_u32(item, case.word(2));
            vec![this, case.word(1), item]
        })
        .run()
        .assert_same();
}

#[test]
fn set_size() {
    Diff::new(entry!(0x0086_4280, array::set_size(u32, u32)))
        .family(&FORMS)
        .inputs(&[Input::Of(SHAPES), Input::Of(&[0, 1, 2, 3, 4, 8])])
        .setup(|e, case| {
            doubles(e);
            vec![shape(e, case.word(0)), case.word(1)]
        })
        .run()
        .assert_same();
}

#[test]
fn set_at() {
    Diff::new(entry!(0x0061_bd20, array::set_at(u32, u32, u32)))
        .family(&FORMS)
        .inputs(&[
            Input::Of(SHAPES),
            Input::Of(&[0, 1, 2, 3]),
            Input::Of(&[0, 0x77]),
        ])
        .setup(|e, case| {
            doubles(e);
            let this = shape(e, case.word(0));
            let item = e.mem.alloc(4);
            e.mem.set_u32(item, case.word(2));
            vec![this, case.word(1), item]
        })
        .run()
        .assert_same();
}

#[test]
fn remove_at() {
    Diff::new(entry!(0x0048_6b80, array::remove_at(u32, u32) -> u32))
        .family(&FORMS)
        .inputs(&[Input::Of(SHAPES), Input::Of(&[0, 1, 2, 3, u32::MAX])])
        .setup(|e, case| {
            doubles(e);
            vec![shape(e, case.word(0)), case.word(1)]
        })
        .run()
        .assert_same();
}

#[test]
fn clear() {
    Diff::new(entry!(0x0086_3db0, array::clear(u32)))
        .family(&FORMS)
        .inputs(&[Input::Of(SHAPES)])
        .setup(|e, case| {
            doubles(e);
            vec![shape(e, case.word(0))]
        })
        .run()
        .assert_same();
}

mod forms {
    //! The registry array's life through the switch in every form: made by `create_registries`
    //! (its record attached before the constructor), filled with more forms than its capacity (so
    //! the block grows and is released), looked up, removed from, cleared and destroyed. The answers
    //! and the bytes untyped code reads (vtable, capacity, used size, count) are the same in each
    //! form; K1 makes the guest heap's allocations and takes the time slice's steps stage 1 does;
    //! the dual run compares and finds no difference; the moved forms keep poison in the block word
    //! while the array lives, and nothing of the family is left in the slab after.
    use super::*;
    use engine_core::typed::moved::Store;

    const FORM: u32 = 0x0400_0000;
    const LOG: u32 = 0x0310_0000;
    const LOG_BYTES: u32 = 0x1_0000;
    /// The array's scalar deleting destructor: slot 0 of the vtable the constructor stores.
    const VTABLE: u32 = 0x0101_9a3c;
    const VT_DESTROY: u32 = 0x00ff_2000;
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
        e.typed_apply_for_test(&[&FORMS, &leaf::LEAVES], spec);
        e.map(LOG, LOG_BYTES);
        e.map(FORM_MAP & !0xfff, 0x1000);
        e.map(VTABLE & !0xfff, 0x1000);
        e.mem.set_u32(VTABLE, VT_DESTROY);
        // The scalar deleting destructor: the destructor body, then the block's release.
        e.register(VT_DESTROY, |e, a| {
            e.call(0x0047_0110, &[a[0]]);
            if a[1] & 1 != 0 {
                e.call(0x0040_1030, &[a[0]]);
            }
            ret(a[0])
        });
        // The allocator's own call under each, and the log of every block made and released.
        e.register(ALLOCATE, |_, _| ret(0));
        e.register(DEALLOCATE, |_, _| ret(0));
        e.register(OPERATOR_NEW, |e, a| {
            e.call(ALLOCATE, &[]);
            let p = e.mem.alloc(a[0].max(1));
            log(e, 1, a[0], p);
            ret(p)
        });
        e.register(0x0040_1030, |e, a| {
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
        e.register(FREE_BLOCK, |e, a| {
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
        /// What untyped code reads of the array at each look: vtable, capacity, used size, count.
        bytes: Vec<[u32; 4]>,
        heap: Vec<[u32; 3]>,
        poison_held: bool,
        dual: Option<(u64, u64)>,
        slab_after: Vec<(u8, u16, u64, u64)>,
        steps: Vec<u32>,
    }

    fn form(i: u32) -> u32 {
        FORM + 0x10 * i
    }

    /// `with_item`'s stack word: the item passed by address to `Add`.
    fn add(e: &mut Engine, list: u32, v: u32) -> u32 {
        let slot = e.mem.alloc(4);
        e.mem.set_u32(slot, v);
        e.call(0x0047_0140, &[list, slot]).u32()
    }

    fn life(spec: &str) -> Life {
        let mut e = engine(spec);
        STEPS.with(|s| s.borrow_mut().clear());
        let moved_form = spec.ends_with(":moved") || spec.ends_with(":freed");
        // The other two registries exist already: only the array is made.
        e.mem.set_u32(FORM_MAP, 1);
        e.mem.set_u32(EDITOR_ID_MAP, 1);
        e.slice_hook = Some(|e, addr| {
            STEPS.with(|s| s.borrow_mut().push(addr));
            e.slice_left = 0;
        });
        e.slice_len = 0;
        e.slice_left = 0;
        let mut answers = Vec::new();
        let mut bytes = Vec::new();
        let mut poison_held = true;
        e.call(0x0048_3a50, &[]);
        let list = e.mem.u32(FORM_LIST);
        assert_ne!(list, 0, "the registry array is made");
        let mut look = |e: &mut Engine, answers: &mut Vec<u32>| {
            bytes.push([
                e.mem.u32(list),
                e.mem.u32(list + CAPACITY),
                e.mem.u32(list + SIZE),
                e.mem.u32(list + COUNT),
            ]);
            if moved_form {
                poison_held &= e.mem.u32(list + BASE) == poison(form_array::FAMILY, BASE);
            }
            let _ = answers;
        };
        look(&mut e, &mut answers);
        // Fill past the capacity (0x20): the block grows by one word a time.
        for i in 0..0x30 {
            answers.push(add(&mut e, list, form(i)));
        }
        look(&mut e, &mut answers);
        // Look the forms up the way the typed code does: by index through the array.
        for i in [0u32, 1, 0x1f, 0x20, 0x2f] {
            let element = e.call(0x0087_7a30, &[list, i]).u32();
            answers.push(form_array::slot(&e, list, i, element));
        }
        for i in [0u32, 5, 0x20, 0x2f, 0x40] {
            answers.push(e.call(0x0048_6b80, &[list, i]).u32());
        }
        look(&mut e, &mut answers);
        // A hole is filled again by an add; a null item is refused.
        answers.push(add(&mut e, list, form(0x50)));
        answers.push(add(&mut e, list, 0));
        look(&mut e, &mut answers);
        e.call(0x0086_3db0, &[list]);
        look(&mut e, &mut answers);
        answers.push(add(&mut e, list, form(0x51)));
        look(&mut e, &mut answers);
        e.mem.set_u32(FORM_MAP, 0);
        e.mem.set_u32(EDITOR_ID_MAP, 0);
        e.call(0x0048_3b80, &[]);
        let heap = logged(&e);
        let moved = e.moved();
        Life {
            steps: STEPS.with(|s| std::mem::take(&mut *s.borrow_mut())),
            answers,
            bytes,
            heap,
            poison_held,
            dual: moved.map(|m| m.dual.counts(form_array::FAMILY)),
            slab_after: moved.map_or(Vec::new(), |m| m.slab.census()),
        }
    }

    fn mine(c: &[(u8, u16, u64, u64)]) -> Vec<(u8, u16, u64, u64)> {
        c.iter()
            .copied()
            .filter(|row| row.0 == form_array::FAMILY.number())
            .collect()
    }

    #[test]
    fn the_array_lives_alike_in_every_form() {
        let stage1 = life("forms");
        assert!(stage1.poison_held);
        // The default: stage 1's store.
        let default = life("");
        assert_eq!(default.answers, stage1.answers);
        assert_eq!(default.heap, stage1.heap);
        assert_eq!(default.steps, stage1.steps);

        let dual = life("forms:dual");
        assert_eq!(dual.answers, stage1.answers);
        assert_eq!(dual.bytes, stage1.bytes);
        assert_eq!(dual.heap, stage1.heap);
        let (compared, differed) = dual.dual.expect("a dual run counts");
        assert!(compared > 100, "the dual run compared {compared} accesses");
        assert_eq!(differed, 0, "the dual run found differences");

        // K1: the same answers, bytes, guest heap (same sizes, same order) and steps; poison while
        // it lives; nothing left in the slab.
        let k1 = life("forms:moved");
        assert_eq!(k1.answers, stage1.answers);
        assert_eq!(k1.bytes, stage1.bytes);
        assert_eq!(
            k1.heap, stage1.heap,
            "K1 makes the guest heap's allocations"
        );
        assert_eq!(k1.steps, stage1.steps, "K1 takes stage 1's steps");
        assert!(k1.poison_held, "the element block's word held its poison");
        assert!(
            mine(&k1.slab_after).is_empty(),
            "left in the slab: {:?}",
            mine(&k1.slab_after)
        );

        // K2: the same answers and bytes; the guest bytes of the block are gone.
        let k2 = life("forms:freed");
        assert_eq!(k2.answers, stage1.answers);
        assert_eq!(k2.bytes, stage1.bytes);
        assert!(k2.heap.len() < stage1.heap.len());
        assert!(k2.poison_held);
        assert!(mine(&k2.slab_after).is_empty());
        assert!(!matches!(Store::Guest, Store::Moved { .. }));
    }
}
