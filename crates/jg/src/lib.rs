// SPDX-License-Identifier: GPL-3.0-only
//! JohnnyGuitar NVSE built in (ADR-0016, "later built-ins"): an original implementation for nv-rs, written
//! from the plugin's public behaviour and documentation only. No code of the plugin, and no
//! decompiled code, is read or copied; see `NOTICE` for the sources and the licence check.
//!
//! What is here so far:
//! - [`identity`]: the plugin as xNVSE's plugin list knows it (name, DLL file name, version,
//!   first opcode `0x3100`), which is what `IsPluginInstalled`, `GetPluginVersion` and
//!   `GetPluginInfoByName` read;
//! - [`cosave`]: the plugin's co-save section carried from a load to the next save, record by
//!   record, until its records are interpreted (the row `NV_COSAVE_KEEP` keeps it as bytes
//!   today).
//! - [`commands`]: the five commands Tale of Two Wastelands 3.4 calls (`IsDLLLoaded`,
//!   `InitExtraMiscStat`, `ModExtraMiscStat`, `GetExtraMiscStat`, `GetQuestFailed`), at the
//!   opcodes its compiled scripts carry. The extra statistics live in the engine from a new game
//!   or a load until the next one: they are NOT saved in the co-save (nv-rs#405) and NOT shown in
//!   the Stats menu. Every other
//!   command of the plugin is not built.
//!
//! The plugin answers "installed" only together with those commands ([`install_as`]): a mod that
//! checks for it and needs another command then fails at that line. Off ([`ANSWERS_INSTALLED`],
//! the default, or [`set_answers_installed`]) [`install`] registers nothing, so a game, a save
//! and a co-save are as they were without this crate.

pub mod commands;
pub mod cosave;
pub mod identity;

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
    /// The extra miscellaneous statistics by name (exact bytes), from a new game or a load until
    /// the next one ([`cosave`] clears them).
    pub stats: std::collections::BTreeMap<Vec<u8>, i32>,
}

/// The run's switch: the plugin answers "installed" (`on`), or not. Read by [`install`], so set
/// it before; after [`install`] it changes nothing.
pub fn set_answers_installed(e: &mut Engine, on: bool) {
    e.ext::<State>().answers_installed = Some(on);
}

/// The long names of the commands this crate builds, for the capabilities report.
pub const COMMAND_NAMES: [&str; 5] = [
    "GetQuestFailed",
    "IsDLLLoaded",
    "GetExtraMiscStat",
    "ModExtraMiscStat",
    "InitExtraMiscStat",
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

/// [`install`] with the flag given (the tests check both answers).
pub fn install_as(e: &mut Engine, answers_installed: bool) {
    if e.ext::<State>().installed {
        return;
    }
    e.ext::<State>().installed = true;
    if answers_installed {
        commands::install(e);
        nvse::plugins::register(e, identity::plugin());
    }
    // The game's state, not one engine's: real threads' engines share it.
    e.share_ext::<State>();
}

// The unit tests' own program: no other crate's functions are linked in.
#[cfg(test)]
engine_core::assemble!();
