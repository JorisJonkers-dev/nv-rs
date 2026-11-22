// SPDX-License-Identifier: GPL-3.0-only
//! Plugin DLLs of the mods, loaded when `NV_PLUGIN_DLLS` is on (off by default; ADR-0018): the
//! `.dll` files of `Data\NVSE\Plugins`, read through the mod-files seam as the built-ins read their
//! settings, in xNVSE's order (by file name without case). A DLL for a plugin the engine builds in
//! is not loaded; a DLL for which nothing is built in is.

use engine_core::prelude::*;

pub const ENV: &str = "NV_PLUGIN_DLLS";
const FOLDER: &[u8] = b"Data\\NVSE\\Plugins";

/// DLLs whose plugin the engine has built in: not loaded (the built-in answers), by file name
/// without case. The profile's own copies of xNVSE's plugins are in it.
pub const BUILT_IN: &[&str] = &[
    "jip_nvse.dll",
    "johnnyguitar.dll",
    "showoffnvse.dll",
    "nvse_stewie_tweaks.dll",
    "ttw_nvse.dll",
    "mcm.dll",
    "ui_organizer.dll",
];

pub fn built_in(file: &str) -> bool {
    BUILT_IN.contains(&file.to_ascii_lowercase().as_str())
}

/// Switches the loader on when the variable says so and loads the folder's DLLs. Returns the lines
/// for the log.
pub fn load_all(e: &mut Engine, var: Option<&str>) -> Vec<String> {
    if !plugin_host::wanted(var) {
        return Vec::new();
    }
    plugin_host::set_enabled(e, true);
    let mut files: Vec<String> = engine_core::mod_files::list(e, FOLDER)
        .into_iter()
        .filter(|f| !f.is_folder)
        .map(|f| String::from_utf8_lossy(&f.name).into_owned())
        .filter(|n| n.to_ascii_lowercase().ends_with(".dll"))
        .collect();
    files.sort_by_key(|n| n.to_ascii_lowercase());
    let mut lines = vec![format!(
        "plugin DLLs: on ({ENV}), {} in Data\\NVSE\\Plugins",
        files.len()
    )];
    for file in files {
        if built_in(&file) {
            lines.push(format!(
                "plugin DLLs: {file}: not loaded, the engine has it built in"
            ));
            continue;
        }
        let mut path = FOLDER.to_vec();
        path.push(b'\\');
        path.extend_from_slice(file.as_bytes());
        let Some(bytes) = engine_core::mod_files::read(e, &path) else {
            lines.push(format!("plugin DLLs: {file}: cannot be read"));
            continue;
        };
        match plugin_host::load(e, &file, &bytes) {
            Ok(i) => {
                let p = &plugin_host::plugins_loaded(e)[i];
                lines.push(format!(
                    "plugin DLLs: {file}: loaded as \"{}\" version {}, {} command(s)",
                    p.name,
                    p.version,
                    p.commands.len()
                ));
            }
            Err(err) => lines.push(format!("plugin DLLs: {file}: not loaded: {err:?}")),
        }
    }
    lines.extend(
        plugin_host::reports(e)
            .into_iter()
            .map(|l| format!("plugin DLLs: {l}")),
    );
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_engines_own_extenders_are_not_loaded_without_case() {
        assert!(built_in("JIP_NVSE.dll"));
        assert!(built_in("MCM.dll"));
        assert!(!built_in("kNVSE.dll"));
    }

    #[test]
    fn on_with_an_empty_folder_says_so_and_loads_nothing() {
        let mut e = Engine::new();
        let lines = load_all(&mut e, Some("1"));
        assert!(plugin_host::enabled(&mut e));
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains("0 in Data"), "{lines:?}");
        assert!(plugin_host::plugins_loaded(&mut e).is_empty());
    }

    #[test]
    fn off_without_the_variable() {
        let mut e = Engine::new();
        assert!(load_all(&mut e, None).is_empty());
        assert!(load_all(&mut e, Some("0")).is_empty());
        assert!(!plugin_host::enabled(&mut e));
    }
}
