// SPDX-License-Identifier: GPL-3.0-only
//! The script extender plugin Tale of Two Wastelands ships (its readers call it ROOGNVSE; xNVSE
//! knows it as `ttw_nvse`) built in: an original implementation for nv-rs, written from public
//! documentation and from how TTW's scripts call its commands. No code of the plugin is read or
//! copied; see `NOTICE`.
//!
//! TTW's start-up script asks `IsPluginInstalled ttw_nvse` and, when it is not listed, shows its
//! "plugin is missing" box. The plugin's commands it calls are:
//! - `TTW_GetPlugins` (`0x3806`, returns an array): built ([`commands`]), the loaded plugins as
//!   `[name, version]` pairs;
//! - `TTW_EnableRadioFix` (`0x3808`, one integer): built as far as storing the value
//!   ([`radio_fix`]). The radio fix itself is NOT built: the value changes nothing in the engine;
//! - `TTW_ShowGeneProjector` (`0x3800`): NOT built. It opens the gene projector character menu.
//!   Its record name is registered with no handler, so the script error log names it and a call
//!   fails (the script stops for that run).
//!
//! Identity: name `ttw_nvse`, version 1 (unknown, a guess), DLL file name `ttw_nvse.dll` (unknown,
//! the name plus `.dll`), opcode base `0x3800`, listed when installed. The host's switch
//! ([`set_answers_installed`]) decides whether it answers "installed"; off, [`install`]
//! registers nothing.

pub mod commands;

use engine_core::prelude::*;

/// `PluginInfo::name`: what `IsPluginInstalled` is asked for (exact).
pub const NAME: &str = "ttw_nvse";
/// `PluginInfo::version`. Unknown: not stated in a public document.
pub const VERSION: u32 = 1;
/// The DLL's file name. Unknown: the name plus `.dll`.
pub const FILE: &str = "ttw_nvse.dll";
/// `SetOpcodeBase`.
pub const OPCODE_BASE: u32 = 0x3800;
/// The commands built, by name.
pub const COMMAND_NAMES: [&str; 2] = ["TTW_GetPlugins", "TTW_EnableRadioFix"];
/// The commands not built (record name only, no handler).
pub const UNBUILT_COMMAND_NAMES: [&str; 1] = ["TTW_ShowGeneProjector"];

/// The plugin on this engine: one slot per engine ([`Engine::ext`]).
#[derive(Default)]
pub struct State {
    /// The host's switch ([`set_answers_installed`]).
    pub on: bool,
    /// [`install`] ran.
    pub installed: bool,
    /// The last value `TTW_EnableRadioFix` was given (nothing acts on it).
    pub radio_fix: bool,
}

/// The host's switch: the plugin answers "installed" (before [`install`]).
pub fn set_answers_installed(e: &mut Engine, on: bool) {
    e.ext::<State>().on = on;
}

/// What `TTW_EnableRadioFix` last stored (false before any call). The fix itself is not built.
pub fn radio_fix(e: &mut Engine) -> bool {
    e.ext::<State>().radio_fix
}

/// xNVSE's record of the plugin.
fn plugin() -> nvse::plugins::BuiltIn {
    nvse::plugins::BuiltIn {
        file: FILE,
        name: NAME,
        version: VERSION,
        opcode_base: OPCODE_BASE,
        listed: true,
        listener: None,
        save: None,
        load: None,
        pre_load: None,
        new_game: None,
    }
}

/// Builds the plugin into the engine, after xNVSE, when the switch is on: its commands at their
/// opcodes and its place in xNVSE's list. Once per engine; off, or a second call, registers
/// nothing.
pub fn install(e: &mut Engine) {
    let state = e.ext::<State>();
    if state.installed || !state.on {
        return;
    }
    state.installed = true;
    commands::install(e);
    nvse::plugins::register(e, plugin());
    // The game's state, not one engine's: real threads' engines share it.
    e.share_ext::<State>();
}

// The unit tests' own program: no other crate's functions are linked in.
#[cfg(test)]
engine_core::assemble!();

#[cfg(test)]
#[path = "commands_tests.rs"]
mod commands_tests;
