// SPDX-License-Identifier: GPL-3.0-only
//! The function at `0086a830`: a `thiscall` method with one stack argument that ignores both words
//! and returns zero. It reads nothing, writes nothing and calls nothing.

use engine_core::Engine;

// Typed from 0086a830
/// Ignores `this` and the stack word and returns zero (`thiscall`, one stack argument).
pub fn ignores_both(_e: &mut Engine, _this: u32, _word: u32) -> u8 {
    0
}
