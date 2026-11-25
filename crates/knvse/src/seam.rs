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
//! - Every frame ([`poll`], xNVSE's `MainGameLoop`): for each group that started with a polled
//!   override among its candidates and still plays, the choice is made again; when it differs
//!   from the sequence playing (the condition began or stopped holding), the group is started
//!   again in its slot, so the seam's new answer, or the game's own sequence, takes over.

use crate::{Cached, State};
use engine_core::extend::{Owner, SiteSlot};
use engine_core::prelude::*;
use nvse::plugins::{msg, Message};

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
/// `Animation +0x4c`: the group playing in each of the 8 slots (`u16[8]`); `+0xe0`: the sequence
/// playing in each (`BSAnimGroupSequence*[8]`).
const ANIMATION_GROUPS: u32 = 0x4c;
const ANIMATION_CURRENT_SEQUENCES: u32 = 0xe0;
const SLOTS: u32 = 8;
/// `Animation::StartGroup(group, slot)`, the function the seam is in.
const ANIMATION_START_GROUP: u32 = 0x0049_48c0;

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
/// A polled override among them, the group is watched while it plays ([`poll`]).
pub fn start_group(e: &mut Engine, words: &[u32]) -> Option<u32> {
    if e.ext_free::<crate::Active>().count() == 0 {
        return None;
    }
    let (animation, group) = (words[0], words[1] as u16);
    let (answer, polled) = choose(e, animation, group);
    if polled {
        let count = {
            let watched = &mut e.ext::<State>().watched;
            watched.insert((animation, group));
            watched.len()
        };
        crate::set_watched(e, count);
    }
    answer
}

/// The sequence the overrides answer now for `animation`'s `group` (the first candidate whose
/// condition holds and that has a sequence), and whether a candidate polls its condition.
fn choose(e: &mut Engine, animation: u32, group: u16) -> (Option<u32>, bool) {
    let actor = e.mem.u32(animation + ANIMATION_ACTOR);
    if actor == 0 {
        return (None, false);
    }
    let form_id = e.mem.u32(actor + FORM_ID);
    let player: u32 = e.global(PLAYER_SINGLETON);
    let first_person = player != 0
        && actor == player
        && e.call(PLAYER_GET_ANIMATION, &args![player, 1u32]).u32() == animation;
    let candidates: Vec<(u32, u32, bool)> = e
        .ext::<State>()
        .store
        .candidates(form_id, first_person, group)
        .filter(|o| o.kf != 0)
        .map(|o| (o.kf, o.condition, o.poll && o.condition != 0))
        .collect();
    let polled = candidates.iter().any(|c| c.2);
    for (kf, condition, _) in candidates {
        if condition != 0 && !condition_holds(e, condition, actor) {
            continue;
        }
        if let Some(sequence) = sequence_for(e, animation, kf) {
            return (Some(sequence), polled);
        }
    }
    (None, polled)
}

/// Each watched group, once a frame: dropped when it no longer plays in any slot (or no longer
/// has a polled candidate), else started again in its slot when the overrides' answer is not the
/// sequence playing: a sequence they answer that does not play, or one of theirs playing that
/// they no longer answer.
pub fn poll(e: &mut Engine) {
    if e.ext_free::<crate::Active>().watched() == 0 {
        return;
    }
    let watched: Vec<(u32, u16)> = e.ext::<State>().watched.iter().copied().collect();
    let mut gone = Vec::new();
    for (animation, group) in watched {
        let slot = (0..SLOTS).find(|i| e.mem.u16(animation + ANIMATION_GROUPS + 2 * i) == group);
        let Some(slot) = slot else {
            gone.push((animation, group));
            continue;
        };
        let playing = e
            .mem
            .u32(animation + ANIMATION_CURRENT_SEQUENCES + 4 * slot);
        let (wanted, polled) = if e.ext_free::<crate::Active>().count() == 0 {
            (None, false)
        } else {
            choose(e, animation, group)
        };
        let ours = playing != 0
            && e.ext::<State>()
                .cache
                .iter()
                .any(|(&(a, _), c)| a == animation && c.sequence == playing);
        let restart = match wanted {
            Some(sequence) => sequence != playing,
            None => ours,
        };
        if !polled {
            gone.push((animation, group));
        }
        if restart {
            e.call(
                ANIMATION_START_GROUP,
                &args![animation, u32::from(group), slot],
            );
        }
    }
    unwatch(e, |k| gone.contains(k));
}

/// Stops watching the groups `which` picks.
fn unwatch(e: &mut Engine, which: impl Fn(&(u32, u16)) -> bool) {
    let count = {
        let watched = &mut e.ext::<State>().watched;
        watched.retain(|k| !which(k));
        watched.len()
    };
    crate::set_watched(e, count);
}

/// The plugin's listener: [`poll`] every frame; a load or the main menu ends every watch (the
/// animations are torn down with their actors).
pub fn listen(e: &mut Engine, m: &Message) {
    match m.kind {
        msg::MAIN_GAME_LOOP => poll(e),
        msg::PRE_LOAD_GAME | msg::EXIT_TO_MAIN_MENU
            if e.ext_free::<crate::Active>().watched() != 0 =>
        {
            unwatch(e, |_| true);
        }
        _ => {}
    }
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
            // A sequence this manager refused is not made again (sequence 0).
            return (c.sequence != 0).then_some(c.sequence);
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
    // Kept with the model it was made from: the cache holds a manual reference until it forgets.
    // A refused sequence is kept as 0, so this manager is not handed a new block at every start.
    let sequence = if added { sequence } else { 0 };
    e.call(crate::KF_MODEL_ADD_REF, &args![kf]);
    let count = {
        let cache = &mut e.ext::<State>().cache;
        cache.insert((animation, kf), Cached { manager, sequence });
        cache.len()
    };
    crate::set_cached(e, count);
    (sequence != 0).then_some(sequence)
}

/// The handler of the manager-made and torn-down seams: words `[animation]`. Forgets the
/// sequences kept for that animation and stops watching its groups; answers nothing.
pub fn forget_animation(e: &mut Engine, words: &[u32]) -> Option<u32> {
    let animation = words[0];
    if e.ext_free::<crate::Active>().watched() != 0 {
        unwatch(e, |&(a, _)| a == animation);
    }
    if e.ext_free::<crate::Active>().cached() == 0 {
        return None;
    }
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
