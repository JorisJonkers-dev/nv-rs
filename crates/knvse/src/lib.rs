// SPDX-License-Identifier: GPL-3.0-only
//! kNVSE built in (ADR-0016, "later built-ins"; nv-rs#865): an original implementation for nv-rs,
//! written from the plugin's public behaviour and documentation only. No code of the plugin is read
//! or copied; see `NOTICE` for the sources and the licence check.
//!
//! What is here so far:
//! - [`store`]: the animation-path overrides, a stack per actor (or every actor), person and
//!   animation group;
//! - [`seam`]: the handler at the seam in `Animation::StartGroup` (`004948c0`) that starts an
//!   override's sequence in place of the one the game's sequence map holds.
//!
//! The commands that fill the store (`SetActorAnimationPath` first) follow, one step at a time
//! (`provenance/`). The plugin answers "installed" only when its commands are
//! ([`ANSWERS_INSTALLED`], or the switch [`set_answers_installed`] gives). Off, [`install`]
//! registers nothing, so the game runs as it did without this crate.

pub mod seam;
pub mod store;

use engine_core::prelude::*;
use std::sync::atomic::{AtomicU32, Ordering};

/// Whether kNVSE answers "installed". Off until the commands the owner's mods use are built; the one
/// place to turn it on.
pub const ANSWERS_INSTALLED: bool = false;

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
}

/// How many overrides the store holds, read without the shared slot's lock ([`Engine::ext_free`]):
/// the seam's first test on every group start.
#[derive(Default)]
pub struct Active(AtomicU32);

impl Active {
    /// The count of overrides in the store.
    pub fn count(&self) -> u32 {
        self.0.load(Ordering::Relaxed)
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
    }
    // The game's state, not one engine's: real threads' engines share it.
    e.share_ext::<State>();
}

/// Puts `entry` on top of `key`'s stack ([`store::Store::enable`]).
pub fn enable(e: &mut Engine, key: store::Key, entry: store::Override) {
    let count = {
        let store = &mut e.ext::<State>().store;
        store.enable(key, entry);
        store.len()
    };
    set_count(e, count);
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
    removed
}

/// Empties the store (a load, a new game).
pub fn clear(e: &mut Engine) {
    e.ext::<State>().store.clear();
    set_count(e, 0);
}

fn set_count(e: &mut Engine, count: usize) {
    let count = u32::try_from(count).unwrap_or(u32::MAX);
    e.ext_free::<Active>().0.store(count, Ordering::Relaxed);
}

// The unit tests' own program: no other crate's functions are linked in.
#[cfg(test)]
engine_core::assemble!();
