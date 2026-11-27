// SPDX-License-Identifier: GPL-3.0-only
//! The extra data node classes (nv-rs#952, step 1): the constructors, destructors, `Compare`, `Copy`
//! and `InitItem` bodies and payload accessors of the `BSExtraData` subclasses that hang on a
//! list's chain, typed so that a node can later be built and read only through typed code.
//!
//! The nodes are read and written through the family's store ([`store`]): in stage 1 (the family's
//! default today) that is the guest heap, as for every other family; the K1 move (a later step of
//! the ticket) changes what the store answers, not these functions. A node's vtable and type stay
//! in the guest word (untyped virtual calls read them); the rest of the node is the store's.

pub mod layouts;
pub mod objects;
pub mod objects_p2;
pub mod objects_p3;

use super::FAMILY;
use engine_core::typed::moved::Store;
use engine_core::Engine;

/// The family's store for a node's fields.
#[inline(always)]
pub(crate) fn store(e: &Engine) -> Store {
    e.typed_store(FAMILY)
}

/// The low 16 bits of a word, for a 16-bit field a word was built for.
#[inline(always)]
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn low_half(word: u32) -> u16 {
    word as u16
}

/// A sum in `f64` rounded to `float`, where the exe rounds as it stores.
#[inline(always)]
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn round_to_f32(value: f64) -> f32 {
    value as f32
}

/// The functions whose differential case has no world yet that reaches their real paths (nv-rs#952):
/// the random world stops both sides alike on a null field or loops on a count drawn from a pointer.
/// They are typed but not registered; each leaves this list when its case runs and passes.
pub(crate) const UNVERIFIED: &[u32] = &[
    0x0043_02a0,
    0x0043_02d0,
    0x0043_0410,
    0x0043_05f0,
    0x0043_0660,
    0x0043_0780,
    0x0043_09e0,
    0x0043_0a10,
    0x0043_0ae0,
    0x0043_0b40,
    0x0043_0bc0,
    0x0043_0ce0,
    0x0043_10d0,
    0x0043_1120,
    0x0043_1260,
    0x0043_23b0,
    0x0043_2440,
    0x0043_26c0,
    0x0043_2900,
    0x0043_30b0,
    0x0043_30e0,
    0x0043_31c0,
    0x0043_36f0,
    0x0043_3750,
    0x0043_3800,
    0x0043_3830,
    0x0043_3940,
    0x0043_3970,
    0x0043_3a00,
    0x0043_3b70,
    0x0043_3d10,
    0x0043_3d40,
    0x0043_3db0,
    0x0043_3ed0,
    0x0043_4050,
    0x0043_42b0,
    0x0043_43c0,
    0x0043_4410,
    0x0043_4480,
    0x0043_4560,
    0x0043_46f0,
    0x0043_4800,
    0x0043_48a0,
    0x0043_4a70,
    0x0043_4af0,
    0x0043_4c80,
    0x0043_4d90,
    0x0043_4ee0,
    0x0043_50c0,
    0x0043_51d0,
    0x0043_5a90,
    0x0043_5ac0,
    0x0043_5b80,
    0x0043_5bb0,
    0x0043_5c40,
    0x0043_5d40,
    0x0043_5dd0,
    0x0043_5e00,
    0x0043_5e20,
    0x0043_5e40,
    0x0043_5fa0,
    0x0043_6780,
    0x0043_6b20,
    0x0043_6e10,
    0x0043_6f20,
    0x0043_6f80,
    0x0043_7010,
    0x0043_7080,
    0x0043_70f0,
    0x0043_74e0,
    0x0043_75c0,
    0x0043_7750,
    0x0043_77c0,
    0x0043_77f0,
    0x0043_7850,
    0x0043_7a30,
    0x0043_7ab0,
    0x0043_7c40,
    0x0043_7ce0,
    0x0043_7d10,
    0x0043_7f90,
    0x0043_8170,
    0x0043_81b0,
    0x0043_81d0,
    0x0043_83e0,
    0x0043_8470,
    0x0043_8610,
    0x0043_87b0,
    0x0043_8820,
    0x0043_8940,
];

/// The node functions that are registered: every one of the three files' entries that passes its case.
pub(crate) fn registered() -> Vec<(u32, engine_core::AbiFn)> {
    let mut all = objects::entries();
    all.extend(objects_p2::entries());
    all.extend(objects_p3::entries());
    all.retain(|(addr, _)| !UNVERIFIED.contains(addr));
    all
}
