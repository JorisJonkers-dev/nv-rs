//! The host path of a name relative to a host folder (fork-only file; see docs/fork/LINUX.md, change 5).
//! Upstream's own `host_paths.rs` maps the game's full paths (drive `Z:`, names found without case);
//! a relative name it keeps as written. The mod files folder keeps a mod's names relative to a folder
//! of this system, so off Windows the fork finds each part of such a name in its folder without
//! regard to case, as Windows finds it.

/// The host's path for `rel`, a name relative to the host folder `folder` as the game's ANSI calls
/// carry it (the mod files folder keeps a mod's names as the game passes them): Windows: the folder
/// joined with the name as upstream's `files::host_path` gives it; elsewhere each part of the name is
/// found in its directory without regard to case, starting in `folder` ([`unix_path_in`]).
pub fn host_path_in(folder: &std::path::Path, rel: &[u8]) -> std::path::PathBuf {
    #[cfg(unix)]
    {
        unix_path_in(folder, &super::game_text(rel))
    }
    #[cfg(not(unix))]
    {
        folder.join(super::host_path(rel))
    }
}

/// `text`, a name as the game writes it (a `\` separates, an optional leading `Z:` is the host's
/// root), relative to the host folder `base`: each part that does not exist as written is looked
/// up without regard to case in its directory (the first entry that matches); a part with no match
/// stays as written (a file to be made).
#[cfg(unix)]
fn unix_path_in(base: &std::path::Path, text: &str) -> std::path::PathBuf {
    use std::path::{Component, Path};
    let slashed = text.replace('\\', "/");
    let rest = match slashed.as_bytes() {
        [d, b':', ..] if d.eq_ignore_ascii_case(&b'z') => &slashed[2..],
        _ => &slashed[..],
    };
    let mut found = base.to_path_buf();
    for part in Path::new(rest).components() {
        let Component::Normal(name) = part else {
            found.push(part.as_os_str());
            continue;
        };
        let next = found.join(name);
        if next.symlink_metadata().is_ok() {
            found = next;
            continue;
        }
        let from = if found.as_os_str().is_empty() {
            Path::new(".")
        } else {
            found.as_path()
        };
        let same = std::fs::read_dir(from).ok().and_then(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name())
                .find(|n| {
                    n.to_string_lossy()
                        .eq_ignore_ascii_case(&name.to_string_lossy())
                })
        });
        found.push(same.as_deref().unwrap_or(name));
    }
    found
}

/// The names relative to a host folder on a Unix host ([`host_path_in`]).
#[cfg(all(test, unix))]
mod tests_unix_paths {
    use super::*;

    fn scratch(test: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("nv-rs-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Data")).unwrap();
        std::fs::write(dir.join("Data").join("Fallout.esm"), b"x").unwrap();
        dir
    }

    #[test]
    fn a_name_in_a_folder_is_found_without_regard_to_case_from_that_folder() {
        let dir = scratch("in");
        assert_eq!(
            host_path_in(&dir, b"DATA\\fallout.ESM"),
            dir.join("Data").join("Fallout.esm")
        );
        assert_eq!(
            host_path_in(&dir, b"Config\\New.ini"),
            dir.join("Config").join("New.ini")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
