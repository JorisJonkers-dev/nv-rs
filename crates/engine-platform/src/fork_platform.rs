//! The platform's answers off Windows (fork-owned; `docs/fork/LINUX.md`): what the game asks the
//! machine that Windows answers and a Unix machine answers differently.
//!
//! Every function here is built on every target so its tests run everywhere. The folders are the
//! ones the upstream-owned files ask for ([`data_dir`], whose Windows arm is the `%LOCALAPPDATA%`
//! lookup the call sites made before, and [`known_folders`]); nothing on Windows takes another
//! path than it took.
//!
//! - File times: [`filetime`] turns a Unix time into a `FILETIME`; [`find_fields`] gives the
//!   attributes and the three times of a `WIN32_FIND_DATAA` from the file's own metadata. No
//!   upstream file calls them since upstream's `files.rs` gives the times itself
//!   (slaterain/nv-rs#633); they stay for the fork's own tests and tools. The
//!   birth time is the file system's (`statx` with `STATX_BTIME`, as the standard library asks
//!   for it); on a file system that keeps none, [`creation`] answers the older of the last write
//!   and the last status change, the oldest time the file system recorded.
//! - Folders: [`data_dir`] is where the tables go (`$XDG_DATA_HOME/nv-rs`), and [`userdata_dir`]
//!   is the player's Documents, AppData and Local AppData as the game sees them.

use std::path::{Path, PathBuf};

/// Seconds from 1601-01-01 (the `FILETIME` epoch) to 1970-01-01.
const EPOCH_GAP_SECS: i64 = 11_644_473_600;

/// A Unix time (seconds and nanoseconds since 1970, the nanoseconds in `0..1_000_000_000`) as a
/// `FILETIME`: 100 ns ticks since 1601-01-01. A time before 1601 answers 0.
pub fn filetime(secs: i64, nanos: u32) -> u64 {
    let ticks = (i128::from(secs) + i128::from(EPOCH_GAP_SECS)) * 10_000_000
        + i128::from(nanos.min(999_999_999) / 100);
    u64::try_from(ticks.max(0)).unwrap_or(u64::MAX)
}

/// A `SystemTime` as a `FILETIME` ([`filetime`]).
pub fn system_filetime(time: std::time::SystemTime) -> u64 {
    match time.duration_since(std::time::UNIX_EPOCH) {
        Ok(after) => filetime(after.as_secs() as i64, after.subsec_nanos()),
        Err(before) => {
            let before = before.duration();
            // Rounded towards the past: -(secs + nanos) is -(secs + 1) plus the rest of a second.
            let (secs, nanos) = (before.as_secs() as i64, before.subsec_nanos());
            if nanos == 0 {
                filetime(-secs, 0)
            } else {
                filetime(-secs - 1, 1_000_000_000 - nanos)
            }
        }
    }
}

/// The creation time of a file: the birth time when the file system keeps one, else the older of
/// the last write and the last status change (all `FILETIME`s).
pub fn creation(birth: Option<u64>, written: u64, changed: u64) -> u64 {
    birth.unwrap_or_else(|| written.min(changed))
}

/// `FILE_ATTRIBUTE_DIRECTORY` and `FILE_ATTRIBUTE_ARCHIVE`, the attributes `files::attributes`
/// gives a Unix file.
#[cfg(unix)]
const DIRECTORY: u32 = 0x10;
#[cfg(unix)]
const ARCHIVE: u32 = 0x20;

/// A file's attributes, creation, last-access and last-write times for a `WIN32_FIND_DATAA`,
/// from its metadata (as `files::attributes` has it: a directory, else an archive file). The
/// times are the file system's own; a time it cannot give stops the run with a message naming
/// the file, never a made-up value.
#[cfg(unix)]
pub fn find_fields(meta: &std::fs::Metadata, shown: &str) -> (u32, u64, u64, u64) {
    use std::os::unix::fs::MetadataExt;
    let time = |what: &str, got: std::io::Result<std::time::SystemTime>| {
        got.map(system_filetime).unwrap_or_else(|why| {
            panic!("FindFirstFileA(\"{shown}\"): the file system gives no {what} time ({why})")
        })
    };
    let written = time("last write", meta.modified());
    let accessed = time("last access", meta.accessed());
    let changed = filetime(meta.ctime(), meta.ctime_nsec().clamp(0, 999_999_999) as u32);
    let created = creation(meta.created().ok().map(system_filetime), written, changed);
    let attributes = if meta.is_dir() { DIRECTORY } else { ARCHIVE };
    (attributes, created, accessed, written)
}

/// The XDG data folder: `$XDG_DATA_HOME` when it is set to an absolute path (the specification
/// ignores a relative one), else `$HOME/.local/share`.
pub fn xdg_data_home(
    xdg: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
) -> Option<PathBuf> {
    match xdg.map(Path::new) {
        Some(p) if p.is_absolute() => Some(p.to_path_buf()),
        _ => home
            .filter(|h| !h.is_empty())
            .map(|h| Path::new(h).join(".local").join("share")),
    }
}

/// Where the player's tables live (the sound table, the Direct3D 9 adapter table, the D3DX
/// texture cache): `%LOCALAPPDATA%\nv-rs` on Windows (as the call sites always had it), elsewhere
/// `$XDG_DATA_HOME/nv-rs` ([`xdg_data_home`]).
pub fn data_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("nv-rs"))
    }
    #[cfg(not(windows))]
    {
        xdg_data_home(
            std::env::var_os("XDG_DATA_HOME").as_deref(),
            std::env::var_os("HOME").as_deref(),
        )
        .map(|d| d.join("nv-rs"))
    }
}

/// The folder the game's user data lives in off Windows: `<data_dir>/userdata`, with `Documents`,
/// `AppData/Roaming` and `AppData/Local` under it. Nothing is created here.
pub fn userdata_dir() -> Option<PathBuf> {
    data_dir().map(|d| d.join("userdata"))
}

/// The player's Documents, AppData and Local AppData folders as the game sees them
/// ([`userdata_dir`]); empty when there is no data folder.
pub fn known_folders() -> Vec<PathBuf> {
    userdata_dir().map_or_else(Vec::new, |u| {
        vec![
            u.join("Documents"),
            u.join("AppData").join("Roaming"),
            u.join("AppData").join("Local"),
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    /// Mods' own files on a Unix host (upstream's `mod_files` tests use Windows paths with
    /// `std::fs`, so they run on Windows only): the game's folder as the native build gives it
    /// (`Z:` and backslashes), a read from the game's Data, a write kept in the port's folder
    /// whatever the case of the name's parts there, and the game's file never written.
    #[cfg(unix)]
    #[test]
    fn mod_files_on_a_unix_host_read_the_game_and_keep_writes_in_the_ports_folder() {
        use engine_core::mod_files as seam;
        let root = std::env::temp_dir().join(format!("nv-fork-mod-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (game, kept) = (root.join("game"), root.join("kept"));
        std::fs::create_dir_all(game.join("Data/Config")).unwrap();
        std::fs::write(
            game.join("Data/Config/Mod.ini"),
            b"[List]\r\nMaxWidth=350\r\n",
        )
        .unwrap();
        let mut e = crate::Engine::new();
        e.kernel.current_directory = format!("Z:{}", game.display())
            .replace('/', "\\")
            .into_bytes();
        let name = br"Data\Config\Mod.ini";
        assert_eq!(
            seam::profile_string(&e, name, b"List", b"MaxWidth").as_deref(),
            Some(&b"350"[..])
        );
        // The port's folder holds the name's folders in another case.
        std::fs::create_dir_all(kept.join("data/config")).unwrap();
        crate::mod_files::set_folder(&mut e, Some(kept.clone()));
        seam::write_profile_string(&mut e, name, b"List", b"MaxWidth", b"100").unwrap();
        assert_eq!(
            std::fs::read(kept.join("data/config/Mod.ini")).unwrap(),
            b"[List]\r\nMaxWidth=100\r\n"
        );
        // No second folder in the game's case (read from the listing: a case-insensitive file
        // system answers `exists` for either case).
        let names: Vec<_> = std::fs::read_dir(&kept)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["data"]);
        assert_eq!(
            std::fs::read(game.join("Data/Config/Mod.ini")).unwrap(),
            b"[List]\r\nMaxWidth=350\r\n"
        );
        assert_eq!(
            seam::profile_string(&e, name, b"List", b"MaxWidth").as_deref(),
            Some(&b"100"[..])
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn filetime_counts_100_ns_ticks_from_1601() {
        assert_eq!(filetime(0, 0), 116_444_736_000_000_000);
        assert_eq!(filetime(1, 0), 116_444_736_000_000_000 + 10_000_000);
        assert_eq!(
            filetime(0, 123_456_789),
            116_444_736_000_000_000 + 1_234_567
        );
        assert_eq!(filetime(-EPOCH_GAP_SECS, 0), 0);
        assert_eq!(filetime(-EPOCH_GAP_SECS - 5, 0), 0);
        // 2024-01-01T00:00:00Z
        assert_eq!(filetime(1_704_067_200, 0), 133_485_408_000_000_000);
    }

    #[test]
    fn a_time_before_1970_rounds_towards_the_past() {
        use std::time::{Duration, UNIX_EPOCH};
        let t = UNIX_EPOCH - Duration::new(2, 500_000_000);
        assert_eq!(system_filetime(t), filetime(-3, 500_000_000));
        assert_eq!(
            system_filetime(UNIX_EPOCH - Duration::from_secs(2)),
            filetime(-2, 0)
        );
    }

    #[test]
    fn without_a_birth_time_the_oldest_recorded_time_is_the_creation() {
        assert_eq!(creation(Some(7), 5, 3), 7);
        assert_eq!(creation(None, 5, 3), 3);
        assert_eq!(creation(None, 2, 3), 2);
    }

    #[test]
    fn xdg_data_home_prefers_an_absolute_xdg_path() {
        let path = |p: &str| Some(Path::new(p).to_path_buf());
        let abs = if cfg!(windows) { "C:\\data" } else { "/data" };
        assert_eq!(
            xdg_data_home(Some(OsStr::new(abs)), Some(OsStr::new("/h"))),
            path(abs)
        );
        assert_eq!(
            xdg_data_home(Some(OsStr::new("rel")), Some(OsStr::new("/h"))),
            Some(Path::new("/h").join(".local").join("share"))
        );
        assert_eq!(
            xdg_data_home(None, Some(OsStr::new("/h"))),
            Some(Path::new("/h").join(".local").join("share"))
        );
        assert_eq!(xdg_data_home(None, None), None);
        assert_eq!(xdg_data_home(None, Some(OsStr::new(""))), None);
    }
}

/// Tests that read and set the process's environment run one at a time.
#[cfg(all(test, unix))]
mod unix_tests {
    use super::*;
    use crate::files::{find_close, find_first_file_a, find_next_file_a, INVALID_HANDLE_VALUE};
    use crate::{Engine, Ptr};
    use std::fs::{File, FileTimes};
    use std::sync::Mutex;
    use std::time::{Duration, UNIX_EPOCH};

    static ENV: Mutex<()> = Mutex::new(());

    fn scratch(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nv-rs-fork-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn text(e: &mut Engine, s: &str) -> Ptr {
        let at = e.mem.alloc(0x200);
        e.mem.set_cstr(at, s.as_bytes());
        Ptr::new(at)
    }

    fn time_at(e: &Engine, data: Ptr, offset: u32) -> u64 {
        u64::from(e.mem.u32(data.addr() + offset + 4)) << 32
            | u64::from(e.mem.u32(data.addr() + offset))
    }

    #[test]
    fn a_search_answers_the_files_own_times_in_ntfs_order() {
        let dir = scratch("search-times");
        // Name, last write (seconds), last access (seconds); written out of order.
        let files = [
            ("b.esm", 1_600_000_000u64, 1_650_000_000u64),
            ("A.esm", 1_500_000_000, 1_550_000_000),
            ("c.esp", 1_700_000_000, 1_710_000_000),
        ];
        for (name, write, access) in files {
            let file = File::create(dir.join(name)).unwrap();
            file.set_times(
                FileTimes::new()
                    .set_modified(UNIX_EPOCH + Duration::new(write, 250_000_000))
                    .set_accessed(UNIX_EPOCH + Duration::new(access, 0)),
            )
            .unwrap();
        }
        std::fs::create_dir(dir.join("sub")).unwrap();
        let mut e = Engine::new();
        let pattern = text(&mut e, &format!("{}/*.es?", dir.display()));
        let data = Ptr::new(e.mem.alloc(0x140));
        let h = find_first_file_a(&mut e, pattern, data);
        assert_ne!(h, INVALID_HANDLE_VALUE);
        let mut seen = Vec::new();
        loop {
            let name = String::from_utf8(e.mem.cstr(data.addr() + 0x2c)).unwrap();
            seen.push((
                name,
                e.mem.u32(data.addr()),
                time_at(&e, data, 0xc),
                time_at(&e, data, 0x14),
                time_at(&e, data, 4),
            ));
            if find_next_file_a(&mut e, h, data) == 0 {
                break;
            }
        }
        assert_eq!(find_close(&mut e, h), 1);
        let names: Vec<&str> = seen.iter().map(|s| s.0.as_str()).collect();
        assert_eq!(names, ["A.esm", "b.esm", "c.esp"]);
        let by_name = |n: &str| seen.iter().find(|s| s.0 == n).unwrap();
        for (name, write, access) in files {
            let s = by_name(name);
            assert_eq!(s.1, ARCHIVE, "{name}");
            assert_eq!(
                s.3,
                filetime(write as i64, 250_000_000),
                "{name} last write"
            );
            assert_eq!(s.2, filetime(access as i64, 0), "{name} last access");
            assert_ne!(s.4, 0, "{name} creation");
        }
        // A directory is described as one.
        let sub = text(&mut e, &format!("{}/sub", dir.display()));
        let h = find_first_file_a(&mut e, sub, data);
        assert_ne!(h, INVALID_HANDLE_VALUE);
        assert_eq!(e.mem.u32(data.addr()), DIRECTORY);
        assert_eq!(e.mem.cstr(data.addr() + 0x2c), b"sub".to_vec());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_creation_time_is_the_birth_time_the_file_system_keeps() {
        let dir = scratch("birth");
        let path = dir.join("f.esm");
        std::fs::write(&path, b"x").unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        let (_, created, _, written) = find_fields(&meta, "f.esm");
        match meta.created() {
            Ok(birth) => assert_eq!(created, system_filetime(birth)),
            Err(_) => assert!(created <= written),
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_tables_default_under_xdg_data_home() {
        let _guard = ENV.lock().unwrap_or_else(|p| p.into_inner());
        let saved = (std::env::var_os("XDG_DATA_HOME"), std::env::var_os("HOME"));
        std::env::set_var("XDG_DATA_HOME", "/fake/xdg");
        assert_eq!(data_dir(), Some(PathBuf::from("/fake/xdg/nv-rs")));
        assert_eq!(
            userdata_dir(),
            Some(PathBuf::from("/fake/xdg/nv-rs/userdata"))
        );
        assert_eq!(
            known_folders(),
            [
                PathBuf::from("/fake/xdg/nv-rs/userdata/Documents"),
                PathBuf::from("/fake/xdg/nv-rs/userdata/AppData/Roaming"),
                PathBuf::from("/fake/xdg/nv-rs/userdata/AppData/Local"),
            ]
        );
        std::env::remove_var("XDG_DATA_HOME");
        std::env::set_var("HOME", "/fake/home");
        assert_eq!(
            data_dir(),
            Some(PathBuf::from("/fake/home/.local/share/nv-rs"))
        );
        for (key, value) in [("XDG_DATA_HOME", saved.0), ("HOME", saved.1)] {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
    }
}
