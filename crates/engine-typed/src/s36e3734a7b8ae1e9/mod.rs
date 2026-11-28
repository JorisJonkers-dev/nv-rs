// SPDX-License-Identifier: GPL-3.0-only
//! The singleton-word getters of the `s36e3734a7b8ae1e9` family: five tiny `cdecl` functions in the
//! shared engine parts that only load a word from the exe's data and return it (the platform swap
//! word, the default texture setting, the memory limit, the audio instance and the task queue
//! object). Each translation is a single load with no call and no side effect; the typed version
//! reads the same address in place, so the family is a stage-1 family with no objects of its own.
//!
//! The family is off by default: it is a research family of look-alike one-word getters, not a
//! subsystem, so the switch only turns it on when named.

use engine_core::typed::{Family, FamilyId, Threads};
use engine_core::{entry, AbiFn};

pub mod globals;

/// This family's bit: what each `typed_call!` of it would name (its functions call nothing).
pub const FAMILY: FamilyId = FamilyId::new(57);

/// `NV_TYPED=s36e3734a7b8ae1e9`.
pub static S36E3734A7B8AE1E9: Family = Family {
    name: "s36e3734a7b8ae1e9",
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
        entry!(0x004b_7210, globals::platform_swap_word() -> u32),
        entry!(0x0043_c4b0, globals::default_texture_setting() -> u32),
        entry!(0x0087_81f0, globals::limit_word() -> u32),
        entry!(0x0045_3a70, globals::bs_audio_q_instance() -> u32),
        entry!(0x0045_37b0, globals::task_queue_object() -> u32),
    ]
}
