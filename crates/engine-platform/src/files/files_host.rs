//! The host path of a game path (fork-only file: the fork's Linux file-path code lives here so that
//! upstream's `files.rs` stays as it is; see docs/fork/LINUX.md, change 5).
//!
//! Windows: the path as it is. Unix: `Z:` is the host's root (as Wine's is), `\` separates, and
//! names match without regard to case in their directory.

/// The host's path for the game's full path `path` (Windows: as it is; elsewhere: `unix_path`).
pub fn host_path(path: &[u8]) -> std::path::PathBuf {
    let ansi = super::real_path(path);
    #[cfg(unix)]
    {
        unix_path(&ansi.to_string_lossy())
    }
    #[cfg(not(unix))]
    {
        ansi
    }
}

/// A game path (`Z:\home\me\Data\Fallout.esm`; names are not case-sensitive, the separator is a
/// backslash) as a path of a Unix host: `Z:` is the host's root, as Wine's is, so a name under it
/// is the same name with `/` for `\`; a name without a drive letter keeps its place (the
/// current directory's or an absolute one). Any other drive letter has no mapping: its name stays
/// as it is and is not found, never made up. Each part that does not exist as written is looked up
/// without regard to case in its directory (the first entry that matches), so `data\FALLOUT.ESM`
/// finds `Data/Fallout.esm`; a part with no match stays as written (a file to be made).
#[cfg(unix)]
fn unix_path(text: &str) -> std::path::PathBuf {
    use std::path::{Component, Path, PathBuf};
    let slashed = text.replace('\\', "/");
    let rest = match slashed.as_bytes() {
        [d, b':', ..] if d.eq_ignore_ascii_case(&b'z') => &slashed[2..],
        _ => &slashed[..],
    };
    let mut found = PathBuf::new();
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

/// What `host_path` gives on every host (Windows runs this one too).
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ascii_path_keeps_its_letters() {
        let path = host_path(b"Data\\Fallout.esm");
        let text = path.to_string_lossy().to_lowercase();
        assert!(text.ends_with("fallout.esm"), "{text}");
    }
}

/// The game's paths on a Unix host ([`host_path`]).
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
    fn the_z_drive_is_the_hosts_root_and_backslashes_are_separators() {
        assert_eq!(
            host_path(b"Z:\\usr\\share\\none"),
            std::path::PathBuf::from("/usr/share/none")
        );
        assert_eq!(host_path(b"z:\\tmp"), std::path::PathBuf::from("/tmp"));
    }

    #[test]
    fn another_drive_has_no_mapping_and_stays_as_it_is() {
        let path = host_path(b"C:\\Games\\Data");
        assert_eq!(path, std::path::PathBuf::from("C:/Games/Data"));
        assert!(!path.exists());
    }

    #[test]
    fn a_name_is_found_without_regard_to_case() {
        let dir = scratch("case");
        let asked = format!("Z:{}\\DATA\\fallout.ESM", dir.display());
        let found = host_path(asked.as_bytes());
        assert_eq!(found, dir.join("Data").join("Fallout.esm"));
        assert!(found.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_name_with_no_match_stays_as_written() {
        let dir = scratch("nomatch");
        let asked = format!("Z:{}\\Data\\New.fos", dir.display());
        assert_eq!(
            host_path(asked.as_bytes()),
            dir.join("Data").join("New.fos")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
