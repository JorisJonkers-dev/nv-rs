// SPDX-License-Identifier: GPL-3.0-only
//! Fork-only. The plugins Tale of Two Wastelands' start-up check asks for besides JIP LN
//! (nv-rs#399; JIP LN is a fixes row, `fixes::Choice::with_ttw_detected`).
//!
//! TTW 3.4's start-up script reads `GetPluginVersion` of each plugin it needs and shows a
//! "missing dependency" box for each that is not listed. A plugin of [`PLUGINS`] answers
//! "installed" in a run when the data folder holds TTW's master (`TaleOfTwoWastelands.esm`), with
//! exactly the commands of it that TTW's compiled scripts call built (docs/FIXES.md, "Tale of Two
//! Wastelands' plugins", says what each has and has not). Without TTW each stays as it was: not
//! listed, so a mod that checks for it stops with its own message.
//!
//! lStewieAl's Tweaks is the one plugin here with no command built: TTW's scripts call none of
//! its eight (no opcode of its block `0x2132` to `0x2139` is in a compiled script of TTW 3.4's
//! plugin files), TTW asks for it for its engine fixes and settings. It answers only when its row
//! (`NV_STEWIE`) is on in the run, with the fixes built so far.
//!
//! [`ENV`] (`NV_TTW_PLUGINS`) decides for all of them when it is set: `0` none answers (TTW shows
//! its boxes again), `1` they answer whatever the data folder holds (for a test).

use engine_core::prelude::*;

/// The variable that decides for every plugin of [`PLUGINS`]: `0` or `1`; unset, or any other
/// value, leaves it to the data folder.
pub const ENV: &str = "NV_TTW_PLUGINS";

/// A plugin the detection answers for: the name TTW asks xNVSE for, and its crate's switch.
pub struct Plugin {
    pub name: &'static str,
    pub switch: fn(&mut Engine, bool),
}

/// The plugins switched on, in xNVSE's order of blocks.
pub const PLUGINS: &[Plugin] = &[
    Plugin {
        name: showoff::identity::NAME,
        switch: showoff::set_answers_installed,
    },
    Plugin {
        name: stewie::NAME,
        switch: stewie::set_answers_installed,
    },
];

/// Whether the plugins answer "installed": the variable's `0` or `1` when it says so, else
/// whether the data folder holds TTW's master.
pub fn wanted(var: Option<&str>, master: bool) -> bool {
    match var.map(str::trim) {
        Some("0") => false,
        Some("1") => true,
        _ => master,
    }
}

/// Switches the plugins on for this run when [`wanted`] says so (before
/// `install_rows_and_extenders`, which installs them) and returns the line to log.
pub fn switch_on(e: &mut Engine, data: &std::path::Path) -> Option<String> {
    let var = std::env::var(ENV).ok();
    let master = crate::fixes::holds_ttw_master(data);
    apply(e, var.as_deref(), master)
}

/// [`switch_on`] with the variable and the folder's answer given.
pub fn apply(e: &mut Engine, var: Option<&str>, master: bool) -> Option<String> {
    if !wanted(var, master) {
        return None;
    }
    for plugin in PLUGINS {
        (plugin.switch)(e, true);
    }
    let names: Vec<&str> = PLUGINS.iter().map(|p| p.name).collect();
    let why = if master {
        "the data folder holds TaleOfTwoWastelands.esm, which asks for them"
    } else {
        "NV_TTW_PLUGINS=1"
    };
    Some(format!(
        "{}: answer \"installed\" with the commands Tale of Two Wastelands calls, because {why} ({ENV}=0 turns them off)",
        names.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_variable_wins_and_the_master_decides_without_it() {
        for (var, master, on) in [
            (None, false, false),
            (None, true, true),
            (Some("0"), true, false),
            (Some(" 0 "), true, false),
            (Some("1"), false, true),
            (Some("yes"), false, false),
            (Some("yes"), true, true),
            (Some(""), true, true),
        ] {
            assert_eq!(wanted(var, master), on, "{var:?} {master}");
        }
    }

    #[test]
    fn nothing_is_switched_without_the_master() {
        let mut e = Engine::new();
        assert_eq!(apply(&mut e, None, false), None);
        assert_eq!(apply(&mut e, Some("0"), true), None);
        assert_eq!(e.ext::<showoff::State>().answers, None);
        assert!(!e.ext::<stewie::Stewie>().answers_installed);
    }

    #[test]
    fn the_master_switches_every_plugin_on_and_the_line_names_them() {
        let mut e = Engine::new();
        let line = apply(&mut e, None, true).expect("a line to log");
        assert_eq!(e.ext::<showoff::State>().answers, Some(true));
        assert!(e.ext::<stewie::Stewie>().answers_installed);
        for plugin in PLUGINS {
            assert!(line.contains(plugin.name), "{line}");
        }
        assert!(line.contains("TaleOfTwoWastelands.esm"), "{line}");
        assert!(line.contains(ENV), "{line}");
        let forced = apply(&mut Engine::new(), Some("1"), false).expect("a line to log");
        assert!(forced.contains("NV_TTW_PLUGINS=1"), "{forced}");
    }

    /// The plugin's own install then lists it under the name and version TTW reads
    /// (`GetPluginVersion "ShowOffNVSE Plugin"` at least 180) with its command registered; the
    /// crates' tests cover the commands themselves.
    #[test]
    fn show_off_is_the_name_and_version_ttw_reads() {
        assert_eq!(showoff::identity::NAME, "ShowOffNVSE Plugin");
        assert!(showoff::identity::VERSION_INT >= 180);
    }

    /// `GetPluginVersion "lStewieAl's Tweaks"` at least 941.
    #[test]
    fn tweaks_is_the_name_and_version_ttw_reads() {
        assert_eq!(stewie::NAME, "lStewieAl's Tweaks");
        assert!(stewie::VERSION >= 941);
    }
}
