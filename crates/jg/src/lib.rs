// SPDX-License-Identifier: GPL-3.0-only
//! JohnnyGuitar NVSE built in (ADR-0016, "later built-ins"): an original implementation for nv-rs, written
//! from the plugin's public behaviour and documentation only. No code of the plugin, and no
//! decompiled code, is read or copied; see `NOTICE` for the sources and the licence check.
//!
//! What is here so far:
//! - [`identity`]: the plugin as xNVSE's plugin list knows it (name, DLL file name, version,
//!   first opcode `0x3100`), which is what `IsPluginInstalled`, `GetPluginVersion` and
//!   `GetPluginInfoByName` read;
//! - [`cosave`]: the plugin's co-save section: the extra statistics in a record of nv-rs's own,
//!   every other record carried from a load to the next save, record by record, until it is
//!   interpreted (the row `NV_COSAVE_KEEP` keeps a plugin's section as bytes).
//! - [`commands`]: the five commands Tale of Two Wastelands 3.4 calls (`IsDLLLoaded`,
//!   `InitExtraMiscStat`, `ModExtraMiscStat`, `GetExtraMiscStat`, `GetQuestFailed`), at the
//!   opcodes its compiled scripts carry. The extra statistics live in the engine from a new game
//!   or a load until the next one and are saved in the co-save ([`cosave`]); they are NOT shown in
//!   the Stats menu. [`math`]: `Clamp`, `Lerp` and `Sign`, written from the plugin's wiki pages
//!   (`provenance/01-math.md`); [`lookups`]: `EditorIDToFormID` and `GetEditorID`
//!   (`provenance/02-lookups.md`). Every other command of the plugin is not built.
//!
//! The plugin answers "installed" only together with those commands ([`install_as`]): a mod that
//! checks for it and needs another command then fails at that line. Off ([`ANSWERS_INSTALLED`],
//! the default, or [`set_answers_installed`]) [`install`] registers nothing, so a game, a save
//! and a co-save are as they were without this crate.

pub mod commands;
pub mod cosave;
pub mod identity;
pub mod lookups;
pub mod math;
#[cfg(test)]
mod test_support;

use engine_core::prelude::*;

/// Whether JohnnyGuitar NVSE answers "installed" unless [`set_answers_installed`] says otherwise.
/// Off: the plugin has five of its commands only.
pub const ANSWERS_INSTALLED: bool = false;

/// JohnnyGuitar NVSE built in on this engine: its state, one slot per engine ([`Engine::ext`]).
#[derive(Default)]
pub struct State {
    /// [`install`] ran.
    pub installed: bool,
    /// The records of the co-save section, carried ([`cosave`]).
    pub carried: Vec<nvse::cosave::Chunk>,
    /// [`set_answers_installed`]; `None` takes [`ANSWERS_INSTALLED`].
    pub answers_installed: Option<bool>,
    /// The extra miscellaneous statistics by name (exact bytes): saved and loaded with the game
    /// ([`cosave`]), empty after a new game and after a load of a save without them.
    pub stats: std::collections::BTreeMap<Vec<u8>, i32>,
}

/// The run's switch: the plugin answers "installed" (`on`), or not. Read by [`install`], so set
/// it before; after [`install`] it changes nothing.
pub fn set_answers_installed(e: &mut Engine, on: bool) {
    e.ext::<State>().answers_installed = Some(on);
}

/// The long names of the commands this crate builds, for the capabilities report.
pub const COMMAND_NAMES: [&str; 10] = [
    "GetQuestFailed",
    "IsDLLLoaded",
    "GetExtraMiscStat",
    "ModExtraMiscStat",
    "InitExtraMiscStat",
    "Clamp",
    "Lerp",
    "Sign",
    "EditorIDToFormID",
    "GetEditorID",
];

/// Builds the plugin into the engine, after xNVSE, when it answers "installed"
/// ([`ANSWERS_INSTALLED`]). Once per engine; a second call does nothing.
pub fn install(e: &mut Engine) {
    let on = e
        .ext::<State>()
        .answers_installed
        .unwrap_or(ANSWERS_INSTALLED);
    install_as(e, on);
}

/// Puts JohnnyGuitar NVSE's record in xNVSE's list. The editor ids' crate (`johnnyguitar`, in the
/// fixes preset) registers the same plugin first (same name, same opcode base) with its deferred-start
/// listener; a second registration panicked ("registered twice", nv-rs#482) for a game with Tale of Two
/// Wastelands in the fixes preset. It is one plugin, so there is one record carrying both halves: the
/// first one's listener, this one's co-save, listed.
fn register_plugin(e: &mut engine_core::Engine) {
    let mine = identity::plugin();
    let list = &mut e.ext::<nvse::Nvse>().plugins.built_ins;
    match list.iter_mut().find(|p| p.opcode_base == mine.opcode_base) {
        Some(first) => {
            first.listed |= mine.listed;
            first.listener = first.listener.or(mine.listener);
            first.save = first.save.or(mine.save);
            first.load = first.load.or(mine.load);
            first.pre_load = first.pre_load.or(mine.pre_load);
            first.new_game = first.new_game.or(mine.new_game);
        }
        None => nvse::plugins::register(e, mine),
    }
}

/// [`install`] with the flag given (the tests check both answers).
pub fn install_as(e: &mut Engine, answers_installed: bool) {
    if e.ext::<State>().installed {
        return;
    }
    e.ext::<State>().installed = true;
    if answers_installed {
        commands::install(e);
        register_plugin(e);
    }
    // The game's state, not one engine's: real threads' engines share it.
    e.share_ext::<State>();
}

// The unit tests' own program: no other crate's functions are linked in.
#[cfg(test)]
engine_core::assemble!();
