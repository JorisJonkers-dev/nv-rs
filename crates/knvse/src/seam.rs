// SPDX-License-Identifier: GPL-3.0-only
//! The handlers at kNVSE's seams in the animation code (nv-rs#865):
//!
//! - `Animation::StartGroup` (`004948c0`): asked with the animation, the group and the slot before
//!   the game's sequence map is, it answers the sequence of the override for that actor, person
//!   and group whose condition holds, or nothing (the game's own path). The sequence is made from
//!   the override's `KFModel` the way `Animation::AddAnimation` (`00490500`) makes one: a
//!   `BSAnimGroupSequence` of the model's group and sequence, added to the animation's controller
//!   manager under the accumulation root's name. A sequence belongs to one manager, so it is made
//!   once per animation and model and kept ([`crate::State::cache`]).
//! - The animation's controller manager made (`0048ffd0`, after it is stored) and the animation
//!   torn down (`0048fb50`, after the manager is let go): the sequences kept for that animation are
//!   forgotten and their models' references given back, so a rebuilt 3D or manager gets new ones.

use crate::{Cached, State};
use engine_core::extend::{Owner, SiteSlot};
use engine_core::prelude::*;

/// The seam in `Animation::StartGroup`.
pub const START_GROUP: SiteSlot = SiteSlot::new(Owner::Knvse, 0x0049_48c0);
/// The seam after the animation's controller manager is made (`0048ffd0`).
pub const MANAGER_MADE: SiteSlot = SiteSlot::new(Owner::Knvse, 0x0048_ffd0);
/// The seam after the animation let its controller manager go (`~Animation`, `0048fb50`).
pub const TORN_DOWN: SiteSlot = SiteSlot::new(Owner::Knvse, 0x0048_fb50);

/// `PlayerCharacter *g_thePlayer`.
const PLAYER_SINGLETON: u32 = 0x011d_ea3c;
/// `PlayerCharacter::GetAnimation(firstPerson)`.
const PLAYER_GET_ANIMATION: u32 = 0x0095_0a60;
/// `Animation +0x4`: the actor it animates.
const ANIMATION_ACTOR: u32 = 0x4;
/// `Animation +0xd8`: `NiPointer<NiControllerManager>`.
const ANIMATION_MANAGER: u32 = 0xd8;
/// `TESForm +0xC`: the form id.
const FORM_ID: u32 = 0xc;

/// `KFModel` getters: its sequence (+4) and its animation group (+8).
pub(crate) const KF_MODEL_SEQUENCE: u32 = 0x007f_a950;
pub(crate) const KF_MODEL_ANIM_GROUP: u32 = 0x0055_85e0;
/// `NiNew` of a block (`00aa13e0`), and the size of a `BSAnimGroupSequence`.
pub(crate) const NI_OPERATOR_NEW: u32 = 0x00aa_13e0;
const ANIM_GROUP_SEQUENCE_SIZE: u32 = 0x78;
/// `BSAnimGroupSequence::BSAnimGroupSequence(group, sequence)`.
pub(crate) const ANIM_GROUP_SEQUENCE_CONSTRUCT: u32 = 0x004e_e9f0;
/// `NiControllerManager::AddSequence(sequence, name, storeTargets)` and the name
/// `AddAnimation` passes.
pub(crate) const MANAGER_ADD_SEQUENCE: u32 = 0x00a2_f0c0;
pub(crate) const ACCUM_ROOT_NAME: u32 = 0x011f_49e0;

/// The `StartGroup` seam's handler: words `[animation, group, slot]`. `None` while the store is
/// empty, for an animation of no actor or without a controller manager, and when no override
/// answers (none for that actor, person and group, no condition holds, or no sequence could be
/// made).
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
    let candidates: Vec<(u32, u32)> = e
        .ext::<State>()
        .store
        .candidates(form_id, first_person, group)
        .filter(|o| o.kf != 0)
        .map(|o| (o.kf, o.condition))
        .collect();
    for (kf, condition) in candidates {
        if condition != 0 && !condition_holds(e, condition, actor) {
            continue;
        }
        if let Some(sequence) = sequence_for(e, animation, kf) {
            return Some(sequence);
        }
    }
    None
}

/// The sequence made from `kf` for `animation`: the one kept while the animation still has the
/// manager it was added to, else a new one (the stale one forgotten first).
fn sequence_for(e: &mut Engine, animation: u32, kf: u32) -> Option<u32> {
    let manager = e.mem.u32(animation + ANIMATION_MANAGER);
    if manager == 0 {
        return None;
    }
    let kept = e.ext::<State>().cache.get(&(animation, kf)).copied();
    if let Some(c) = kept {
        if c.manager == manager {
            return Some(c.sequence);
        }
        forget(e, |&(a, k)| a == animation && k == kf);
    }
    let source = e.call(KF_MODEL_SEQUENCE, &args![kf]).u32();
    let group = e.call(KF_MODEL_ANIM_GROUP, &args![kf]).u32();
    if source == 0 || group == 0 {
        return None;
    }
    let block = e
        .call(NI_OPERATOR_NEW, &args![ANIM_GROUP_SEQUENCE_SIZE])
        .u32();
    if block == 0 {
        return None;
    }
    let sequence = e
        .call(ANIM_GROUP_SEQUENCE_CONSTRUCT, &args![block, group, source])
        .u32();
    let added = e
        .call(
            MANAGER_ADD_SEQUENCE,
            &args![manager, sequence, ACCUM_ROOT_NAME, 1u32],
        )
        .bool();
    if !added {
        return None;
    }
    // Kept with the model it was made from: the cache holds a manual reference until it forgets.
    e.call(crate::KF_MODEL_ADD_REF, &args![kf]);
    let count = {
        let cache = &mut e.ext::<State>().cache;
        cache.insert((animation, kf), Cached { manager, sequence });
        cache.len()
    };
    crate::set_cached(e, count);
    Some(sequence)
}

/// The handler of the manager-made and torn-down seams: words `[animation]`. Forgets the
/// sequences kept for that animation; answers nothing.
pub fn forget_animation(e: &mut Engine, words: &[u32]) -> Option<u32> {
    if e.ext_free::<crate::Active>().cached() == 0 {
        return None;
    }
    let animation = words[0];
    forget(e, |&(a, _)| a == animation);
    None
}

/// Forgets the kept sequences whose (animation, model) `which` picks and gives their models'
/// references back. The sequences themselves are their managers'.
fn forget(e: &mut Engine, which: impl Fn(&(u32, u32)) -> bool) {
    let (gone, count) = {
        let cache = &mut e.ext::<State>().cache;
        let gone: Vec<(u32, u32)> = cache.keys().filter(|k| which(k)).copied().collect();
        for k in &gone {
            cache.remove(k);
        }
        (gone, cache.len())
    };
    crate::set_cached(e, count);
    for (_, kf) in gone {
        e.call(crate::KF_MODEL_RELEASE, &args![kf]);
    }
}

/// Whether the condition function returns true with the actor as its calling reference (xNVSE's
/// function call, `GetBool` of its result; no result is false).
#[cfg(not(test))]
fn condition_holds(e: &mut Engine, script: u32, actor: u32) -> bool {
    use nvse::expr::Tok;
    match nvse::functions::call(
        e,
        nvse::functions::Caller::internal(script, actor, 0, vec![]),
    ) {
        Some(Tok::Number { v, .. }) => v != 0.0,
        Some(Tok::Bool(b)) => b,
        Some(Tok::Form(id) | Tok::Array(id)) => id != 0,
        Some(Tok::Str(s)) => !s.is_empty(),
        _ => false,
    }
}

// The unit tests answer conditions from a table (no script runs in them): (script, actor) asked,
// and the scripts that hold.
#[cfg(test)]
pub(crate) type Conditions = (Vec<(u32, u32)>, Vec<u32>);

#[cfg(test)]
thread_local! {
    pub(crate) static CONDITIONS: std::cell::RefCell<Conditions> =
        const { std::cell::RefCell::new((Vec::new(), Vec::new())) };
}

#[cfg(test)]
fn condition_holds(_: &mut Engine, script: u32, actor: u32) -> bool {
    CONDITIONS.with(|c| {
        let mut c = c.borrow_mut();
        c.0.push((script, actor));
        c.1.contains(&script)
    })
}

#[cfg(test)]
#[path = "seam_tests.rs"]
mod tests;
