// SPDX-License-Identifier: GPL-3.0-only
//! Plugin DLLs of the mods, loaded when `NV_PLUGIN_DLLS` is on (off by default; ADR-0018): the
//! `.dll` files of `Data\NVSE\Plugins`, read through the mod-files seam as the built-ins read their
//! settings, in xNVSE's order (by file name without case). A DLL for a plugin the engine builds in
//! is not loaded; a DLL for which nothing is built in is.

use engine_core::prelude::*;

pub const ENV: &str = "NV_PLUGIN_DLLS";
/// A folder of the user's own copies of the Microsoft C++ runtime DLLs the plugins import
/// (`msvcp140.dll`): each `.dll` in it is mapped before the plugins, like any other DLL.
/// Nothing of Microsoft's is shipped with the engine.
pub const RUNTIME_ENV: &str = "NV_PLUGIN_RUNTIME";
/// The user's own unpacked `FalloutNV.exe` (1.4.0.525), whose code bytes a plugin that checks
/// what it patches then reads (the Steam executable's code is encrypted and is refused).
pub const CODE_ENV: &str = "NV_PLUGIN_CODE";
/// Where the verdicts of the plugin DLLs are written (`mod-support.json`, for raul).
pub const VERDICTS_ENV: &str = "NV_PLUGIN_VERDICTS";
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

/// The folder's DLL names, by file name without case.
fn dll_names(e: &Engine) -> Vec<String> {
    let mut files: Vec<String> = engine_core::mod_files::list(e, FOLDER)
        .into_iter()
        .filter(|f| !f.is_folder)
        .map(|f| String::from_utf8_lossy(&f.name).into_owned())
        .filter(|n| n.to_ascii_lowercase().ends_with(".dll"))
        .collect();
    files.sort_by_key(|n| n.to_ascii_lowercase());
    files
}

/// Writes the verdict of every plugin DLL of the folder (what the engine does with it and why)
/// to `path` as JSON, whether or not the loader is on: a plugin the engine builds in or has the
/// fix for needs no run. Returns the line for the log.
pub fn write_verdicts(e: &mut Engine, path: &str) -> String {
    let names = dll_names(e);
    let v = plugin_host::verdicts::verdicts(e, &names);
    match std::fs::write(path, plugin_host::verdicts::to_json(&v)) {
        Ok(()) => format!("plugin DLLs: {} verdict(s) written to {path}", v.len()),
        Err(err) => format!("plugin DLLs: verdicts not written to {path}: {err}"),
    }
}

/// Switches the loader on when the variable says so and loads the folder's DLLs. Returns the lines
/// for the log.
pub fn load_all(e: &mut Engine, var: Option<&str>) -> Vec<String> {
    load_with(e, var, std::env::var(RUNTIME_ENV).ok().as_deref())
}

/// The runtime DLLs of `dir`, mapped: one line for each.
pub fn load_runtime(e: &mut Engine, dir: &str) -> Vec<String> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return vec![format!("plugin DLLs: runtime folder {dir} cannot be read")];
    };
    let mut files: Vec<_> = read
        .flatten()
        .filter(|f| {
            f.file_name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .ends_with(".dll")
        })
        .collect();
    files.sort_by_key(|f| f.file_name().to_ascii_lowercase());
    let mut lines = Vec::new();
    for f in files {
        let name = f.file_name().to_string_lossy().into_owned();
        match std::fs::read(f.path()) {
            Ok(bytes) => match plugin_host::supply_runtime(e, &name, &bytes) {
                Ok(()) => lines.push(format!("plugin DLLs: runtime {name}: mapped")),
                Err(err) => lines.push(format!("plugin DLLs: runtime {name}: not mapped: {err:?}")),
            },
            Err(err) => lines.push(format!(
                "plugin DLLs: runtime {name}: cannot be read: {err}"
            )),
        }
    }
    lines
}

/// [`load_all`] with the runtime folder given.
pub fn load_with(e: &mut Engine, var: Option<&str>, runtime: Option<&str>) -> Vec<String> {
    if !plugin_host::wanted(var) {
        return Vec::new();
    }
    plugin_host::set_enabled(e, true);
    let files = dll_names(e);
    let mut lines = vec![format!(
        "plugin DLLs: on ({ENV}), {} in Data\\NVSE\\Plugins",
        files.len()
    )];
    if let Some(dir) = runtime.filter(|d| !d.is_empty()) {
        lines.extend(load_runtime(e, dir));
    }
    if let Some(path) = std::env::var(CODE_ENV).ok().filter(|p| !p.is_empty()) {
        lines.push(match std::fs::read(&path) {
            Ok(b) => match plugin_host::supply_code_from_exe(e, &b) {
                Ok(n) => format!("plugin DLLs: the game's code supplied from {path}: {n} bytes"),
                Err(why) => format!("plugin DLLs: the game's code not supplied from {path}: {why}"),
            },
            Err(err) => format!("plugin DLLs: {path} cannot be read: {err}"),
        });
    }
    let mut todo: Vec<(String, Vec<u8>)> = Vec::new();
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
        match engine_core::mod_files::read(e, &path) {
            Some(bytes) => todo.push((file, bytes)),
            None => lines.push(format!("plugin DLLs: {file}: cannot be read")),
        }
    }
    // As xNVSE does: every DLL mapped, then each queried, then each loaded.
    let results = plugin_host::load_many(e, &todo);
    for ((file, _), r) in todo.iter().zip(results) {
        match r {
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
    fn a_runtime_folder_is_mapped_before_the_plugins() {
        let dir = std::env::temp_dir().join(format!("nv-plugin-rt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.txt"), b"not a dll").unwrap();
        std::fs::write(dir.join("broken.dll"), b"not a dll").unwrap();
        let mut e = Engine::new();
        let lines = load_with(&mut e, Some("1"), Some(dir.to_str().unwrap()));
        assert!(
            lines
                .iter()
                .any(|l| l.contains("runtime broken.dll: not mapped")),
            "{lines:?}"
        );
        assert!(!lines.iter().any(|l| l.contains("notes.txt")), "{lines:?}");
        let gone = load_runtime(&mut e, dir.join("nowhere").to_str().unwrap());
        assert!(gone[0].contains("cannot be read"), "{gone:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_verdicts_are_written_with_the_loader_off() {
        let path =
            std::env::temp_dir().join(format!("nv-plugin-verdicts-{}.json", std::process::id()));
        let mut e = Engine::new();
        let line = write_verdicts(&mut e, path.to_str().unwrap());
        assert!(line.contains("0 verdict(s) written"), "{line}");
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("\"plugins\""));
        std::fs::remove_file(&path).unwrap();
        let bad = write_verdicts(&mut e, "/nonexistent-folder/x.json");
        assert!(bad.contains("not written"), "{bad}");
    }

    #[test]
    fn off_without_the_variable() {
        let mut e = Engine::new();
        assert!(load_all(&mut e, None).is_empty());
        assert!(load_all(&mut e, Some("0")).is_empty());
        assert!(!plugin_host::enabled(&mut e));
    }
}
