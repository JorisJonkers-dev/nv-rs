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
