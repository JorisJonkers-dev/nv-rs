// SPDX-License-Identifier: GPL-3.0-only
//! The singleton-byte getters of the `s680707dea67c970e` family: five tiny `cdecl` functions in the
//! shared engine parts that only load one byte from the exe's data and return it (the world update
//! flag, the Havok step flag, the scene flag, the interface draw flag and the semaphore pool's in-use
//! flag). Each translation is a single byte load with no call and no side effect; the typed version
//! reads the same address in place, so the family is a stage-1 family with no objects of its own.
//!
//! The family is off by default: it is a research family of look-alike one-byte getters, not a
//! subsystem, so the switch only turns it on when named.

use engine_core::typed::{Family, FamilyId, Threads};
use engine_core::{entry, AbiFn};

pub mod globals;

/// This family's bit: what each `typed_call!` of it would name (its functions call nothing).
pub const FAMILY: FamilyId = FamilyId::new(60);

/// `NV_TYPED=s680707dea67c970e`.
pub static S680707DEA67C970E: Family = Family {
    name: "s680707dea67c970e",
    id: FAMILY,
    past_gate: false,
    threads: Threads::Both,
    stage2: false,
    funcs,
};

/// The family's functions, by exe address.
#[rustfmt::skip]
fn funcs() -> Vec<(u32, AbiFn)> {
    vec![
        entry!(0x0086_ff40, globals::world_update_flag() -> u8),
        entry!(0x0052_5420, globals::havok_step_flag() -> u8),
        entry!(0x0087_05c0, globals::scene_flag() -> u8),
        entry!(0x0087_09f0, globals::interface_draw_flag() -> u8),
        entry!(0x0071_3d90, globals::semaphore_pool_in_use() -> u8),
    ]
}
