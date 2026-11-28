// SPDX-License-Identifier: GPL-3.0-only
//! The five one-byte globals the shared engine parts hand out through `cdecl` getters. Each
//! translation loads one byte from its exe address and returns it; there is no call tree, no float
//! maths and no write, so each typed function is the same load at the same address.

use engine_core::Engine;

/// The world update flag (`0118abb0`).
const WORLD_UPDATE_FLAG: u32 = 0x0118_abb0;
/// The Havok step flag (`011dea2a`).
const HAVOK_STEP_FLAG: u32 = 0x011d_ea2a;
/// The scene flag (`01189184`).
const SCENE_FLAG: u32 = 0x0118_9184;
/// The interface draw flag (`011f91a4`).
const INTERFACE_DRAW_FLAG: u32 = 0x011f_91a4;
/// The semaphore pool's in-use byte (`011dfa19`).
const SEMAPHORE_POOL_IN_USE: u32 = 0x011d_fa19;

// Typed from 0086ff40
/// The byte at `0118abb0`, read as the exe reads it (`cdecl`, no arguments).
pub fn byte_at_0118abb0(e: &mut Engine) -> u8 {
    e.global::<u8>(WORLD_UPDATE_FLAG)
}

// Typed from 00525420
/// The byte at `011dea2a` (`cdecl`, no arguments).
pub fn byte_at_011dea2a(e: &mut Engine) -> u8 {
    e.global::<u8>(HAVOK_STEP_FLAG)
}

// Typed from 008705c0
/// The byte at `01189184` (`cdecl`, no arguments).
pub fn byte_at_01189184(e: &mut Engine) -> u8 {
    e.global::<u8>(SCENE_FLAG)
}

// Typed from 008709f0
/// The byte at `011f91a4` (`cdecl`, no arguments).
pub fn byte_at_011f91a4(e: &mut Engine) -> u8 {
    e.global::<u8>(INTERFACE_DRAW_FLAG)
}

// Typed from 00713d90
/// Whether the semaphore pool is in use: the byte at `011dfa19` (`cdecl`, no arguments).
pub fn semaphore_pool_in_use(e: &mut Engine) -> u8 {
    e.global::<u8>(SEMAPHORE_POOL_IN_USE)
}
