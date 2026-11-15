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
//! No command is built yet, and the plugin answers "installed" only when its commands are
//! ([`ANSWERS_INSTALLED`]): a mod that checks for it then stops with its own "requires" message,
//! as without the DLL, instead of halfway through a script. Until then [`install`] registers
//! nothing, so a game, a save and a co-save are as they were without this crate.

pub mod cosave;
pub mod identity;

use engine_core::prelude::*;

/// Whether ShowOff xNVSE answers "installed". Off until the commands the owner's mods use are built; the
/// one place to turn it on.
pub const ANSWERS_INSTALLED: bool = false;

/// ShowOff xNVSE built in on this engine: its state, one slot per engine ([`Engine::ext`]).
#[derive(Default)]
pub struct State {
    /// [`install`] ran.
    pub installed: bool,
    /// The records of the co-save section, carried ([`cosave`]).
    pub carried: Vec<nvse::cosave::Chunk>,
}

/// Builds the plugin into the engine, after xNVSE, when it answers "installed"
/// ([`ANSWERS_INSTALLED`]). Once per engine; a second call does nothing.
pub fn install(e: &mut Engine) {
    install_as(e, ANSWERS_INSTALLED);
}

/// [`install`] with the flag given (the tests check both answers).
pub fn install_as(e: &mut Engine, answers_installed: bool) {
    if e.ext::<State>().installed {
        return;
    }
    e.ext::<State>().installed = true;
    if answers_installed {
        nvse::plugins::register(e, identity::plugin());
    }
    // The game's state, not one engine's: real threads' engines share it.
    e.share_ext::<State>();
}

// The unit tests' own program: no other crate's functions are linked in.
#[cfg(test)]
engine_core::assemble!();
