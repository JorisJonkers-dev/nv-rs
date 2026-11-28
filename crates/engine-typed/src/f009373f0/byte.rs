// SPDX-License-Identifier: GPL-3.0-only
//! The one byte getter of the `009373f0` family. Its translation only loads the byte at the object
//! it is given, so there is no call to name, no float rounding to match and no side effect beyond
//! the read.

use engine_core::{Engine, Ptr};

// Typed from 009373f0
/// The byte at `this` (`thiscall`, no stack words). The engine map has no name for it: the quest
/// stage item's `InitItem` (`0060fa10`) calls it for the actor value byte of a modifier, and the
/// interface's wrappers read the same byte as their ready flag. The high bytes of the word at
/// `this` are never used.
pub fn fn_009373f0(e: &mut Engine, this: Ptr) -> u8 {
    e.mem.u8(this.addr())
}
