// SPDX-License-Identifier: GPL-3.0-only
//! The `009373f0` family: the single byte getter of the AI modifier-list unit, which answers the
//! actor value byte of a modifier (and doubles as the interface manager's ready flag). The
//! translation is one load of the byte at `this`, with no call, no write and no float work; the
//! typed version is the same load at the same address. There are no objects of its own, so this is a
//! stage-1 family with no call tree to name.
//!
//! The family is off by default: it is a research family of one look-alike byte getter, not a
//! subsystem, so the switch only turns it on when named.

use engine_core::ptr::Ptr;
use engine_core::typed::{Family, FamilyId, Threads};
use engine_core::{entry, AbiFn};

pub mod byte;

/// This family's bit: what each `typed_call!` of it would name (its function calls nothing).
pub const FAMILY: FamilyId = FamilyId::new(61);

/// `NV_TYPED=f009373f0`.
pub static F009373F0: Family = Family {
    name: "f009373f0",
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
        entry!(0x0093_73f0, byte::fn_009373f0(Ptr) -> u8),
    ]
}
