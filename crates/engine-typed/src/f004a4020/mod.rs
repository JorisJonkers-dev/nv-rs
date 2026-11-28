// SPDX-License-Identifier: GPL-3.0-only
//! The `004a4020` family: one `thiscall` predicate that is true when the signed byte at
//! `this + 0x38` is positive (the menu-topmost checks call it on the object `0071b160` returns).
//! It reads one byte and writes and calls nothing, so the family is a stage-1 family with no
//! objects of its own.
//!
//! The family is off by default: it is a research family of one look-alike predicate, not a
//! subsystem, so the switch only turns it on when named.

use engine_core::ptr::Ptr;
use engine_core::typed::{Family, FamilyId, Threads};
use engine_core::{entry, AbiFn};

pub mod flag;

/// This family's bit: what each `typed_call!` of it would name (its function calls nothing).
pub const FAMILY: FamilyId = FamilyId::new(59);

/// `NV_TYPED=f004a4020`.
pub static F004A4020: Family = Family {
    name: "f004a4020",
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
        entry!(0x004a_4020, flag::byte_is_positive(Ptr) -> bool),
    ]
}
