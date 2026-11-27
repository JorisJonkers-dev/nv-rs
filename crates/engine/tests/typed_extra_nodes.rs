// SPDX-License-Identifier: GPL-3.0-only
//! The differential test of every typed function of the extra data node classes
//! (`engine_typed::extra_data::nodes`) against its translation: here because the translations
//! live in the part crates, and this crate registers them all.
//!
//! The world: a page of control words, an arena of words that only ever point forward into itself
//! (so a chain ends), a vtable whose slots are stand-ins that count their calls and answer their
//! first word, and the functions of other units the node functions call by address, also stand-ins
//! (`operator new` hands out a block, `operator delete` counts, the dynamic cast answers its object
//! or none as the case says). Each pointer argument is a block of words drawn from the case's seed:
//! zero, small numbers, or a pointer into the arena, with the world's vtable in its first word.
//! The same world is made on the translation's engine and the typed engine; the comparison covers
//! the result, the memory, the calls made and the words they were handed.

use engine as _;
use engine_core::prelude::*;
use engine_core::typed::diff::{Diff, Input};
use engine_typed::extra_data::{self, EXTRA_DATA};

/// The world's pages.
const CONTROL: u32 = 0x0300_0000;
const ARENA: u32 = 0x0400_0000;
const ARENA_LEN: u32 = 0x4000;
const WORLD_VT: u32 = 0x0500_0000;
/// Where the world's vtable slots are registered: past `.text`, one a slot.
const VT_FN: u32 = 0x00ff_6000;
const VT_SLOTS: u32 = 0x40;
/// The stand-ins' call count, and whether the dynamic cast finds its target.
const NOTES: u32 = CONTROL;
const CAST_OK: u32 = CONTROL + 4;
/// `operator new`, `operator delete` and `__RTDynamicCast`.
const OPERATOR_NEW: u32 = 0x0040_1000;
const OPERATOR_DELETE: u32 = 0x0040_1030;
const DYNAMIC_CAST: u32 = 0x00ec_43fb;

/// The functions of other units the node functions call by address.
const CALLEES: [u32; 112] = [
    0x00401290, 0x00401460, 0x004019a0, 0x004037b0, 0x004037d0, 0x004037f0, 0x004039a0, 0x004039c0,
    0x00403a00, 0x00403d30, 0x00403e20, 0x004048e0, 0x00405430, 0x00406d00, 0x00406d30, 0x00407800,
    0x00408b20, 0x0040ec80, 0x0040ecb0, 0x0040f700, 0x00418d20, 0x00418f10, 0x0041af70, 0x0041fd00,
    0x00425fd0, 0x00425ff0, 0x0042ff80, 0x00430000, 0x00438af0, 0x00438e40, 0x00439060, 0x0043a860,
    0x0043d4d0, 0x0044a670, 0x004568c0, 0x0045cec0, 0x0046ffb0, 0x004702f0, 0x00470470, 0x0047c850,
    0x00483710, 0x004839c0, 0x00484e60, 0x00485d50, 0x004b7240, 0x004b9ba0, 0x004bf150, 0x004d9380,
    0x004d9780, 0x004f8960, 0x004fbf00, 0x005594e0, 0x0055a400, 0x005653d0, 0x00565480, 0x00567e10,
    0x00575690, 0x00575700, 0x00575830, 0x0057c850, 0x005897c0, 0x005ae380, 0x005ae3d0, 0x005b5e40,
    0x005d43c0, 0x005d8a70, 0x005e58f0, 0x005f65d0, 0x00605fc0, 0x00633c90, 0x0063f7b0, 0x0066b0d0,
    0x00674d70, 0x00692710, 0x006a7ad0, 0x006b3eb0, 0x006b7f20, 0x006d5320, 0x00719b20, 0x00726c60,
    0x00761540, 0x007b3fa0, 0x007cb2e0, 0x0083da50, 0x00844700, 0x008454f0, 0x00853130, 0x0085a2e0,
    0x0087f9f0, 0x00898280, 0x008c71b0, 0x008d6f30, 0x008d7510, 0x00905330, 0x00905820, 0x00906dc0,
    0x00913fe0, 0x0092c950, 0x0092eb50, 0x00931850, 0x00935ae0, 0x0096a2d0, 0x0096d450, 0x009a3830,
    0x009a4320, 0x00a59d30, 0x00a5b690, 0x00aa1070, 0x00aa10f0, 0x00ec4835, 0x00ec61c0, 0x00ec62c0,
];

/// The list and array primitives the cases that `node_real` leave to their translations.
const REAL_CONTAINERS: [u32; 11] = [
    0x006a_7ad0,
    0x007c_b2e0,
    0x0071_9b20,
    0x0084_54f0,
    0x0063_f7b0,
    0x005a_e380,
    0x0090_5330,
    0x005f_65d0,
    0x0047_0470,
    0x0047_02f0,
    0x0096_a2d0,
];

/// Typed calls into other families, which go by address here.
const OTHER_FAMILIES: &[u32] = &[
    0x0041_6870,
    0x0043_6aa0,
    0x0044_ddc0,
    0x0045_cd60,
    0x0055_9450,
    0x0068_15c0,
    0x0072_6070,
    0x0082_56d0,
    0x0084_e3a0,
    0x008d_8520,
];

fn ret(eax: u32) -> Ret {
    Ret {
        eax,
        ..Ret::default()
    }
}

fn mix(a: u32, b: u32) -> u32 {
    let mut x = a.wrapping_mul(0x9e37_79b9) ^ b.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^ (x >> 12)
}

/// A word of the block `seed` makes: mostly zero or small, now and then a pointer into the arena.
fn seeded_word(seed: u32, i: u32) -> u32 {
    let m = mix(seed, i);
    match m % 9 {
        0..=2 => 0,
        3 => 1,
        4 => 2,
        5 => 3,
        6 => 0x14,
        _ => ARENA + ((m >> 8) % 0x3c0) * 0x10,
    }
}

/// A stand-in that counts and answers its first word.
fn noted(e: &mut Engine, a: &[u32]) -> Ret {
    let n = e.mem.u32(NOTES);
    e.mem.set_u32(NOTES, n.wrapping_add(1));
    ret(a.first().copied().unwrap_or(0))
}

/// `operator new`: a block of junk.
fn new_block(e: &mut Engine, a: &[u32]) -> Ret {
    let size = a[0].max(0x40);
    let block = e.mem.alloc(size);
    for i in (0..size).step_by(4) {
        e.mem.set_u32(block + i, 0xa5a5_a5a5);
    }
    ret(block)
}

/// `__RTDynamicCast(object, ..)`: the object, or none.
fn cast(e: &mut Engine, a: &[u32]) -> Ret {
    ret(if e.mem.u32(CAST_OK) != 0 { a[0] } else { 0 })
}

/// The world, made on each engine from the case's world word.
fn world(e: &mut Engine, salt: u32, real: bool) {
    e.map(CONTROL, 0x1000);
    e.map(ARENA, ARENA_LEN);
    e.map(WORLD_VT, 0x100);
    e.map(0x0101_2000, 0x3000);
    for i in 0..ARENA_LEN / 4 {
        let m = mix(0xa4e7a, i);
        let word = if m % 4 == 0 {
            ARENA + (i + 1 + (m >> 8) % 12) * 4
        } else {
            [0, 1, 2, 3][(m >> 4) as usize % 4]
        };
        e.mem.set_u32(ARENA + i * 4, word);
    }
    for slot in 0..VT_SLOTS {
        e.mem.set_u32(WORLD_VT + slot * 4, VT_FN + slot * 4);
        e.register(VT_FN + slot * 4, noted);
    }
    e.register(OPERATOR_NEW, new_block);
    e.register(OPERATOR_DELETE, noted);
    e.register(DYNAMIC_CAST, cast);
    for addr in CALLEES {
        // The containers' own translations run when the case asks (`node_real`): a loop that empties
        // a list through them ends, where a counting stand-in never empties it.
        if !(real && REAL_CONTAINERS.contains(&addr)) {
            e.register(addr, noted);
        }
    }
    e.mem.set_u32(CAST_OK, salt & 1);
}

/// A block of `size` bytes whose words the seed draws, with the world's vtable first.
fn object(e: &mut Engine, size: u32, seed: u32) -> u32 {
    let block = e.mem.alloc(size);
    for i in (0..size).step_by(4) {
        e.mem.set_u32(block + i, seeded_word(seed, i / 4));
    }
    e.mem.set_u32(block, WORLD_VT);
    block
}

/// What an argument is.
#[derive(Clone, Copy)]
enum A {
    /// `this`: a block of this many bytes, never null.
    This(u32),
    /// A pointer argument: a block of this many bytes, or null.
    Obj(u32),
    /// A block of this many bytes, never null (or null when the flag says so, as `Obj`), whose fields
    /// the fills make real: a count that is small, a pointer that points at a block.
    Shaped(u32, &'static [Fill], bool),
    /// A word from this list.
    Word(&'static [u32]),
    Flag,
    Float,
}

/// One field of a shaped block.
#[derive(Clone, Copy)]
#[allow(
    dead_code,
    reason = "the fills the worlds of the functions in nodes::UNVERIFIED will use"
)]
enum Fill {
    /// A word at this offset, taken from the list by the seed.
    Small(u32, &'static [u32]),
    /// A pointer at this offset to a block of this many bytes the seed draws.
    Ptr(u32, u32),
    /// A `BSSimpleArray` at this offset (`pBuffer` +4, `iSize` +8, `iReservedSize` +0xc) of pointers
    /// to blocks of this many bytes: 0 to 3 of them by the seed, and room for 4.
    Array(u32, u32),
    /// A `BSSimpleList` head node at this offset (item, next) whose item and chain are blocks of this
    /// many bytes: 0 to 3 further nodes by the seed.
    List(u32, u32),
}

fn fill(e: &mut Engine, block: u32, fills: &[Fill], seed: u32) {
    for (k, f) in fills.iter().enumerate() {
        let m = mix(seed, 0x1000 + k as u32);
        match *f {
            Fill::Small(off, values) => e
                .mem
                .set_u32(block + off, values[m as usize % values.len()]),
            Fill::Ptr(off, size) => {
                let target = object(e, size, m);
                e.mem.set_u32(block + off, target);
            }
            Fill::Array(off, elem) => {
                let count = m % 4;
                let buffer = e.mem.alloc(16);
                for i in 0..4 {
                    let item = object(e, elem, mix(m, i));
                    e.mem.set_u32(buffer + i * 4, item);
                }
                e.mem.set_u32(block + off + 4, buffer);
                e.mem.set_u32(block + off + 8, count);
                e.mem.set_u32(block + off + 12, 4);
            }
            Fill::List(off, size) => {
                let mut tail = block + off;
                let item = object(e, size, m);
                e.mem.set_u32(tail, item);
                e.mem.set_u32(tail + 4, 0);
                for i in 0..m % 4 {
                    let node = e.mem.alloc(8);
                    let item = object(e, size, mix(m, 0x10 + i));
                    e.mem.set_u32(node, item);
                    e.mem.set_u32(node + 4, 0);
                    e.mem.set_u32(tail + 4, node);
                    tail = node;
                }
            }
        }
    }
}

const SEEDS: &[u32] = &[1, 2, 3, 4, 5, 6];
const OPTIONAL_SEEDS: &[u32] = &[0, 1, 2, 3, 4, 5];

trait NodeCase {
    fn node(self, args: &[A]) -> Diff;
    fn node_real(self, args: &[A]) -> Diff;
}

impl NodeCase for Diff {
    fn node(self, args: &[A]) -> Diff {
        node_world(self, args, false)
    }

    fn node_real(self, args: &[A]) -> Diff {
        node_world(self, args, true)
    }
}

fn node_world(diff: Diff, args: &[A], real: bool) -> Diff {
    {
        let inputs: Vec<Input> = args
            .iter()
            .map(|a| match a {
                A::This(_) => Input::Of(SEEDS),
                A::Obj(_) => Input::Of(OPTIONAL_SEEDS),
                A::Shaped(_, _, nullable) => {
                    Input::Of(if *nullable { OPTIONAL_SEEDS } else { SEEDS })
                }
                A::Word(pool) => Input::Of(pool),
                A::Flag => Input::Flag,
                A::Float => Input::F32,
            })
            .collect();
        let args = args.to_vec();
        diff.family(&EXTRA_DATA)
            .goes_by_address(OTHER_FAMILIES)
            .inputs(&inputs)
            .setup(move |e, case| {
                let words = case.words();
                world(e, words.iter().fold(0, |a, w| mix(a, *w)), real);
                let salt = words.iter().fold(7, |a, w| mix(a, *w));
                let mut call = Vec::new();
                for (i, a) in args.iter().enumerate() {
                    call.push(match a {
                        A::This(size) | A::Obj(size) if words[i] == 0 => {
                            let _ = size;
                            0
                        }
                        A::This(size) | A::Obj(size) => object(e, *size, mix(salt, words[i])),
                        A::Shaped(_, _, true) if words[i] == 0 => 0,
                        A::Shaped(size, fills, _) => {
                            let seed = mix(salt, words[i]);
                            let block = object(e, *size, seed);
                            fill(e, block, fills, seed);
                            block
                        }
                        A::Word(_) | A::Flag | A::Float => words[i],
                    });
                }
                call
            })
    }
}

#[test]
fn n_004300f0() {
    Diff::new(entry!(
        0x0043_00f0,
        extra_data::nodes::objects::extra_anim_scalar_deleting_destructor(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x00430120])
    .run()
    .assert_same();
}

#[test]
fn n_00430120() {
    Diff::new(entry!(
        0x0043_0120,
        extra_data::nodes::objects::fn_00430120(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004301b0() {
    Diff::new(entry!(
        0x0043_01b0,
        extra_data::nodes::objects::fn_004301b0(Ptr<_>, i32) -> u16
    ))
    .node(&[
        A::This(0x50),
        A::Word(&[0, 1, 2, 5, 0x14, 0x15, 0xffff_ffff]),
    ])
    .run()
    .assert_same();
}

#[test]
fn n_00430200() {
    Diff::new(entry!(
        0x0043_0200,
        extra_data::nodes::objects::extra_dismembered_limbs_extra_dismembered_limbs(
            Ptr<_>,
        )
            -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00430390() {
    Diff::new(entry!(
        0x0043_0390,
        extra_data::nodes::objects::fn_00430390(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x004303c0])
    .run()
    .assert_same();
}

#[test]
fn n_004303c0() {
    Diff::new(entry!(
        0x0043_03c0,
        extra_data::nodes::objects::fn_004303c0(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004303e0() {
    Diff::new(entry!(
        0x0043_03e0,
        extra_data::nodes::objects::extra_dismembered_limbs_dismembered(Ptr<_>, u8) -> bool
    ))
    .node(&[A::This(0x40), A::Word(&[0, 1, 2, 5, 0x14])])
    .run()
    .assert_same();
}

#[test]
fn n_00430410() {
    Diff::new(entry!(
        0x0043_0410,
        extra_data::nodes::objects::extra_dismembered_limbs_dismember(Ptr<_>, Ptr<_>, u8, u8)
    ))
    .node(&[
        A::This(0x40),
        A::Obj(0x50),
        A::Word(&[0, 1, 2, 5, 0x14]),
        A::Word(&[0, 1, 2, 5, 0x14]),
    ])
    .translation_calls_plainly(&[0x004303e0])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00430660() {
    Diff::new(entry!(0x0043_0660, extra_data::nodes::objects::extra_dismembered_limbs_check_dismembered_limbs_identical(Ptr<_>, Ptr<_>) -> bool))
        .node(&[A::This(0x40), A::Obj(0x40)])
        .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
        .assert_same();
}

#[test]
fn n_00430830() {
    Diff::new(entry!(
        0x0043_0830,
        extra_data::nodes::objects::fn_00430830(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00430850() {
    Diff::new(entry!(
        0x0043_0850,
        extra_data::nodes::objects::extra_starting_position_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_004308c0() {
    Diff::new(entry!(
        0x0043_08c0,
        extra_data::nodes::objects::fn_004308c0(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004308f0() {
    Diff::new(entry!(
        0x0043_08f0,
        extra_data::nodes::objects::fn_004308f0(Ptr<_>, Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00430920() {
    Diff::new(entry!(
        0x0043_0920,
        extra_data::nodes::objects::extra_light_scalar_deleting_destructor(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x00430950])
    .run()
    .assert_same();
}

#[test]
fn n_00430950() {
    Diff::new(entry!(
        0x0043_0950,
        extra_data::nodes::objects::fn_00430950(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00430a10() {
    Diff::new(entry!(
        0x0043_0a10,
        extra_data::nodes::objects::refr_lock_get_level(Ptr<_>, Ptr<_>) -> i32
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00430a90() {
    Diff::new(entry!(
        0x0043_0a90,
        extra_data::nodes::objects::refr_lock_set_locked(Ptr<_>, bool)
    ))
    .node(&[A::This(0x40), A::Flag])
    .run()
    .assert_same();
}

#[test]
fn n_00430bc0() {
    Diff::new(entry!(0x0043_0bc0, extra_data::nodes::objects::fn_00430bc0(u32) -> i32))
        .node(&[A::Word(&[0, 1, 2, 0x10, 0xffff_ffff])])
        .tolerate_garbage_stops(3)
        .max_calls(2000)
        .run()
        .assert_same();
}

#[test]
fn n_00430c70() {
    Diff::new(entry!(
        0x0043_0c70,
        extra_data::nodes::objects::extra_lock_extra_lock(Ptr<_>, Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Obj(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00430ca0() {
    Diff::new(entry!(
        0x0043_0ca0,
        extra_data::nodes::objects::fn_00430ca0(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00430dd0() {
    Diff::new(entry!(
        0x0043_0dd0,
        extra_data::nodes::objects::extra_follower_extra_follower(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00430e70() {
    Diff::new(entry!(
        0x0043_0e70,
        extra_data::nodes::objects::extra_follower_scalar_deleting_destructor(
            Ptr<_>,
            u32,
        ) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x00430ea0])
    .run()
    .assert_same();
}

#[test]
fn n_00430ea0() {
    Diff::new(entry!(
        0x0043_0ea0,
        extra_data::nodes::objects::fn_00430ea0(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00430f30() {
    Diff::new(entry!(
        0x0043_0f30,
        extra_data::nodes::objects::extra_guarded_ref_data_extra_guarded_ref_data(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00430fa0() {
    Diff::new(entry!(
        0x0043_0fa0,
        extra_data::nodes::objects::extra_guarded_ref_data_scalar_deleting_destructor(
            Ptr<_>,
            u32,
        )
            -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x00430fd0])
    .run()
    .assert_same();
}

#[test]
fn n_00430fd0() {
    Diff::new(entry!(
        0x0043_0fd0,
        extra_data::nodes::objects::fn_00430fd0(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00431030() {
    Diff::new(entry!(
        0x0043_1030,
        extra_data::nodes::objects::extra_guarded_ref_data_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004310d0() {
    Diff::new(entry!(
        0x0043_10d0,
        extra_data::nodes::objects::extra_guarded_ref_data_add_guard(Ptr<_>, Ptr<_>)
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_004311f0() {
    Diff::new(entry!(
        0x0043_11f0,
        extra_data::nodes::objects::fn_004311f0(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00431230() {
    Diff::new(entry!(
        0x0043_1230,
        extra_data::nodes::objects::fn_00431230(Ptr<_>, Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00431260() {
    Diff::new(entry!(
        0x0043_1260,
        extra_data::nodes::objects::fn_00431260(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_004312f0() {
    Diff::new(entry!(
        0x0043_12f0,
        extra_data::nodes::objects::extra_teleport_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00431360() {
    Diff::new(entry!(
        0x0043_1360,
        extra_data::nodes::objects::fn_00431360(Ptr<_>, Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00431390() {
    Diff::new(entry!(
        0x0043_1390,
        extra_data::nodes::objects::extra_map_marker_scalar_deleting_destructor(
            Ptr<_>,
            u32,
        ) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x004313c0])
    .run()
    .assert_same();
}

#[test]
fn n_004313c0() {
    Diff::new(entry!(
        0x0043_13c0,
        extra_data::nodes::objects::fn_004313c0(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00431450() {
    Diff::new(entry!(
        0x0043_1450,
        extra_data::nodes::objects::extra_map_marker_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_004314c0() {
    Diff::new(entry!(
        0x0043_14c0,
        extra_data::nodes::objects::fn_004314c0(Ptr<_>, Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_004314f0() {
    Diff::new(entry!(
        0x0043_14f0,
        extra_data::nodes::objects::extra_audio_marker_scalar_deleting_destructor(
            Ptr<_>,
            u32,
        )
            -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x00431520])
    .run()
    .assert_same();
}

#[test]
fn n_00431520() {
    Diff::new(entry!(
        0x0043_1520,
        extra_data::nodes::objects::fn_00431520(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004315b0() {
    Diff::new(entry!(
        0x0043_15b0,
        extra_data::nodes::objects::extra_audio_marker_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00431620() {
    Diff::new(entry!(
        0x0043_1620,
        extra_data::nodes::objects::fn_00431620(Ptr<_>, Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00431650() {
    Diff::new(entry!(
        0x0043_1650,
        extra_data::nodes::objects::extra_audio_buoy_marker_scalar_deleting_destructor(
            Ptr<_>,
            u32,
        )
            -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x00431680])
    .run()
    .assert_same();
}

#[test]
fn n_00431680() {
    Diff::new(entry!(
        0x0043_1680,
        extra_data::nodes::objects::fn_00431680(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00431710() {
    Diff::new(entry!(
        0x0043_1710,
        extra_data::nodes::objects::extra_audio_buoy_marker_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00431780() {
    Diff::new(entry!(
        0x0043_1780,
        extra_data::nodes::objects::fn_00431780(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004317c0() {
    Diff::new(entry!(
        0x0043_17c0,
        extra_data::nodes::objects::extra_action_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00431830() {
    Diff::new(entry!(
        0x0043_1830,
        extra_data::nodes::objects::fn_00431830(Ptr<_>, Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00431860() {
    Diff::new(entry!(
        0x0043_1860,
        extra_data::nodes::objects::extra_container_changes_scalar_deleting_destructor(
            Ptr<_>,
            u32,
        )
            -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x00431890, 0x00431920])
    .run()
    .assert_same();
}

#[test]
fn n_00431890() {
    Diff::new(entry!(
        0x0043_1890,
        extra_data::nodes::objects::fn_00431890(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .translation_calls_plainly(&[0x00431920])
    .run()
    .assert_same();
}

#[test]
fn n_00431920() {
    Diff::new(entry!(
        0x0043_1920,
        extra_data::nodes::objects::fn_00431920(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x50), A::Flag])
    .run()
    .assert_same();
}

#[test]
fn n_00431950() {
    Diff::new(entry!(
        0x0043_1950,
        extra_data::nodes::objects::fn_00431950(Ptr<_>, Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00431980() {
    Diff::new(entry!(
        0x0043_1980,
        extra_data::nodes::objects::fn_00431980(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x004319b0])
    .run()
    .assert_same();
}

#[test]
fn n_004319b0() {
    Diff::new(entry!(
        0x0043_19b0,
        extra_data::nodes::objects::fn_004319b0(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004319d0() {
    Diff::new(entry!(
        0x0043_19d0,
        extra_data::nodes::objects::extra_original_reference_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00431a40() {
    Diff::new(entry!(
        0x0043_1a40,
        extra_data::nodes::objects::fn_00431a40(Ptr<_>, Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00431a70() {
    Diff::new(entry!(
        0x0043_1a70,
        extra_data::nodes::objects::extra_ownership_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00431ae0() {
    Diff::new(entry!(
        0x0043_1ae0,
        extra_data::nodes::objects::fn_00431ae0(Ptr<_>, Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00431b10() {
    Diff::new(entry!(
        0x0043_1b10,
        extra_data::nodes::objects::extra_global_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00431b80() {
    Diff::new(entry!(
        0x0043_1b80,
        extra_data::nodes::objects::fn_00431b80(Ptr<_>, i32) -> Ptr<_>
    ))
    .node(&[
        A::This(0x40),
        A::Word(&[0, 1, 2, 5, 0x14, 0x15, 0xffff_ffff]),
    ])
    .run()
    .assert_same();
}

#[test]
fn n_00431bb0() {
    Diff::new(entry!(
        0x0043_1bb0,
        extra_data::nodes::objects::extra_rank_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00431c20() {
    Diff::new(entry!(
        0x0043_1c20,
        extra_data::nodes::objects::fn_00431c20(Ptr<_>, i16) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Word(&[0, 1, 2, 0x10, 0xffff_ffff])])
    .run()
    .assert_same();
}

#[test]
fn n_00431c50() {
    Diff::new(entry!(
        0x0043_1c50,
        extra_data::nodes::objects::extra_count_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00431cc0() {
    Diff::new(entry!(
        0x0043_1cc0,
        extra_data::nodes::objects::extra_leveled_item_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00431d10() {
    Diff::new(entry!(
        0x0043_1d10,
        extra_data::nodes::objects::fn_00431d10(Ptr<_>, f32) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Float])
    .run()
    .assert_same();
}

#[test]
fn n_00431d40() {
    Diff::new(entry!(
        0x0043_1d40,
        extra_data::nodes::objects::extra_health_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00431db0() {
    Diff::new(entry!(
        0x0043_1db0,
        extra_data::nodes::objects::extra_health_perc_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00431e20() {
    Diff::new(entry!(
        0x0043_1e20,
        extra_data::nodes::objects::fn_00431e20(Ptr<_>, u8) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Word(&[0, 1, 2, 5, 0x14])])
    .run()
    .assert_same();
}

#[test]
fn n_00431e50() {
    Diff::new(entry!(
        0x0043_1e50,
        extra_data::nodes::objects::extra_uses_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00431ec0() {
    Diff::new(entry!(
        0x0043_1ec0,
        extra_data::nodes::objects::fn_00431ec0(Ptr<_>, f32) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Float])
    .run()
    .assert_same();
}

#[test]
fn n_00431ef0() {
    Diff::new(entry!(
        0x0043_1ef0,
        extra_data::nodes::objects::extra_time_left_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00431f60() {
    Diff::new(entry!(
        0x0043_1f60,
        extra_data::nodes::objects::fn_00431f60(Ptr<_>, f32) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Float])
    .run()
    .assert_same();
}

#[test]
fn n_00431f90() {
    Diff::new(entry!(
        0x0043_1f90,
        extra_data::nodes::objects::extra_charge_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00432000() {
    Diff::new(entry!(
        0x0043_2000,
        extra_data::nodes::objects::extra_script_extra_script(Ptr<_>, Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00432040() {
    Diff::new(entry!(
        0x0043_2040,
        extra_data::nodes::objects::fn_00432040(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004320d0() {
    Diff::new(entry!(
        0x0043_20d0,
        extra_data::nodes::objects::extra_script_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00432140() {
    Diff::new(entry!(
        0x0043_2140,
        extra_data::nodes::objects::extra_weapon_mod_flags_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_004321b0() {
    Diff::new(entry!(
        0x0043_21b0,
        extra_data::nodes::objects::extra_modding_item_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00432220() {
    Diff::new(entry!(
        0x0043_2220,
        extra_data::nodes::objects::fn_00432220(Ptr<_>, f32) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Float])
    .run()
    .assert_same();
}

#[test]
fn n_00432250() {
    Diff::new(entry!(
        0x0043_2250,
        extra_data::nodes::objects::extra_scale_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_004322c0() {
    Diff::new(entry!(
        0x0043_22c0,
        extra_data::nodes::objects::fn_004322c0(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004322f0() {
    Diff::new(entry!(
        0x0043_22f0,
        extra_data::nodes::objects::fn_004322f0(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00432320() {
    Diff::new(entry!(
        0x0043_2320,
        extra_data::nodes::objects::fn_00432320(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00432350() {
    Diff::new(entry!(
        0x0043_2350,
        extra_data::nodes::objects::fn_00432350(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00432380() {
    Diff::new(entry!(
        0x0043_2380,
        extra_data::nodes::objects::fn_00432380(Ptr<_>, i8) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Word(&[0, 1, 2, 0x10, 0xffff_ffff])])
    .run()
    .assert_same();
}

#[test]
fn n_004323b0() {
    Diff::new(entry!(
        0x0043_23b0,
        extra_data::nodes::objects::extra_info_general_topic_destructor(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00432440() {
    Diff::new(entry!(
        0x0043_2440,
        extra_data::nodes::objects::extra_info_general_topic_scalar_deleting_destructor(
            Ptr<_>,
            u32,
        )
            -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x004323b0])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00432470() {
    Diff::new(entry!(
        0x0043_2470,
        extra_data::nodes::objects::fn_00432470(Ptr<_>, Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_004324a0() {
    Diff::new(entry!(
        0x0043_24a0,
        extra_data::nodes::objects::fn_004324a0(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00432540() {
    Diff::new(entry!(
        0x0043_2540,
        extra_data::nodes::objects::extra_hot_key_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_004325b0() {
    Diff::new(entry!(
        0x0043_25b0,
        extra_data::nodes::objects::fn_004325b0(Ptr<_>, u8) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Word(&[0, 1, 2, 5, 0x14])])
    .run()
    .assert_same();
}

#[test]
fn n_004325e0() {
    Diff::new(entry!(
        0x0043_25e0,
        extra_data::nodes::objects::extra_seed_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00432650() {
    Diff::new(entry!(
        0x0043_2650,
        extra_data::nodes::objects::fn_00432650(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004326c0() {
    Diff::new(entry!(
        0x0043_26c0,
        extra_data::nodes::objects::fn_004326c0(Ptr<_>, Ptr<_>, Ptr<_>, Ptr<_>, f32) -> Ptr<_>
    ))
    .node(&[
        A::This(0x40),
        A::Obj(0x50),
        A::Obj(0x50),
        A::Obj(0x50),
        A::Float,
    ])
    .translation_calls_plainly(&[0x00432650])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00432770() {
    Diff::new(entry!(
        0x0043_2770,
        extra_data::nodes::objects::extra_package_start_location_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_004327e0() {
    Diff::new(entry!(
        0x0043_27e0,
        extra_data::nodes::objects::fn_004327e0(Ptr<_>, Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00432810() {
    Diff::new(entry!(
        0x0043_2810,
        extra_data::nodes::objects::fn_00432810(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00432870() {
    Diff::new(entry!(
        0x0043_2870,
        extra_data::nodes::objects::fn_00432870(Ptr<_>, Ptr<_>, i32, Ptr<_>, u8, u8, u8) -> Ptr<_>
    ))
    .node(&[
        A::This(0x40),
        A::Obj(0x50),
        A::Word(&[0, 1, 2, 5, 0x14, 0x15, 0xffff_ffff]),
        A::Obj(0x50),
        A::Word(&[0, 1, 2, 5, 0x14]),
        A::Word(&[0, 1, 2, 5, 0x14]),
        A::Word(&[0, 1, 2, 5, 0x14]),
    ])
    .run()
    .assert_same();
}

#[test]
fn n_004328d0() {
    Diff::new(entry!(
        0x0043_28d0,
        extra_data::nodes::objects::fn_004328d0(Ptr<_>, Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00432900() {
    Diff::new(entry!(
        0x0043_2900,
        extra_data::nodes::objects::extra_tres_pass_package_destructor(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_004329d0() {
    Diff::new(entry!(
        0x0043_29d0,
        extra_data::nodes::objects::fn_004329d0(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00432a70() {
    Diff::new(entry!(
        0x0043_2a70,
        extra_data::nodes::objects::extra_player_crime_list_scalar_deleting_destructor(
            Ptr<_>,
            u32,
        )
            -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x00432b50])
    .run()
    .assert_same();
}

#[test]
fn n_00432aa0() {
    Diff::new(entry!(
        0x0043_2aa0,
        extra_data::nodes::objects::fn_00432aa0(Ptr<_>, Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00432b50() {
    Diff::new(entry!(
        0x0043_2b50,
        extra_data::nodes::objects::fn_00432b50(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00432be0() {
    Diff::new(entry!(
        0x0043_2be0,
        extra_data::nodes::objects::fn_00432be0(Ptr<_>, i32) -> Ptr<_>
    ))
    .node(&[
        A::This(0x40),
        A::Word(&[0, 1, 2, 5, 0x14, 0x15, 0xffff_ffff]),
    ])
    .run()
    .assert_same();
}

#[test]
fn n_00432c20() {
    Diff::new(entry!(
        0x0043_2c20,
        extra_data::nodes::objects::fn_00432c20(Ptr<_>, Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00432c60() {
    Diff::new(entry!(
        0x0043_2c60,
        extra_data::nodes::objects::fn_00432c60(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x00432c90])
    .run()
    .assert_same();
}

#[test]
fn n_00432c90() {
    Diff::new(entry!(
        0x0043_2c90,
        extra_data::nodes::objects::fn_00432c90(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00432cb0() {
    Diff::new(entry!(
        0x0043_2cb0,
        extra_data::nodes::objects::extra_rag_doll_data_extra_rag_doll_data(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00432ce0() {
    Diff::new(entry!(
        0x0043_2ce0,
        extra_data::nodes::objects::extra_rag_doll_data_scalar_deleting_destructor(
            Ptr<_>,
            u32,
        )
            -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x00432d10, 0x00432da0])
    .run()
    .assert_same();
}

#[test]
fn n_00432d10() {
    Diff::new(entry!(
        0x0043_2d10,
        extra_data::nodes::objects::fn_00432d10(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .translation_calls_plainly(&[0x00432da0])
    .run()
    .assert_same();
}

#[test]
fn n_00432da0() {
    Diff::new(entry!(
        0x0043_2da0,
        extra_data::nodes::objects::fn_00432da0(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x50), A::Flag])
    .run()
    .assert_same();
}

#[test]
fn n_00432dd0() {
    Diff::new(entry!(
        0x0043_2dd0,
        extra_data::nodes::objects::extra_rag_doll_data_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00432e60() {
    Diff::new(entry!(
        0x0043_2e60,
        extra_data::nodes::objects::extra_encounter_zone_extra_encounter_zone(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00432e90() {
    Diff::new(entry!(
        0x0043_2e90,
        extra_data::nodes::objects::extra_encounter_zone_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00432f00() {
    Diff::new(entry!(
        0x0043_2f00,
        extra_data::nodes::objects::fn_00432f00(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00432f30() {
    Diff::new(entry!(
        0x0043_2f30,
        extra_data::nodes::objects::fn_00432f30(Ptr<_>)
    ))
    .node(&[A::This(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00432f50() {
    Diff::new(entry!(
        0x0043_2f50,
        extra_data::nodes::objects::fn_00432f50(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00432f80() {
    Diff::new(entry!(
        0x0043_2f80,
        extra_data::nodes::objects::extra_used_markers_get_marker_used(Ptr<_>, u32) -> bool
    ))
    .node(&[A::This(0x40), A::Word(&[0, 1, 2, 0x10, 0xffff_ffff])])
    .run()
    .assert_same();
}

#[test]
fn n_00432fc0() {
    Diff::new(entry!(
        0x0043_2fc0,
        extra_data::nodes::objects::extra_used_markers_set_marker_used(Ptr<_>, u32, u8)
    ))
    .node(&[
        A::This(0x40),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
        A::Word(&[0, 1, 2, 5, 0x14]),
    ])
    .run()
    .assert_same();
}

#[test]
fn n_00433010() {
    Diff::new(entry!(
        0x0043_3010,
        extra_data::nodes::objects::fn_00433010(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004330b0() {
    Diff::new(entry!(
        0x0043_30b0,
        extra_data::nodes::objects::extra_run_once_packs_scalar_deleting_destructor(
            Ptr<_>,
            u32,
        )
            -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x004330e0])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_004330e0() {
    Diff::new(entry!(
        0x0043_30e0,
        extra_data::nodes::objects::fn_004330e0(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_004331c0() {
    Diff::new(entry!(
        0x0043_31c0,
        extra_data::nodes::objects::fn_004331c0(Ptr<_>, u32, u8)
    ))
    .node(&[
        A::This(0x40),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
        A::Flag,
    ])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00433260() {
    Diff::new(entry!(
        0x0043_3260,
        extra_data::nodes::objects::fn_00433260(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00433300() {
    Diff::new(entry!(
        0x0043_3300,
        extra_data::nodes::objects::fn_00433300(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00433340() {
    Diff::new(entry!(
        0x0043_3340,
        extra_data::nodes::objects::extra_enable_state_parent_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_004333a0() {
    Diff::new(entry!(
        0x0043_33a0,
        extra_data::nodes::objects::fn_004333a0(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00433410() {
    Diff::new(entry!(
        0x0043_3410,
        extra_data::nodes::objects::extra_enable_state_children_scalar_deleting_destructor(
            Ptr<_>,
            u32,
        )
            -> Ptr<
            _,
        >
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x00433440])
    .run()
    .assert_same();
}

#[test]
fn n_00433440() {
    Diff::new(entry!(
        0x0043_3440,
        extra_data::nodes::objects::fn_00433440(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004334b0() {
    Diff::new(entry!(
        0x0043_34b0,
        extra_data::nodes::objects::fn_004334b0(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004334e0() {
    Diff::new(entry!(
        0x0043_34e0,
        extra_data::nodes::objects::extra_random_teleport_marker_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00433530() {
    Diff::new(entry!(
        0x0043_3530,
        extra_data::nodes::objects::fn_00433530(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004335a0() {
    Diff::new(entry!(
        0x0043_35a0,
        extra_data::nodes::objects::extra_linked_ref_children_scalar_deleting_destructor(
            Ptr<_>,
            u32,
        )
            -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x004335d0])
    .run()
    .assert_same();
}

#[test]
fn n_004335d0() {
    Diff::new(entry!(
        0x0043_35d0,
        extra_data::nodes::objects::fn_004335d0(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00433640() {
    Diff::new(entry!(
        0x0043_3640,
        extra_data::nodes::objects::fn_00433640(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00433670() {
    Diff::new(entry!(
        0x0043_3670,
        extra_data::nodes::objects::extra_linked_ref_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_004336c0() {
    Diff::new(entry!(
        0x0043_36c0,
        extra_data::nodes::objects::fn_004336c0(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004336f0() {
    Diff::new(entry!(
        0x0043_36f0,
        extra_data::nodes::objects::fn_004336f0(u32, Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]), A::This(0x50)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00433750() {
    Diff::new(entry!(
        0x0043_3750,
        extra_data::nodes::objects::fn_00433750(Ptr<_>)
    ))
    .node_real(&[A::This(0x50)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00433790() {
    Diff::new(entry!(
        0x0043_3790,
        extra_data::nodes::objects::fn_00433790(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004338b0() {
    Diff::new(entry!(
        0x0043_38b0,
        extra_data::nodes::objects::fn_004338b0(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00433a00() {
    Diff::new(entry!(
        0x0043_3a00,
        extra_data::nodes::objects::fn_00433a00(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Word(&[0, 1, 2, 0x10, 0xffff_ffff])])
    .translation_calls_plainly(&[0x004336f0])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00433a20() {
    Diff::new(entry!(
        0x0043_3a20,
        extra_data::nodes::objects::extra_activate_ref_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .translation_calls_plainly(&[0x004336f0, 0x00433a00])
    .run()
    .assert_same();
}

#[test]
fn n_00433ca0() {
    Diff::new(entry!(
        0x0043_3ca0,
        extra_data::nodes::objects::fn_00433ca0(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00433db0() {
    Diff::new(entry!(
        0x0043_3db0,
        extra_data::nodes::objects::extra_decal_refs_init_item(Ptr<_>, Ptr<_>)
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_004341e0() {
    Diff::new(entry!(
        0x0043_41e0,
        extra_data::nodes::objects::extra_decal_refs_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_004343c0() {
    Diff::new(entry!(
        0x0043_43c0,
        extra_data::nodes::objects::fn_004343c0(Ptr<_>)
    ))
    .node_real(&[A::Shaped(0x14, &[Fill::List(0x0c, 0x1c)], false)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00434410() {
    Diff::new(entry!(
        0x0043_4410,
        extra_data::nodes::objects::extra_decal_refs_get_ref_decal_data(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Word(&[0, 1, 2, 0x10, 0xffff_ffff])])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00434560() {
    Diff::new(entry!(
        0x0043_4560,
        extra_data::nodes::objects::fn_00434560(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00434620() {
    Diff::new(entry!(
        0x0043_4620,
        extra_data::nodes::objects::extra_reflected_refs_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00434800() {
    Diff::new(entry!(
        0x0043_4800,
        extra_data::nodes::objects::fn_00434800(Ptr<_>, u32)
    ))
    .node(&[A::This(0x40), A::Word(&[0, 1, 2, 0x10, 0xffff_ffff])])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_004348a0() {
    Diff::new(entry!(
        0x0043_48a0,
        extra_data::nodes::objects::fn_004348a0(Ptr<_>, u32, u32)
    ))
    .node(&[
        A::This(0x40),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
        A::Flag,
    ])
    .translation_calls_plainly(&[0x00434410])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00434950() {
    Diff::new(entry!(
        0x0043_4950,
        extra_data::nodes::objects::fn_00434950(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004349c0() {
    Diff::new(entry!(
        0x0043_49c0,
        extra_data::nodes::objects::extra_water_light_refs_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00434a70() {
    Diff::new(entry!(
        0x0043_4a70,
        extra_data::nodes::objects::extra_water_light_refs_copy(Ptr<_>, Ptr<_>)
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .translation_calls_plainly(&[0x00434f60, 0x00434fa0])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00434af0() {
    Diff::new(entry!(
        0x0043_4af0,
        extra_data::nodes::objects::fn_00434af0(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00434bb0() {
    Diff::new(entry!(
        0x0043_4bb0,
        extra_data::nodes::objects::extra_reflector_refs_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00434d90() {
    Diff::new(entry!(
        0x0043_4d90,
        extra_data::nodes::objects::extra_reflected_refs_clear_reference_data(Ptr<_>)
    ))
    .node_real(&[A::This(0x40)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00434dc0() {
    Diff::new(entry!(
        0x0043_4dc0,
        extra_data::nodes::objects::fn_00434dc0(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00434e30() {
    Diff::new(entry!(
        0x0043_4e30,
        extra_data::nodes::objects::extra_lit_water_refs_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00434ee0() {
    Diff::new(entry!(
        0x0043_4ee0,
        extra_data::nodes::objects::extra_lit_water_refs_copy(Ptr<_>, Ptr<_>)
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .translation_calls_plainly(&[0x00434f60, 0x00434fa0])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00434f60() {
    Diff::new(entry!(
        0x0043_4f60,
        extra_data::nodes::objects::fn_00434f60(Ptr<_>, u32)
    ))
    .node(&[A::This(0x40), A::Word(&[0, 1, 2, 0x10, 0xffff_ffff])])
    .run()
    .assert_same();
}

#[test]
fn n_00434fa0() {
    Diff::new(entry!(
        0x0043_4fa0,
        extra_data::nodes::objects::fn_00434fa0(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00434fc0() {
    Diff::new(entry!(
        0x0043_4fc0,
        extra_data::nodes::objects::fn_00434fc0(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00434ff0() {
    Diff::new(entry!(
        0x0043_4ff0,
        extra_data::nodes::objects::extra_merchant_container_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00435040() {
    Diff::new(entry!(
        0x0043_5040,
        extra_data::nodes::objects::fn_00435040(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00435070() {
    Diff::new(entry!(
        0x0043_5070,
        extra_data::nodes::objects::extra_lev_crea_modifier_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00435100() {
    Diff::new(entry!(
        0x0043_5100,
        extra_data::nodes::objects::fn_00435100(Ptr<_>) -> i32
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00435150() {
    Diff::new(entry!(
        0x0043_5150,
        extra_data::nodes::objects::fn_00435150(Ptr<_>, Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00435180() {
    Diff::new(entry!(
        0x0043_5180,
        extra_data::nodes::objects::extra_poison_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00435270() {
    Diff::new(entry!(
        0x0043_5270,
        extra_data::nodes::objects::extra_last_finished_sequence_scalar_deleting_destructor(
            Ptr<_>,
            u32,
        )
            -> Ptr<
            _,
        >
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x004352a0])
    .run()
    .assert_same();
}

#[test]
fn n_004352a0() {
    Diff::new(entry!(
        0x0043_52a0,
        extra_data::nodes::objects::fn_004352a0(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00435310() {
    Diff::new(entry!(
        0x0043_5310,
        extra_data::nodes::objects::extra_last_finished_sequence_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00435370() {
    Diff::new(entry!(
        0x0043_5370,
        extra_data::nodes::objects::fn_00435370(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004353a0() {
    Diff::new(entry!(
        0x0043_53a0,
        extra_data::nodes::objects::fn_004353a0(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x004353d0])
    .run()
    .assert_same();
}

#[test]
fn n_004353d0() {
    Diff::new(entry!(
        0x0043_53d0,
        extra_data::nodes::objects::fn_004353d0(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00435ac0() {
    Diff::new(entry!(
        0x0043_5ac0,
        extra_data::nodes::objects::fn_00435ac0(Ptr<_>)
    ))
    .node(&[A::This(0x50)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .translation_calls_plainly(&[0x00435b20])
    .run()
    .assert_same();
}

#[test]
fn n_00435bb0() {
    Diff::new(entry!(
        0x0043_5bb0,
        extra_data::nodes::objects::fn_00435bb0(Ptr<_>)
    ))
    .node(&[A::This(0x50)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .translation_calls_plainly(&[0x00435b20])
    .run()
    .assert_same();
}

#[test]
fn n_004353f0() {
    Diff::new(entry!(
        0x0043_53f0,
        extra_data::nodes::objects_p2::extra_x_target_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00435440() {
    Diff::new(entry!(
        0x0043_5440,
        extra_data::nodes::objects_p2::fn_00435440(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00435470() {
    Diff::new(entry!(
        0x0043_5470,
        extra_data::nodes::objects_p2::fn_00435470(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x004354a0])
    .run()
    .assert_same();
}

#[test]
fn n_004354a0() {
    Diff::new(entry!(
        0x0043_54a0,
        extra_data::nodes::objects_p2::fn_004354a0(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004354c0() {
    Diff::new(entry!(
        0x0043_54c0,
        extra_data::nodes::objects_p2::extra_emittance_source_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00435510() {
    Diff::new(entry!(
        0x0043_5510,
        extra_data::nodes::objects_p2::fn_00435510(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00435540() {
    Diff::new(entry!(
        0x0043_5540,
        extra_data::nodes::objects_p2::extra_multi_bound_ref_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00435590() {
    Diff::new(entry!(
        0x0043_5590,
        extra_data::nodes::objects_p2::fn_00435590(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004355c0() {
    Diff::new(entry!(
        0x0043_55c0,
        extra_data::nodes::objects_p2::extra_multi_bound_data_scalar_deleting_destructor(
            Ptr<_>,
            u32,
        )
            -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x004355f0])
    .run()
    .assert_same();
}

#[test]
fn n_004355f0() {
    Diff::new(entry!(
        0x0043_55f0,
        extra_data::nodes::objects_p2::fn_004355f0(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00435630() {
    Diff::new(entry!(
        0x0043_5630,
        extra_data::nodes::objects_p2::extra_multi_bound_data_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00435690() {
    Diff::new(entry!(
        0x0043_5690,
        extra_data::nodes::objects_p2::fn_00435690(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00435710() {
    Diff::new(entry!(
        0x0043_5710,
        extra_data::nodes::objects_p2::extra_multi_bound_scalar_deleting_destructor(
            Ptr<_>,
            u32,
        )
            -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x00435740])
    .run()
    .assert_same();
}

#[test]
fn n_00435740() {
    Diff::new(entry!(
        0x0043_5740,
        extra_data::nodes::objects_p2::fn_00435740(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004357a0() {
    Diff::new(entry!(
        0x0043_57a0,
        extra_data::nodes::objects_p2::fn_004357a0(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00435820() {
    Diff::new(entry!(
        0x0043_5820,
        extra_data::nodes::objects_p2::fn_00435820(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004358a0() {
    Diff::new(entry!(
        0x0043_58a0,
        extra_data::nodes::objects_p2::fn_004358a0(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00435920() {
    Diff::new(entry!(
        0x0043_5920,
        extra_data::nodes::objects_p2::fn_00435920(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00435950() {
    Diff::new(entry!(
        0x0043_5950,
        extra_data::nodes::objects_p2::fn_00435950(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004359c0() {
    Diff::new(entry!(
        0x0043_59c0,
        extra_data::nodes::objects_p2::extra_dropped_item_list_scalar_deleting_destructor(
            Ptr<_>,
            u32,
        )
            -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x004359f0])
    .run()
    .assert_same();
}

#[test]
fn n_004359f0() {
    Diff::new(entry!(
        0x0043_59f0,
        extra_data::nodes::objects_p2::fn_004359f0(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00435a60() {
    Diff::new(entry!(
        0x0043_5a60,
        extra_data::nodes::objects_p2::fn_00435a60(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00435a90() {
    Diff::new(entry!(
        0x0043_5a90,
        extra_data::nodes::objects_p2::extra_saved_animation_scalar_deleting_destructor(
            Ptr<_>,
            u32,
        )
            -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .translation_calls_plainly(&[0x00435b20])
    .run()
    .assert_same();
}

#[test]
fn n_00435b20() {
    Diff::new(entry!(
        0x0043_5b20,
        extra_data::nodes::objects_p2::fn_00435b20(Ptr<_>)
    ))
    .node(&[A::This(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00435b50() {
    Diff::new(entry!(
        0x0043_5b50,
        extra_data::nodes::objects_p2::fn_00435b50(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00435b80() {
    Diff::new(entry!(
        0x0043_5b80,
        extra_data::nodes::objects_p2::extra_saved_havok_data_scalar_deleting_destructor(
            Ptr<_>,
            u32,
        )
            -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .translation_calls_plainly(&[0x00435b20])
    .run()
    .assert_same();
}

#[test]
fn n_00435c10() {
    Diff::new(entry!(
        0x0043_5c10,
        extra_data::nodes::objects_p2::extra_saved_animation_delete_buffer(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .translation_calls_plainly(&[0x00435b20])
    .run()
    .assert_same();
}

#[test]
fn n_00435c40() {
    Diff::new(entry!(
        0x0043_5c40,
        extra_data::nodes::objects_p2::extra_friend_hits_extra_friend_hits(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .translation_calls_plainly(&[0x00438a60])
    .run()
    .assert_same();
}

#[test]
fn n_00435cb0() {
    Diff::new(entry!(
        0x0043_5cb0,
        extra_data::nodes::objects_p2::extra_friend_hits_scalar_deleting_destructor(
            Ptr<_>,
            u32,
        )
            -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x00435ce0])
    .run()
    .assert_same();
}

#[test]
fn n_00435ce0() {
    Diff::new(entry!(
        0x0043_5ce0,
        extra_data::nodes::objects_p2::fn_00435ce0(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00435de0() {
    Diff::new(entry!(
        0x0043_5de0,
        extra_data::nodes::objects_p2::fn_00435de0(Ptr<_>, f32) -> Ptr<_>
    ))
    .node(&[A::This(0x50), A::Float])
    .run()
    .assert_same();
}

#[test]
fn n_00435ec0() {
    Diff::new(entry!(
        0x0043_5ec0,
        extra_data::nodes::objects_p2::fn_00435ec0(Ptr<_>, Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00435ef0() {
    Diff::new(entry!(
        0x0043_5ef0,
        extra_data::nodes::objects_p2::fn_00435ef0(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00435f20() {
    Diff::new(entry!(
        0x0043_5f20,
        extra_data::nodes::objects_p2::fn_00435f20(Ptr<_>, f32) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Float])
    .run()
    .assert_same();
}

#[test]
fn n_00435f50() {
    Diff::new(entry!(
        0x0043_5f50,
        extra_data::nodes::objects_p2::fn_00435f50(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .run()
    .assert_same();
}

#[test]
fn n_00435f80() {
    Diff::new(entry!(
        0x0043_5f80,
        extra_data::nodes::objects_p2::fn_00435f80(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00435fa0() {
    Diff::new(entry!(
        0x0043_5fa0,
        extra_data::nodes::objects_p2::fn_00435fa0(Ptr<_>, Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .translation_calls_plainly(&[0x00430830])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00436090() {
    Diff::new(entry!(
        0x0043_6090,
        extra_data::nodes::objects_p2::fn_00436090(Ptr<_>, u8) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Word(&[0, 1, 2, 5, 0x14])])
    .run()
    .assert_same();
}

#[test]
fn n_004360c0() {
    Diff::new(entry!(
        0x0043_60c0,
        extra_data::nodes::objects_p2::fn_004360c0(Ptr<_>, u32, u32, u32) -> Ptr<_>
    ))
    .node(&[
        A::This(0x40),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
    ])
    .run()
    .assert_same();
}

#[test]
fn n_00436150() {
    Diff::new(entry!(
        0x0043_6150,
        extra_data::nodes::objects_p2::fn_00436150(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x00436180])
    .run()
    .assert_same();
}

#[test]
fn n_00436180() {
    Diff::new(entry!(
        0x0043_6180,
        extra_data::nodes::objects_p2::fn_00436180(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004361e0() {
    Diff::new(entry!(
        0x0043_61e0,
        extra_data::nodes::objects_p2::fn_004361e0(Ptr<_>, u32, u32, u32) -> Ptr<_>
    ))
    .node(&[
        A::This(0x40),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
    ])
    .run()
    .assert_same();
}

#[test]
fn n_00436270() {
    Diff::new(entry!(
        0x0043_6270,
        extra_data::nodes::objects_p2::fn_00436270(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x004362a0])
    .run()
    .assert_same();
}

#[test]
fn n_004362a0() {
    Diff::new(entry!(
        0x0043_62a0,
        extra_data::nodes::objects_p2::fn_004362a0(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00436300() {
    Diff::new(entry!(
        0x0043_6300,
        extra_data::nodes::objects_p2::fn_00436300(Ptr<_>, u32, u32, u32) -> Ptr<_>
    ))
    .node(&[
        A::This(0x40),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
    ])
    .run()
    .assert_same();
}

#[test]
fn n_00436390() {
    Diff::new(entry!(
        0x0043_6390,
        extra_data::nodes::objects_p2::fn_00436390(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x004363c0])
    .run()
    .assert_same();
}

#[test]
fn n_004363c0() {
    Diff::new(entry!(
        0x0043_63c0,
        extra_data::nodes::objects_p2::fn_004363c0(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00436420() {
    Diff::new(entry!(
        0x0043_6420,
        extra_data::nodes::objects_p2::fn_00436420(Ptr<_>, u32, u32, u32) -> Ptr<_>
    ))
    .node(&[
        A::This(0x40),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
    ])
    .run()
    .assert_same();
}

#[test]
fn n_004364b0() {
    Diff::new(entry!(
        0x0043_64b0,
        extra_data::nodes::objects_p2::fn_004364b0(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x004364e0])
    .run()
    .assert_same();
}

#[test]
fn n_004364e0() {
    Diff::new(entry!(
        0x0043_64e0,
        extra_data::nodes::objects_p2::fn_004364e0(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00436540() {
    Diff::new(entry!(
        0x0043_6540,
        extra_data::nodes::objects_p2::fn_00436540(Ptr<_>, u32, u32, u32) -> Ptr<_>
    ))
    .node(&[
        A::This(0x40),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
    ])
    .run()
    .assert_same();
}

#[test]
fn n_004365d0() {
    Diff::new(entry!(
        0x0043_65d0,
        extra_data::nodes::objects_p2::fn_004365d0(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x00436600])
    .run()
    .assert_same();
}

#[test]
fn n_00436600() {
    Diff::new(entry!(
        0x0043_6600,
        extra_data::nodes::objects_p2::fn_00436600(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00436660() {
    Diff::new(entry!(
        0x0043_6660,
        extra_data::nodes::objects_p2::fn_00436660(Ptr<_>, u32, u32, u32) -> Ptr<_>
    ))
    .node(&[
        A::This(0x40),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
    ])
    .run()
    .assert_same();
}

#[test]
fn n_004366f0() {
    Diff::new(entry!(
        0x0043_66f0,
        extra_data::nodes::objects_p2::fn_004366f0(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x00436720])
    .run()
    .assert_same();
}

#[test]
fn n_00436720() {
    Diff::new(entry!(
        0x0043_6720,
        extra_data::nodes::objects_p2::fn_00436720(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00436780() {
    Diff::new(entry!(
        0x0043_6780,
        extra_data::nodes::objects_p2::fn_00436780(Ptr<_>, Ptr<_>, Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x50), A::Obj(0x50), A::Obj(0x50)])
    .translation_calls_plainly(&[0x00430830, 0x00436aa0])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00436ac0() {
    Diff::new(entry!(
        0x0043_6ac0,
        extra_data::nodes::objects_p2::fn_00436ac0(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00436af0() {
    Diff::new(entry!(
        0x0043_6af0,
        extra_data::nodes::objects_p2::fn_00436af0(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x00436bb0])
    .run()
    .assert_same();
}

#[test]
fn n_00436bb0() {
    Diff::new(entry!(
        0x0043_6bb0,
        extra_data::nodes::objects_p2::fn_00436bb0(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00436bd0() {
    Diff::new(entry!(
        0x0043_6bd0,
        extra_data::nodes::objects_p2::extra_radius_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00436c40() {
    Diff::new(entry!(
        0x0043_6c40,
        extra_data::nodes::objects_p2::extra_radiation_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00436cb0() {
    Diff::new(entry!(
        0x0043_6cb0,
        extra_data::nodes::objects_p2::fn_00436cb0(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00436d50() {
    Diff::new(entry!(
        0x0043_6d50,
        extra_data::nodes::objects_p2::extra_faction_changes_scalar_deleting_destructor(
            Ptr<_>,
            u32,
        )
            -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x00436d80])
    .run()
    .assert_same();
}

#[test]
fn n_00436d80() {
    Diff::new(entry!(
        0x0043_6d80,
        extra_data::nodes::objects_p2::fn_00436d80(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00436e10() {
    Diff::new(entry!(
        0x0043_6e10,
        extra_data::nodes::objects_p2::fn_00436e10(Ptr<_>, Ptr<_>)
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00436f20() {
    Diff::new(entry!(
        0x0043_6f20,
        extra_data::nodes::objects_p2::fn_00436f20(Ptr<_>, Ptr<_>, u8)
    ))
    .node(&[A::This(0x40), A::Obj(0x50), A::Word(&[0, 1, 2, 5, 0x14])])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00436f80() {
    Diff::new(entry!(
        0x0043_6f80,
        extra_data::nodes::objects_p2::fn_00436f80(Ptr<_>, Ptr<_>, u8)
    ))
    .node(&[A::This(0x40), A::Obj(0x50), A::Word(&[0, 1, 2, 5, 0x14])])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00437010() {
    Diff::new(entry!(
        0x0043_7010,
        extra_data::nodes::objects_p2::extra_faction_changes_get_is_in_faction(
            Ptr<_>,
            Ptr<_>,
        ) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00437080() {
    Diff::new(entry!(
        0x0043_7080,
        extra_data::nodes::objects_p2::extra_faction_changes_get_is_expelled(
            Ptr<_>,
            Ptr<_>,
        ) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00437240() {
    Diff::new(entry!(
        0x0043_7240,
        extra_data::nodes::objects_p2::fn_00437240(Ptr<_>, Ptr<_>)
    ))
    .node(&[A::This(0x40), A::Obj(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00437290() {
    Diff::new(entry!(
        0x0043_7290,
        extra_data::nodes::objects_p2::extra_radio_data_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00437300() {
    Diff::new(entry!(
        0x0043_7300,
        extra_data::nodes::objects_p2::extra_combat_style_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00437370() {
    Diff::new(entry!(
        0x0043_7370,
        extra_data::nodes::objects_p2::extra_primitive_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_004373c0() {
    Diff::new(entry!(
        0x0043_73c0,
        extra_data::nodes::objects_p2::extra_ammo_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00437440() {
    Diff::new(entry!(
        0x0043_7440,
        extra_data::nodes::objects_p2::fn_00437440(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004374e0() {
    Diff::new(entry!(0x0043_74e0, extra_data::nodes::objects_p2::extra_say_topic_info_once_a_day_scalar_deleting_destructor(Ptr<_>, u32) -> Ptr<_>))
        .node_real(&[A::This(0x40), A::Flag])
        .translation_calls_plainly(&[0x004375c0])
        .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
        .assert_same();
}

#[test]
fn n_00437510() {
    Diff::new(entry!(
        0x0043_7510,
        extra_data::nodes::objects_p2::fn_00437510(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Word(&[0, 1, 2, 0x10, 0xffff_ffff])])
    .translation_calls_plainly(&[0x00437440])
    .run()
    .assert_same();
}

#[test]
fn n_004375c0() {
    Diff::new(entry!(
        0x0043_75c0,
        extra_data::nodes::objects_p2::fn_004375c0(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00437690() {
    Diff::new(entry!(
        0x0043_7690,
        extra_data::nodes::objects_p2::fn_00437690(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_004376e0() {
    Diff::new(entry!(
        0x0043_76e0,
        extra_data::nodes::objects_p2::fn_004376e0(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Word(&[0, 1, 2, 0x10, 0xffff_ffff])])
    .run()
    .assert_same();
}

#[test]
fn n_00437730() {
    Diff::new(entry!(
        0x0043_7730,
        extra_data::nodes::objects_p2::fn_00437730(Ptr<_>, u32)
    ))
    .node(&[A::This(0x40), A::Word(&[0, 1, 2, 0x10, 0xffff_ffff])])
    .run()
    .assert_same();
}

#[test]
fn n_00437750() {
    Diff::new(entry!(
        0x0043_7750,
        extra_data::nodes::objects_p2::fn_00437750(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .translation_calls_plainly(&[0x004386b0])
    .run()
    .assert_same();
}

#[test]
fn n_004377c0() {
    Diff::new(entry!(
        0x0043_77c0,
        extra_data::nodes::objects_p2::extra_water_zone_map_scalar_deleting_destructor(
            Ptr<_>,
            u32,
        )
            -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x004377f0, 0x00438780])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_004377f0() {
    Diff::new(entry!(
        0x0043_77f0,
        extra_data::nodes::objects_p2::fn_004377f0(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .translation_calls_plainly(&[0x00438780])
    .run()
    .assert_same();
}

#[test]
fn n_00437970() {
    Diff::new(entry!(
        0x0043_7970,
        extra_data::nodes::objects_p2::fn_00437970(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_004379a0() {
    Diff::new(entry!(
        0x0043_79a0,
        extra_data::nodes::objects_p2::fn_004379a0(Ptr<_>, Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00437a30() {
    Diff::new(entry!(
        0x0043_7a30,
        extra_data::nodes::objects_p2::extra_patrol_ref_in_use_data_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x40)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00437a60() {
    Diff::new(entry!(
        0x0043_7a60,
        extra_data::nodes::objects_p2::fn_00437a60(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x40), A::Flag])
    .translation_calls_plainly(&[0x00437a90])
    .run()
    .assert_same();
}

#[test]
fn n_00437a90() {
    Diff::new(entry!(
        0x0043_7a90,
        extra_data::nodes::objects_p2::fn_00437a90(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00437b90() {
    Diff::new(entry!(
        0x0043_7b90,
        extra_data::nodes::objects_p2::fn_00437b90(Ptr<_>) -> bool
    ))
    .node(&[A::This(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00437bb0() {
    Diff::new(entry!(
        0x0043_7bb0,
        extra_data::nodes::objects_p2::fn_00437bb0(Ptr<_>) -> u8
    ))
    .node(&[A::This(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00437bd0() {
    Diff::new(entry!(
        0x0043_7bd0,
        extra_data::nodes::objects_p2::fn_00437bd0(Ptr<_>) -> bool
    ))
    .node(&[A::This(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00437bf0() {
    Diff::new(entry!(
        0x0043_7bf0,
        extra_data::nodes::objects_p2::fn_00437bf0(Ptr<_>) -> bool
    ))
    .node(&[A::This(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00437c10() {
    Diff::new(entry!(
        0x0043_7c10,
        extra_data::nodes::objects_p2::fn_00437c10(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00437ce0() {
    Diff::new(entry!(0x0043_7ce0, extra_data::nodes::objects_p2::extra_follower_swim_breadcrumbs_scalar_deleting_destructor(Ptr<_>, u32) -> Ptr<_>))
        .node(&[A::This(0x40), A::Flag])
        .translation_calls_plainly(&[0x00437d10])
        .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
        .assert_same();
}

#[test]
fn n_00437d10() {
    Diff::new(entry!(
        0x0043_7d10,
        extra_data::nodes::objects_p2::fn_00437d10(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00437dd0() {
    Diff::new(entry!(
        0x0043_7dd0,
        extra_data::nodes::objects_p2::fn_00437dd0(Ptr<_>) -> u32
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00437df0() {
    Diff::new(entry!(
        0x0043_7df0,
        extra_data::nodes::objects_p2::fn_00437df0(Ptr<_>)
    ))
    .node(&[A::This(0x40)])
    .run()
    .assert_same();
}

#[test]
fn n_00437e50() {
    Diff::new(entry!(
        0x0043_7e50,
        extra_data::nodes::objects_p2::fn_00437e50(Ptr<_>, u32, u32, u32, u32, u32) -> bool
    ))
    .node(&[
        A::This(0x40),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
    ])
    .translation_calls_plainly(&[0x00437c10])
    .run()
    .assert_same();
}

#[test]
fn n_00438170() {
    Diff::new(entry!(
        0x0043_8170,
        extra_data::nodes::objects_p2::fixed_string_construct(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x50), A::Word(&[0, 1, 2, 0x10, 0xffff_ffff])])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00438210() {
    Diff::new(entry!(
        0x0043_8210,
        extra_data::nodes::objects_p2::fn_00438210(Ptr<_>) -> u32
    ))
    .node(&[A::This(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00438220() {
    Diff::new(entry!(0x0043_8220, extra_data::nodes::objects_p2::fn_00438220() -> u32))
        .node(&[])
        .run()
        .assert_same();
}

#[test]
fn n_00438230() {
    Diff::new(entry!(
        0x0043_8230,
        extra_data::nodes::objects_p2::fn_00438230(Ptr<_>, u32)
    ))
    .node(&[A::This(0x50), A::Word(&[0, 1, 2, 0x10, 0xffff_ffff])])
    .run()
    .assert_same();
}

#[test]
fn n_00438260() {
    Diff::new(entry!(
        0x0043_8260,
        extra_data::nodes::objects_p2::extra_securitron_face_set_personality(Ptr<_>, u32)
    ))
    .node(&[A::This(0x40), A::Word(&[0, 1, 2, 0x10, 0xffff_ffff])])
    .run()
    .assert_same();
}

#[test]
fn n_00438280() {
    Diff::new(entry!(
        0x0043_8280,
        extra_data::nodes::objects_p2::extra_securitron_face_set_mood(Ptr<_>, u32)
    ))
    .node(&[A::This(0x40), A::Word(&[0, 1, 2, 0x10, 0xffff_ffff])])
    .run()
    .assert_same();
}

#[test]
fn n_004382a0() {
    Diff::new(entry!(
        0x0043_82a0,
        extra_data::nodes::objects_p3::extra_special_render_flags_compare(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00438330() {
    Diff::new(entry!(
        0x0043_8330,
        extra_data::nodes::objects_p3::extra_special_render_flags_copy(Ptr<_>, Ptr<_>)
    ))
    .node(&[A::This(0x40), A::Obj(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00438390() {
    Diff::new(entry!(
        0x0043_8390,
        extra_data::nodes::objects_p3::fn_00438390(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x50), A::Word(&[0, 1, 2, 0x10, 0xffff_ffff])])
    .run()
    .assert_same();
}

#[test]
fn n_004383b0() {
    Diff::new(entry!(
        0x0043_83b0,
        extra_data::nodes::objects_p3::fn_004383b0(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x50), A::Word(&[0, 1, 2, 0x10, 0xffff_ffff])])
    .translation_calls_plainly(&[0x004386b0])
    .run()
    .assert_same();
}

#[test]
fn n_004383e0() {
    Diff::new(entry!(
        0x0043_83e0,
        extra_data::nodes::objects_p3::fn_004383e0(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x50), A::Word(&[0, 1, 2, 0x10, 0xffff_ffff])])
    .translation_calls_plainly(&[0x00438820])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00438410() {
    Diff::new(entry!(
        0x0043_8410,
        extra_data::nodes::objects_p3::ni_t_map_scalar_deleting_destructor(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x50), A::Flag])
    .translation_calls_plainly(&[0x00438720, 0x00438780])
    .run()
    .assert_same();
}

#[test]
fn n_00438440() {
    Diff::new(entry!(
        0x0043_8440,
        extra_data::nodes::objects_p3::bs_map_scalar_deleting_destructor(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x50), A::Flag])
    .translation_calls_plainly(&[0x004388a0, 0x00438900])
    .run()
    .assert_same();
}

#[test]
fn n_00438570() {
    Diff::new(entry!(
        0x0043_8570,
        extra_data::nodes::objects_p3::fn_00438570(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_004385a0() {
    Diff::new(entry!(
        0x0043_85a0,
        extra_data::nodes::objects_p3::fn_004385a0(Ptr<_>)
    ))
    .node(&[A::This(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_004385c0() {
    Diff::new(entry!(
        0x0043_85c0,
        extra_data::nodes::objects_p3::fn_004385c0(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x50)])
    .translation_calls_plainly(&[0x00438a60])
    .run()
    .assert_same();
}

#[test]
fn n_004385f0() {
    Diff::new(entry!(
        0x0043_85f0,
        extra_data::nodes::objects_p3::fn_004385f0(Ptr<_>)
    ))
    .node(&[A::This(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00438660() {
    Diff::new(entry!(
        0x0043_8660,
        extra_data::nodes::objects_p3::fn_00438660(Ptr<_>) -> Ptr<_>
    ))
    .node(&[A::This(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00438690() {
    Diff::new(entry!(
        0x0043_8690,
        extra_data::nodes::objects_p3::fn_00438690(Ptr<_>)
    ))
    .node(&[A::This(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_004386b0() {
    Diff::new(entry!(
        0x0043_86b0,
        extra_data::nodes::objects_p3::fn_004386b0(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x50), A::Word(&[0, 1, 2, 0x10, 0xffff_ffff])])
    .run()
    .assert_same();
}

#[test]
fn n_00438720() {
    Diff::new(entry!(
        0x0043_8720,
        extra_data::nodes::objects_p3::fn_00438720(Ptr<_>)
    ))
    .node(&[A::This(0x50)])
    .translation_calls_plainly(&[0x00438780])
    .run()
    .assert_same();
}

#[test]
fn n_00438780() {
    Diff::new(entry!(
        0x0043_8780,
        extra_data::nodes::objects_p3::fn_00438780(Ptr<_>)
    ))
    .node(&[A::This(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_004387b0() {
    Diff::new(entry!(
        0x0043_87b0,
        extra_data::nodes::objects_p3::bs_simple_array_compare_buffer(Ptr<_>, Ptr<_>) -> bool
    ))
    .node(&[A::This(0x50), A::Obj(0x50)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00438820() {
    Diff::new(entry!(
        0x0043_8820,
        extra_data::nodes::objects_p3::fn_00438820(Ptr<_>, u32) -> Ptr<_>
    ))
    .node(&[A::This(0x50), A::Word(&[0, 1, 2, 0x10, 0xffff_ffff])])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_004388a0() {
    Diff::new(entry!(
        0x0043_88a0,
        extra_data::nodes::objects_p3::fn_004388a0(Ptr<_>)
    ))
    .node(&[A::This(0x50)])
    .translation_calls_plainly(&[0x00438900])
    .run()
    .assert_same();
}

#[test]
fn n_00438900() {
    Diff::new(entry!(
        0x0043_8900,
        extra_data::nodes::objects_p3::fn_00438900(Ptr<_>)
    ))
    .node(&[A::This(0x50)])
    .run()
    .assert_same();
}

#[test]
fn n_00438940() {
    Diff::new(entry!(
        0x0043_8940,
        extra_data::nodes::objects_p3::fn_00438940(Ptr<_>, Ptr<_>)
    ))
    .node(&[A::This(0x50), A::Obj(0x50)])
    .tolerate_garbage_stops(3)
    .max_calls(2000)
    .run()
    .assert_same();
}

#[test]
fn n_00438970() {
    Diff::new(entry!(0x0043_8970, extra_data::nodes::objects_p3::bs_simple_array_dismembered_limb_scalar_deleting_destructor(Ptr<_>, u32) -> Ptr<_>))
        .node(&[A::This(0x50), A::Flag])
        .translation_calls_plainly(&[0x004385a0])
        .run()
        .assert_same();
}

#[test]
fn n_004389a0() {
    Diff::new(entry!(0x0043_89a0, extra_data::nodes::objects_p3::bs_simple_array_combat_time_stamp_scalar_deleting_destructor(Ptr<_>, u32) -> Ptr<_>))
        .node(&[A::This(0x50), A::Flag])
        .translation_calls_plainly(&[0x004385f0])
        .run()
        .assert_same();
}

#[test]
fn n_004389d0() {
    Diff::new(entry!(
        0x0043_89d0,
        extra_data::nodes::objects_p3::bs_simple_array_uint_scalar_deleting_destructor(
            Ptr<_>,
            u32,
        )
            -> Ptr<_>
    ))
    .node(&[A::This(0x50), A::Flag])
    .translation_calls_plainly(&[0x00438690])
    .run()
    .assert_same();
}

#[test]
fn n_00438a00() {
    Diff::new(entry!(
        0x0043_8a00,
        extra_data::nodes::objects_p3::ni_t_map_base_scalar_deleting_destructor(
            Ptr<_>,
            u32,
        ) -> Ptr<_>
    ))
    .node(&[A::This(0x50), A::Flag])
    .translation_calls_plainly(&[0x00438780])
    .run()
    .assert_same();
}

#[test]
fn n_00438a30() {
    Diff::new(entry!(
        0x0043_8a30,
        extra_data::nodes::objects_p3::bs_map_base_scalar_deleting_destructor(
            Ptr<_>,
            u32,
        ) -> Ptr<_>
    ))
    .node(&[A::This(0x50), A::Flag])
    .translation_calls_plainly(&[0x00438900])
    .run()
    .assert_same();
}

#[test]
fn n_00438a60() {
    Diff::new(entry!(
        0x0043_8a60,
        extra_data::nodes::objects_p3::fn_00438a60(Ptr<_>, u32, u32)
    ))
    .node(&[
        A::This(0x50),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
        A::Word(&[0, 1, 2, 0x10, 0xffff_ffff]),
    ])
    .run()
    .assert_same();
}
