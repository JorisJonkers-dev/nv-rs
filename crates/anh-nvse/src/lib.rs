// SPDX-License-Identifier: GPL-3.0-only
//! AnhNVSE built in: two of the plugin's script commands, the ones the owner's mods call, as an
//! original implementation for nv-rs written from public documentation (`NOTICE`). No code of
//! the plugin is read or copied.
//!
//! - `GetQuestCompletedAlt quest` (`0x3604`): 1 when the quest is completed and not failed
//!   ([`commands`]);
//! - `Sv_PadStart text length pad` (`0x361b`): the text padded from its start to the length.
//!
//! The opcodes are the ones the mods' compiled scripts carry (the plugin's block starts at
//! [`OPCODE_BASE`]). The plugin's other commands, their number and its version are not known, so
//! it does not answer "installed": `IsPluginInstalled "AnhNVSE"` stays 0 (the
//! extender-installed guard, nv-rs#273), and a script that calls one of its other commands fails
//! at that line, as without the plugin. [`install`] registers the two commands; a second call
//! does nothing.

pub mod commands;

use engine_core::prelude::*;

/// `PluginInfo::name`: what `IsPluginInstalled` is asked for (the mods ask for "AnhNVSE").
pub const NAME: &str = "AnhNVSE";
/// The DLL's file name, as the plugin ships it.
pub const FILE: &str = "AnhNVSE.dll";
/// The first opcode of the plugin's block.
pub const OPCODE_BASE: u32 = 0x3600;
/// The commands built, in the order of their opcodes.
pub const COMMAND_NAMES: [&str; 2] = ["GetQuestCompletedAlt", "Sv_PadStart"];
/// The block's slots up to the highest opcode the owner's scripts call: at least this many
/// commands (the real number is not known).
pub const KNOWN_SLOTS: usize = (commands::SV_PAD_START - OPCODE_BASE + 1) as usize;

/// The plugin on this engine.
#[derive(Default)]
pub struct State {
    /// [`install`] ran.
    pub installed: bool,
}

/// Registers the commands at their opcodes, after xNVSE. Once per engine.
pub fn install(e: &mut Engine) {
    if e.ext::<State>().installed {
        return;
    }
    e.ext::<State>().installed = true;
    commands::install(e);
}

// The unit tests' own program: no other crate's functions are linked in.
#[cfg(test)]
engine_core::assemble!();
