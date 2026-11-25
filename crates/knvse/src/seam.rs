// SPDX-License-Identifier: GPL-3.0-only
//! The handler at the seam in `Animation::StartGroup` (`004948c0`, nv-rs#865): asked with the
//! animation, the group and the slot before the game's sequence map is, it answers the sequence of
//! the override for that actor, person and group, or nothing (the game's own path).

use engine_core::extend::{Owner, SiteSlot};
use engine_core::prelude::*;

/// The seam in `Animation::StartGroup`.
pub const START_GROUP: SiteSlot = SiteSlot::new(Owner::Knvse, 0x0049_48c0);

/// `PlayerCharacter *g_thePlayer`.
const PLAYER_SINGLETON: u32 = 0x011d_ea3c;
/// `PlayerCharacter::GetAnimation(firstPerson)`.
const PLAYER_GET_ANIMATION: u32 = 0x0095_0a60;
/// `Animation +0x4`: the actor it animates.
const ANIMATION_ACTOR: u32 = 0x4;
/// `TESForm +0xC`: the form id.
const FORM_ID: u32 = 0xc;

/// The seam's handler: words `[animation, group, slot]`. `None` while the store is empty, for an
/// animation of no actor, and when no override answers or its sequence is not loaded.
pub fn start_group(e: &mut Engine, words: &[u32]) -> Option<u32> {
    if e.ext_free::<crate::Active>().count() == 0 {
        return None;
    }
    let (animation, group) = (words[0], words[1] as u16);
    let actor = e.mem.u32(animation + ANIMATION_ACTOR);
    if actor == 0 {
        return None;
    }
    let form_id = e.mem.u32(actor + FORM_ID);
    let player: u32 = e.global(PLAYER_SINGLETON);
    let first_person = player != 0
        && actor == player
        && e.call(PLAYER_GET_ANIMATION, &args![player, 1u32]).u32() == animation;
    e.ext::<crate::State>()
        .store
        .lookup(form_id, first_person, group)
        .map(|o| o.sequence)
        .filter(|&s| s != 0)
}

#[cfg(test)]
#[path = "seam_tests.rs"]
mod tests;
