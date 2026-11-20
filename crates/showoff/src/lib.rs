//! ShowOff xNVSE built in (ADR-0016, "later built-ins"): an original implementation for nv-rs, written
//! from the plugin's public behaviour and documentation only. No code of the plugin, and no
//! decompiled code, is read or copied; see `NOTICE` for the sources and the licence check.
//!
//! What is here so far:
//! - [`identity`]: the plugin as xNVSE's plugin list knows it (name, DLL file name, version,
//!   first opcode `0x3c93`), which is what `IsPluginInstalled`, `GetPluginVersion` and
//!   `GetPluginInfoByName` read;
//! - [`cosave`]: the plugin's co-save section carried from a load to the next save, record by
//!   record, until its records are interpreted (the row `NV_COSAVE_KEEP` keeps it as bytes
//!   today).
//!
//! - [`commands`]: `ActorHasBaseFlag`, the one command Tale of Two Wastelands' scripts call.
//!   Every other command of the plugin is not built: a script that calls one fails at that line.
//!
//! The plugin answers "installed" only when its commands are ([`ANSWERS_INSTALLED`], or the
//! switch [`set_answers_installed`] gives): a mod that checks for it then stops with its own
//! "requires" message, as without the DLL, instead of halfway through a script. Off, [`install`]
//! registers nothing, so a game, a save and a co-save are as they were without this crate.

pub mod commands;
pub mod cosave;
pub mod identity;

use engine_core::prelude::*;

/// Whether ShowOff xNVSE answers "installed". Off until the commands the owner's mods use are built; the
/// one place to turn it on.
pub const ANSWERS_INSTALLED: bool = false;

/// The command names this crate builds (long names), for the capabilities file.
pub const COMMAND_NAMES: [&str; 1] = commands::COMMAND_NAMES;

/// ShowOff xNVSE built in on this engine: its state, one slot per engine ([`Engine::ext`]).
#[derive(Default)]
pub struct State {
    /// [`install`] ran.
    pub installed: bool,
    /// The switch of [`set_answers_installed`], when it was given.
    pub answers: Option<bool>,
    /// The records of the co-save section, carried ([`cosave`]).
    pub carried: Vec<nvse::cosave::Chunk>,
}

/// Sets whether [`install`] builds the plugin on this engine (before [`install`]); without it
/// [`ANSWERS_INSTALLED`] decides.
pub fn set_answers_installed(e: &mut Engine, on: bool) {
    e.ext::<State>().answers = Some(on);
}

/// Builds the plugin into the engine, after xNVSE, when it answers "installed"
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
