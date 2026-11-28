// SPDX-License-Identifier: GPL-3.0-only
//! The differential test of the `0086a830` family's one function against its translation: a
//! `thiscall` method that ignores `this` and its stack word and returns zero. The cases vary both
//! words, so a typed version that returned either of them (or a stale register) would differ.

use engine as _;
use engine_core::prelude::*;
use engine_core::typed::diff::{Case, Diff, Input};
use engine_core::typed::leaf;
use engine_typed::f0086a830::{self, stub};

/// Words for `this` and the stack argument: zero, small values, both signs and pointer-like bits.
const WORDS: &[u32] = &[
    0,
    1,
    2,
    0x7f,
    0x5a5a_5a5a,
    0x7fff_ffff,
    0x8000_0000,
    0xffff_ffff,
];

/// `0086a830`: returns zero whatever `this` and the stack word are.
#[test]
fn ignores_both() {
    Diff::new(entry!(0x0086_a830, stub::ignores_both(u32, u32) -> u8))
        .family(&f0086a830::F0086A830)
        .family(&leaf::LEAVES)
        .inputs(&[Input::Of(WORDS), Input::Of(WORDS)])
        .setup(|_e, case: &Case| vec![case.word(0), case.word(1)])
        .run()
        .assert_same();
}
