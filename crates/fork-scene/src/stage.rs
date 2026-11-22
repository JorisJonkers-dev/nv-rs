// SPDX-License-Identifier: GPL-3.0-only
//! A game folder for one variant of the game's data, made of symbolic links: the installation's
//! files are never copied or written. A variant is the game's own data, or the game's data with an
//! overlay folder over its `Data` (loose files: `meshes`, `textures`, `lodsettings`, a plugin and
//! its archive). The user data (`xdg`) is a fresh folder per variant, so a run never touches the
//! player's saves or settings.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// What [`stage`] made.
#[derive(Debug, PartialEq)]
pub struct Staged {
    /// The game folder to give the viewer.
    pub game: PathBuf,
    /// The `XDG_*` folders to run it with: data, config, cache, state.
    pub xdg: [PathBuf; 4],
}

/// A link to `src` at `dst`: a symbolic link; on Windows, where that needs a privilege, a copy of a file
/// when the link is refused (a folder is then an error: run with the privilege).
fn symlink(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(src, dst)
    }
    #[cfg(windows)]
    {
        let (src, dst) = (src.as_ref(), dst.as_ref());
        if src.is_dir() {
            std::os::windows::fs::symlink_dir(src, dst)
        } else {
            std::os::windows::fs::symlink_file(src, dst).or_else(|_| fs::copy(src, dst).map(|_| ()))
        }
    }
}

fn link_entries(from: &Path, into: &Path, skip: &dyn Fn(&str) -> bool) -> io::Result<()> {
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        if skip(&name.to_string_lossy()) {
            continue;
        }
        symlink(entry.path(), into.join(&name))?;
    }
    Ok(())
}

/// Links `overlay`'s tree over `target`, a real folder: a file replaces what is there; a folder both
/// sides have is merged (made real, filled with links to what was there, then the overlay's).
pub fn merge_over(overlay: &Path, target: &Path, skip: &[String]) -> io::Result<()> {
    for entry in fs::read_dir(overlay)? {
        let entry = entry?;
        let name = entry.file_name();
        if skip
            .iter()
            .any(|s| s.eq_ignore_ascii_case(&name.to_string_lossy()))
        {
            continue;
        }
        let dest = target.join(&name);
        let is_dir = entry.file_type()?.is_dir() || entry.path().is_dir();
        match fs::symlink_metadata(&dest) {
            Err(_) => symlink(entry.path(), &dest)?,
            Ok(_) if is_dir && dest.is_dir() => {
                // Make the linked folder a real one holding links to its entries, then recurse.
                let old = fs::canonicalize(&dest)?;
                fs::remove_file(&dest).or_else(|_| fs::remove_dir(&dest))?;
                fs::create_dir(&dest)?;
                link_entries(&old, &dest, &|_| false)?;
                merge_over(&entry.path(), &dest, &[])?;
            }
            Ok(_) => {
                fs::remove_file(&dest)?;
                symlink(entry.path(), &dest)?;
            }
        }
    }
    Ok(())
}

/// Makes `<dir>/game` (links to `install`, its `Data` merged with `overlay`) and `<dir>/xdg`
/// (the user data: `userdata_template`'s `Documents` copied in when given, a plugin list naming
/// `plugins` when any). `overlay_skip` names top-level entries of the overlay left out. `dir` must not exist yet.
pub fn stage(
    install: &Path,
    overlay: Option<&Path>,
    overlay_skip: &[String],
    plugins: &[String],
    userdata_template: Option<&Path>,
    dir: &Path,
) -> io::Result<Staged> {
    if dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} exists", dir.display()),
        ));
    }
    let game = dir.join("game");
    fs::create_dir_all(game.join("Data"))?;
    let skip = |n: &str| {
        n == "Data"
            || n.ends_with(".log")
            || matches!(n, "ConsoleHistory.txt" | "ExitData.mhd" | "Crash Logs")
    };
    link_entries(install, &game, &skip)?;
    link_entries(&install.join("Data"), &game.join("Data"), &|_| false)?;
    if let Some(overlay) = overlay {
        merge_over(overlay, &game.join("Data"), overlay_skip)?;
    }
    let xdg = ["data", "config", "cache", "state"].map(|n| dir.join("xdg").join(n));
    for x in &xdg {
        fs::create_dir_all(x)?;
    }
    let user = xdg[0].join("nv-rs").join("userdata");
    let local = user.join("AppData").join("Local").join("FalloutNV");
    fs::create_dir_all(&local)?;
    let list: String = plugins.iter().map(|p| format!("{p}\n")).collect();
    fs::write(local.join("plugins.txt"), list)?;
    if let Some(t) = userdata_template {
        copy_tree(&t.join("Documents"), &user.join("Documents"))?;
    }
    Ok(Staged { game, xdg })
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &dest)?;
        } else {
            fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

// Linking folders on Windows needs a privilege a CI runner may not have: the tests run where links are free.
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("fork-scene-stage-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(p: &Path, text: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    #[test]
    fn the_installation_is_linked_not_copied_and_its_logs_are_left_out() {
        let t = tmp("link");
        let install = t.join("install");
        write(&install.join("FalloutNV.exe"), "exe");
        write(&install.join("Data/FalloutNV.esm"), "esm");
        write(&install.join("Data/lodsettings/a.dlodsettings"), "orig");
        write(&install.join("falloutnv.log"), "log");
        let s = stage(&install, None, &[], &[], None, &t.join("v")).unwrap();
        assert_eq!(
            fs::read_to_string(s.game.join("FalloutNV.exe")).unwrap(),
            "exe"
        );
        assert_eq!(
            fs::read_to_string(s.game.join("Data/FalloutNV.esm")).unwrap(),
            "esm"
        );
        assert!(fs::symlink_metadata(s.game.join("Data/FalloutNV.esm"))
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(!s.game.join("falloutnv.log").exists());
        assert_eq!(
            fs::read_to_string(s.game.join("Data/lodsettings/a.dlodsettings")).unwrap(),
            "orig"
        );
        fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn an_overlay_adds_files_and_wins_where_it_meets_the_installations_own() {
        let t = tmp("overlay");
        let install = t.join("install");
        write(&install.join("Data/lodsettings/a.dlodsettings"), "orig");
        write(&install.join("Data/lodsettings/keep.txt"), "keep");
        write(&install.join("Data/FalloutNV.esm"), "esm");
        let overlay = t.join("overlay");
        write(&overlay.join("lodsettings/a.dlodsettings"), "new");
        write(&overlay.join("meshes/landscape/lod/x.nif"), "nif");
        let s = stage(
            &install,
            Some(&overlay),
            &[],
            &["Lod.esp".into()],
            None,
            &t.join("v"),
        )
        .unwrap();
        let d = s.game.join("Data");
        assert_eq!(
            fs::read_to_string(d.join("lodsettings/a.dlodsettings")).unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_to_string(d.join("lodsettings/keep.txt")).unwrap(),
            "keep"
        );
        assert_eq!(
            fs::read_to_string(d.join("meshes/landscape/lod/x.nif")).unwrap(),
            "nif"
        );
        // The installation's file is untouched.
        assert_eq!(
            fs::read_to_string(install.join("Data/lodsettings/a.dlodsettings")).unwrap(),
            "orig"
        );
        let list = s.xdg[0].join("nv-rs/userdata/AppData/Local/FalloutNV/plugins.txt");
        assert_eq!(fs::read_to_string(list).unwrap(), "Lod.esp\n");
        fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn skipped_overlay_entries_are_left_out() {
        let t = tmp("skip");
        let install = t.join("install");
        write(&install.join("Data/FalloutNV.esm"), "esm");
        let overlay = t.join("overlay");
        write(&overlay.join("meshes/a.nif"), "nif");
        write(&overlay.join("Extra.esm"), "x");
        let s = stage(
            &install,
            Some(&overlay),
            &["Meshes".into()],
            &[],
            None,
            &t.join("v"),
        )
        .unwrap();
        assert!(!s.game.join("Data/meshes").exists());
        assert_eq!(
            fs::read_to_string(s.game.join("Data/Extra.esm")).unwrap(),
            "x"
        );
        fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn the_user_data_template_is_copied_and_an_existing_folder_is_refused() {
        let t = tmp("user");
        let install = t.join("install");
        write(&install.join("Data/FalloutNV.esm"), "esm");
        let template = t.join("tpl");
        write(&template.join("Documents/My Games/x.ini"), "ini");
        let v = t.join("v");
        let s = stage(&install, None, &[], &[], Some(&template), &v).unwrap();
        assert_eq!(
            fs::read_to_string(s.xdg[0].join("nv-rs/userdata/Documents/My Games/x.ini")).unwrap(),
            "ini"
        );
        assert!(stage(&install, None, &[], &[], None, &v).is_err());
        fs::remove_dir_all(&t).unwrap();
    }
}
