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

/// The node functions the cell references tests (`typed_cell_refs`, the `xd_`, `xe_`, `xp_`, `xq_` and
/// `extra_` cases) replace with counting doubles in their worlds: the constructors, destructors and
/// other callees of the other units' extra data functions. A differential case may not double a
/// function the family registers (the double would run on one side only), so these stay on their
/// translations until those worlds build real nodes (a follow-up in `docs/fork/EXTRA-DATA-MOVE.md`).
/// Their typed bodies keep their own cases in `typed_extra_nodes`.
pub(crate) const LEFT_ON_TRANSLATIONS: &[u32] = &[
    0x0043_08c0,
    0x0043_0a90,
    0x0043_0c70,
    0x0043_0ca0,
    0x0043_0f30,
    0x0043_10d0,
    0x0043_1230,
    0x0043_1260,
    0x0043_1360,
    0x0043_14c0,
    0x0043_1620,
    0x0043_1780,
    0x0043_1830,
    0x0043_1950,
    0x0043_1a40,
    0x0043_1ae0,
    0x0043_1b80,
    0x0043_1c20,
    0x0043_1d10,
    0x0043_1e20,
    0x0043_1ec0,
    0x0043_1f60,
    0x0043_2000,
    0x0043_2040,
    0x0043_2220,
    0x0043_22c0,
    0x0043_22f0,
    0x0043_2320,
    0x0043_2350,
    0x0043_25b0,
    0x0043_26c0,
    0x0043_27e0,
    0x0043_2870,
    0x0043_28d0,
    0x0043_2900,
    0x0043_2aa0,
    0x0043_2be0,
    0x0043_2c20,
    0x0043_2cb0,
    0x0043_2f30,
    0x0043_3010,
    0x0043_31c0,
    0x0043_3260,
    0x0043_3300,
    0x0043_33a0,
    0x0043_34b0,
    0x0043_3530,
    0x0043_3640,
    0x0043_36c0,
    0x0043_3790,
    0x0043_38b0,
    0x0043_3a00,
    0x0043_3ca0,
    0x0043_3db0,
    0x0043_4560,
    0x0043_4800,
    0x0043_48a0,
    0x0043_4950,
    0x0043_4a70,
    0x0043_4af0,
    0x0043_4dc0,
    0x0043_4ee0,
    0x0043_5100,
    0x0043_5150,
    0x0043_5820,
    0x0043_58a0,
    0x0043_5920,
    0x0043_5950,
    0x0043_5a60,
    0x0043_5b50,
    0x0043_5c10,
    0x0043_5c40,
    0x0043_5ec0,
    0x0043_5ef0,
    0x0043_5fa0,
    0x0043_60c0,
    0x0043_61e0,
    0x0043_6300,
    0x0043_6420,
    0x0043_6540,
    0x0043_6660,
    0x0043_6f80,
    0x0043_7240,
    0x0043_7730,
];

/// The registered entries of one of the node files, without [`LEFT_ON_TRANSLATIONS`].
pub(crate) fn registered(
    entries: Vec<(u32, engine_core::AbiFn)>,
) -> Vec<(u32, engine_core::AbiFn)> {
    entries
        .into_iter()
        .filter(|(addr, _)| !LEFT_ON_TRANSLATIONS.contains(addr))
        .collect()
}
