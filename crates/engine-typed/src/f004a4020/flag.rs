// SPDX-License-Identifier: GPL-3.0-only
//! The function at `004a4020`: the signed byte at `this + 0x38` is positive.

use engine_core::ptr::Ptr;
use engine_core::Engine;

// Typed from 004a4020
/// True when the signed byte at `this + 0x38` is greater than zero (`thiscall`, no stack argument).
pub fn byte_is_positive(e: &mut Engine, this: Ptr) -> bool {
    e.mem.i8(this.addr().wrapping_add(0x38)) > 0
}
