//! JohnnyGuitar NVSE's identity as xNVSE's plugin list holds it.
//!
//! Sources (public, no code read): the plugin's own README and release page name its DLL and
//! version; xNVSE's wiki-origin names (`crates/nvse/src/docs.rs`, from xNVSE's documented
//! behaviour) name the plugin as `JohnnyGuitarNVSE`. The version number the plugin hands xNVSE is
//! **not verified** from a public document; see `NOTICE`. Until it is, only the file name,
//! name and base are relied on and the version is the release's number as an integer.

use nvse::plugins::BuiltIn;

/// The DLL's file name (xNVSE loads plugins in the order of their file names).
pub const DLL: &str = "johnnyguitar.dll";
/// `PluginInfo::name`: what `IsPluginInstalled` and `GetPluginVersion` are asked for.
pub const NAME: &str = "JohnnyGuitarNVSE";
/// The release this targets, as the owner's list names it after the update.
pub const RELEASE: &str = "5.25a";
/// `PluginInfo::version`.
pub const VERSION_INT: u32 = 525;
/// The first opcode of its block (`SetOpcodeBase`), also the header of its co-save section.
pub const OPCODE_BASE: u32 = 0x3100;

/// The plugin as the plugin list holds it, with its co-save carried and nothing else yet.
pub fn plugin() -> BuiltIn {
    BuiltIn {
        file: DLL,
        name: NAME,
        version: VERSION_INT,
        opcode_base: OPCODE_BASE,
        listed: true,
        listener: None,
        save: Some(crate::cosave::save),
        load: Some(crate::cosave::load),
        pre_load: None,
        new_game: Some(crate::cosave::new_game),
    }
}

#[cfg(test)]
#[path = "identity_tests.rs"]
mod tests;
