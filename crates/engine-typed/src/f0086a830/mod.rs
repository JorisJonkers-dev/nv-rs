// SPDX-License-Identifier: GPL-3.0-only
//! The `0086a830` family: one `thiscall` method that takes `this` and one stack argument, ignores
//! both and returns zero (the per-frame idle update of the `Main` object calls it with its
//! frame budget). The translation has no read, no write and no call, so the typed version is the
//! same constant at the same address and the family is a stage-1 family with no objects of its own.
//!
//! The family is off by default: it is a research family of one look-alike function, not a
//! subsystem, so the switch only turns it on when named.

use engine_core::typed::{Family, FamilyId, Threads};
use engine_core::{entry, AbiFn};

pub mod stub;

/// This family's bit: what each `typed_call!` of it would name (its function calls nothing).
pub const FAMILY: FamilyId = FamilyId::new(58);

/// `NV_TYPED=f0086a830`.
pub static F0086A830: Family = Family {
    name: "f0086a830",
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
        entry!(0x0086_a830, stub::ignores_both(u32, u32) -> u8),
    ]
}
