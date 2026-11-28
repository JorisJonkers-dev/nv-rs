// SPDX-License-Identifier: GPL-3.0-only
//! The differential test of the `004a4020` family's one predicate against its translation: the
//! signed byte at `this + 0x38` is positive. The cases store each kind of byte there (zero,
//! small positive, the largest positive, the smallest negative, minus one) at objects in two
//! pages, so a typed version that compared unsigned would differ.

use engine as _;
use engine_core::prelude::*;
use engine_core::typed::diff::{Case, Diff, Input};
use engine_core::typed::leaf;
use engine_typed::f004a4020::{self, flag};

/// The bytes stored at `this + 0x38`, widened: both sides of zero and both ends of the byte.
const BYTES: &[u32] = &[0x00, 0x01, 0x02, 0x7f, 0x80, 0x81, 0xfe, 0xff];

/// Where the objects sit: one with the byte in the middle of a page, one with it on the first byte of the next.
const OBJECTS: &[u32] = &[0x0300_0000, 0x0300_0fc8];

/// `004a4020`: the signed byte at `this + 0x38` is positive.
#[test]
fn byte_is_positive() {
    Diff::new(entry!(0x004a_4020, flag::byte_is_positive(Ptr) -> bool))
        .family(&f004a4020::F004A4020)
        .family(&leaf::LEAVES)
        .inputs(&[Input::Of(OBJECTS), Input::Of(BYTES)])
        .setup(|e, case: &Case| {
            e.map(0x0300_0000, 0x2000);
            e.mem.set_u8(case.word(0) + 0x38, case.word(1) as u8);
            vec![case.word(0)]
        })
        .run()
        .assert_same();
}

/// The fuzz targets' entry (`scripts/fork/fuzz_gate.py`): the bytes fill the object's words from `this + 0x30` (the byte
/// the predicate reads is the ninth), and the two sides must agree on it.
pub fn replay_bytes(data: &[u8]) {
    let data = data.to_vec();
    Diff::new(entry!(0x004a_4020, flag::byte_is_positive(Ptr) -> bool))
        .family(&f004a4020::F004A4020)
        .family(&leaf::LEAVES)
        .inputs(&[Input::Of(OBJECTS)])
        .setup(move |e, case: &Case| {
            e.map(0x0300_0000, 0x2000);
            for (i, byte) in data.iter().take(16).enumerate() {
                e.mem.set_u8(case.word(0) + 0x30 + i as u32, *byte);
            }
            vec![case.word(0)]
        })
        .run()
        .assert_same();
}

/// The bytes the fuzz targets replay, on both sides of the sign of the ninth byte.
#[test]
fn replay_bytes_agrees() {
    replay_bytes(&[]);
    replay_bytes(&[0, 0, 0, 0, 0, 0, 0, 0, 0x7f]);
    replay_bytes(&[0, 0, 0, 0, 0, 0, 0, 0, 0x80, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
}
