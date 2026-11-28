// SPDX-License-Identifier: GPL-3.0-only
//! The differential test of the `009373f0` family's byte getter against its translation. Each case
//! builds a four-byte object, writes the case's byte at its first byte and other bytes around it,
//! and passes the object's address. Both sides read only the first byte, so the answer, the memory
//! after the call and the control word all have to agree.

use engine as _;
use engine_core::prelude::*;
use engine_core::typed::diff::{Case, Diff, Input};
use engine_core::typed::leaf;
use engine_typed::f009373f0::{self, byte};

/// Bytes worth separating from their word: zero, flags, either sign bit and the byte a modifier's
/// actor value uses.
const BYTES: &[u32] = &[0, 1, 2, 0x7f, 0x80, 0xa5, 0xff];

/// An object whose first byte holds the case's value, with its other bytes deliberately different:
/// the getter must answer that one byte and change nothing.
fn object(e: &mut Engine, case: &Case) -> u32 {
    let this = e.mem.alloc(4);
    e.mem.set_u32(this, case.word(0) | 0x1a2b_3c00);
    this
}

#[test]
fn modifier_byte() {
    Diff::new(entry!(0x0093_73f0, byte::fn_009373f0(Ptr) -> u8))
        .family(&f009373f0::F009373F0)
        .family(&leaf::LEAVES)
        .inputs(&[Input::Of(BYTES)])
        .setup(|e, case| vec![object(e, case)])
        .run()
        .assert_same();
}
