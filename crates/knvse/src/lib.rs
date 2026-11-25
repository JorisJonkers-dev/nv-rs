// SPDX-License-Identifier: GPL-3.0-only
//! kNVSE built in (ADR-0016, "later built-ins"; nv-rs#865): an original implementation for nv-rs,
//! written from the plugin's public behaviour and documentation only. No code of the plugin is read
//! or copied; see `NOTICE` for the sources and the licence check.
//!
//! What is here so far:
//! - [`store`]: the animation-path overrides, a stack per actor (or every actor), person and
//!   animation group;
//! - [`seam`]: the handler at the seam in `Animation::StartGroup` (`004948c0`) that starts an
//!   override's sequence in place of the one the game's sequence map holds, made once per
//!   animation and model and dropped when the animation's controller manager is made again or
//!   torn down (the seams in `0048ffd0` and `0048fb50`);
//! - [`commands`]: `SetActorAnimationPath`, which fills the store;
//! - [`identity`]: the plugin as xNVSE's plugin list holds it.
//!
//! The other commands the owner's mods call follow, one step at a time (`provenance/`). The plugin
//! answers "installed" only when its commands are ([`ANSWERS_INSTALLED`], or the switch
//! [`set_answers_installed`] gives). Off, [`install`] registers nothing, so the game runs as it
//! did without this crate.

pub mod commands;
pub mod identity;
pub mod seam;
pub mod store;

use engine_core::prelude::*;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, Ordering};

/// Whether kNVSE answers "installed". Off until the commands the owner's mods use are built; the one
/// place to turn it on.
pub const ANSWERS_INSTALLED: bool = false;

/// `KFModel` release of one manual reference (`0043bac0`, as `~Animation` runs it on its models).
pub(crate) const KF_MODEL_RELEASE: u32 = 0x0043_bac0;
/// `KFModel` add of one manual reference (`0043be10`, as `ModelLoader::LoadKF` takes it).
pub(crate) const KF_MODEL_ADD_REF: u32 = 0x0043_be10;

/// A sequence the seam made for one animation from one model (`seam`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cached {
    /// The animation's controller manager the sequence was added to.
    pub manager: u32,
    /// The `BSAnimGroupSequence`.
    pub sequence: u32,
}

/// kNVSE built in on this engine: its state, one slot per engine ([`Engine::ext`]), shared by the
/// real threads' engines once installed.
#[derive(Default)]
pub struct State {
    /// [`install`] ran.
    pub installed: bool,
    /// The switch of [`set_answers_installed`], when it was given.
    pub answers: Option<bool>,
    /// The animation-path overrides.
    pub store: store::Store,
    /// The sequences the seam made, by (animation, model). Each holds a manual reference on its
    /// model; the controller manager holds the sequence.
    pub cache: BTreeMap<(u32, u32), Cached>,
}

/// The counts the seams read without the shared slot's lock ([`Engine::ext_free`]): the first
/// test on every group start and every manager rebuild.
#[derive(Default)]
pub struct Active {
    overrides: AtomicU32,
    cached: AtomicU32,
}

impl Active {
    /// The count of overrides in the store.
    pub fn count(&self) -> u32 {
        self.overrides.load(Ordering::Relaxed)
    }

    /// The count of sequences the seam keeps.
    pub fn cached(&self) -> u32 {
        self.cached.load(Ordering::Relaxed)
    }
}

/// Sets whether [`install`] builds the plugin on this engine (before [`install`]); without it
/// [`ANSWERS_INSTALLED`] decides.
pub fn set_answers_installed(e: &mut Engine, on: bool) {
    e.ext::<State>().answers = Some(on);
}

/// Builds the plugin into the engine, after JohnnyGuitar NVSE, when it answers "installed"
/// ([`set_answers_installed`], else [`ANSWERS_INSTALLED`]). Once per engine; a second call does
/// nothing.
pub fn install(e: &mut Engine) {
    let on = e.ext::<State>().answers.unwrap_or(ANSWERS_INSTALLED);
    install_as(e, on);
}

/// [`install`] with the flag given (the tests check both answers).
pub fn install_as(e: &mut Engine, answers_installed: bool) {
    if e.ext::<State>().installed {
        return;
    }
    e.ext::<State>().installed = true;
    // Made before the threads start, as a lock-free slot must be.
    e.ext_free::<Active>();
    if answers_installed {
        e.on_site(seam::START_GROUP, seam::start_group);
        e.on_site(seam::MANAGER_MADE, seam::forget_animation);
        e.on_site(seam::TORN_DOWN, seam::forget_animation);
        commands::install(e);
        nvse::plugins::register(e, identity::plugin());
    }
    // The game's state, not one engine's: real threads' engines share it.
    e.share_ext::<State>();
}

/// Puts `entry` on top of `key`'s stack ([`store::Store::enable`]); what the entry it replaced
/// held is given back.
pub fn enable(e: &mut Engine, key: store::Key, entry: store::Override) {
    let (old, count) = {
        let store = &mut e.ext::<State>().store;
        let old = store.enable(key, entry);
        (old, store.len())
    };
    set_count(e, count);
    release(e, old);
}

/// Takes `owner`'s entry for `path` off `key`'s stack ([`store::Store::disable`]); whether there
/// was one.
pub fn disable(e: &mut Engine, key: store::Key, owner: u32, path: &[u8]) -> bool {
    let (removed, count) = {
        let store = &mut e.ext::<State>().store;
        let removed = store.disable(key, owner, path);
        (removed, store.len())
    };
    set_count(e, count);
    let was = removed.is_some();
    release(e, removed);
    was
}

/// Takes `owner`'s entries for `path` off every stack of `target` and person
/// ([`store::Store::disable_path`]); how many there were.
pub fn disable_path(
    e: &mut Engine,
    target: store::Target,
    first_person: bool,
    owner: u32,
    path: &[u8],
) -> usize {
    let (removed, count) = {
        let store = &mut e.ext::<State>().store;
        let removed = store.disable_path(target, first_person, owner, path);
        (removed, store.len())
    };
    set_count(e, count);
    let n = removed.len();
    release(e, removed);
    n
}

/// Empties the store.
pub fn clear(e: &mut Engine) {
    let removed = e.ext::<State>().store.clear();
    set_count(e, 0);
    release(e, removed);
}

/// Gives back what entries the store no longer holds took: the model's manual reference, the
/// condition lambda's saved variables.
fn release(e: &mut Engine, entries: impl IntoIterator<Item = store::Override>) {
    for o in entries {
        if o.kf != 0 {
            e.call(KF_MODEL_RELEASE, &args![o.kf]);
        }
        if o.condition != 0 {
            unsave_condition(e, o.condition);
        }
    }
}

/// The condition lambda's variables kept while the store holds it (xNVSE's lambda rules).
#[cfg(not(test))]
pub(crate) fn save_condition(e: &mut Engine, script: u32) {
    nvse::lambda::save_variables(e, script);
}

#[cfg(not(test))]
fn unsave_condition(e: &mut Engine, script: u32) {
    nvse::lambda::unsave_variables(e, script);
}

// The unit tests count the saves and unsaves instead (no script runs in them).
#[cfg(test)]
thread_local! {
    pub(crate) static SAVED: std::cell::RefCell<Vec<(u32, bool)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
pub(crate) fn save_condition(_: &mut Engine, script: u32) {
    SAVED.with(|s| s.borrow_mut().push((script, true)));
}

#[cfg(test)]
fn unsave_condition(_: &mut Engine, script: u32) {
    SAVED.with(|s| s.borrow_mut().push((script, false)));
}

fn set_count(e: &mut Engine, count: usize) {
    let count = u32::try_from(count).unwrap_or(u32::MAX);
    e.ext_free::<Active>()
        .overrides
        .store(count, Ordering::Relaxed);
}

pub(crate) fn set_cached(e: &mut Engine, count: usize) {
    let count = u32::try_from(count).unwrap_or(u32::MAX);
    e.ext_free::<Active>()
        .cached
        .store(count, Ordering::Relaxed);
}

#[cfg(test)]
mod test_support;

// The unit tests' own program: no other crate's functions are linked in.
#[cfg(test)]
engine_core::assemble!();
