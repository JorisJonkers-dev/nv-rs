// SPDX-License-Identifier: GPL-3.0-only
//! kNVSE's identity as xNVSE's plugin list holds it.
//!
//! Sources (public interface facts, no code read): the plugin's DLL name, the name and version its
//! query hands xNVSE (`PluginInfo::name` "kNVSE", `::version` 40 at the pinned release) and the
//! opcode base it asks for (`SetOpcodeBase(0x3920)`). See `NOTICE`.

use nvse::plugins::BuiltIn;

/// The DLL's file name (xNVSE loads plugins in the order of their file names).
pub const DLL: &str = "kNVSE.dll";
/// `PluginInfo::name`: what `IsPluginInstalled` and `GetPluginVersion` are asked for.
pub const NAME: &str = "kNVSE";
/// `PluginInfo::version` at the pinned release.
pub const VERSION_INT: u32 = 40;
/// The first opcode of its block (`SetOpcodeBase`).
pub const OPCODE_BASE: u32 = 0x3920;

/// The plugin as the plugin list holds it: no co-save, no listener.
pub fn plugin() -> BuiltIn {
    BuiltIn {
        file: DLL,
        name: NAME,
        version: VERSION_INT,
        opcode_base: OPCODE_BASE,
        listed: true,
        listener: None,
        save: None,
        load: None,
        pre_load: None,
        new_game: None,
    }
}

#[cfg(test)]
#[path = "identity_tests.rs"]
mod tests;
