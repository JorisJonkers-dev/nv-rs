// SPDX-License-Identifier: GPL-3.0-only
//! The differential test of the `s680707dea67c970e` family's five one-byte getters against their
//! translations: `0086ff40` (the world update flag), `00525420` (the Havok step flag), `008705c0`
//! (the scene flag), `008709f0` (the interface draw flag) and `00713d90` (the semaphore pool's
//! in-use byte). The translations live in three part crates, so the suite is here, where the engine
//! facade assembles them all.
//!
//! Each case maps the page holding the global and writes one byte there before the call, so the
//! getter reads a value that varies by case rather than the zero of an unmapped byte; the setup is
//! the same on both sides. The getters make no calls and write no memory, so no callee needs standing
//! in and the leaves family is named only as the other family whose words the run could reach.

use engine as _;
use engine_core::prelude::*;
use engine_core::typed::diff::{Case, Diff, Input};
use engine_core::typed::leaf;
use engine_typed::s680707dea67c970e::{self, globals};

/// The words whose low bytes are written to the global under test: zero, small values, both signs
/// and bit patterns whose truncation to a byte is interesting.
const BYTES: &[u32] = &[
    0,
    1,
    2,
    0x7f,
    0x80,
    0xff,
    0x100,
    0x5a5a_5a5a,
    0x7fff_ffff,
    0xffff_ffff,
];

/// Maps the page holding `at` and stores the case's byte there, identically on both sides. The
/// call's own words are empty: these getters take no arguments.
fn byte(e: &mut Engine, case: &Case, at: u32) {
    e.map(at & !0xfff, 0x1000);
    e.mem.set_u8(at, case.word(0) as u8);
}

/// `0086ff40`: the world update flag at `0118abb0`.
#[test]
fn world_update_flag() {
    Diff::new(entry!(0x0086_ff40, globals::world_update_flag() -> u8))
        .family(&s680707dea67c970e::S680707DEA67C970E)
        .family(&leaf::LEAVES)
        .inputs(&[Input::Of(BYTES)])
        .setup(|e, case| {
            byte(e, case, 0x0118_abb0);
            vec![]
        })
        .run()
        .assert_same();
}

/// `00525420`: the Havok step flag at `011dea2a`.
#[test]
fn havok_step_flag() {
    Diff::new(entry!(0x0052_5420, globals::havok_step_flag() -> u8))
        .family(&s680707dea67c970e::S680707DEA67C970E)
        .family(&leaf::LEAVES)
        .inputs(&[Input::Of(BYTES)])
        .setup(|e, case| {
            byte(e, case, 0x011d_ea2a);
            vec![]
        })
        .run()
        .assert_same();
}

/// `008705c0`: the scene flag at `01189184`.
#[test]
fn scene_flag() {
    Diff::new(entry!(0x0087_05c0, globals::scene_flag() -> u8))
        .family(&s680707dea67c970e::S680707DEA67C970E)
        .family(&leaf::LEAVES)
        .inputs(&[Input::Of(BYTES)])
        .setup(|e, case| {
            byte(e, case, 0x0118_9184);
            vec![]
        })
        .run()
        .assert_same();
}

/// `008709f0`: the interface draw flag at `011f91a4`.
#[test]
fn interface_draw_flag() {
    Diff::new(entry!(0x0087_09f0, globals::interface_draw_flag() -> u8))
        .family(&s680707dea67c970e::S680707DEA67C970E)
        .family(&leaf::LEAVES)
        .inputs(&[Input::Of(BYTES)])
        .setup(|e, case| {
            byte(e, case, 0x011f_91a4);
            vec![]
        })
        .run()
        .assert_same();
}

/// `00713d90`: the semaphore pool's in-use byte at `011dfa19`.
#[test]
fn semaphore_pool_in_use() {
    Diff::new(entry!(0x0071_3d90, globals::semaphore_pool_in_use() -> u8))
        .family(&s680707dea67c970e::S680707DEA67C970E)
        .family(&leaf::LEAVES)
        .inputs(&[Input::Of(BYTES)])
        .setup(|e, case| {
            byte(e, case, 0x011d_fa19);
            vec![]
        })
        .run()
        .assert_same();
}
