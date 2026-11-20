//! The profile (INI) file imports, `GetPrivateProfileIntA`, `GetPrivateProfileStringA` and
//! `WritePrivateProfileStringA`: they read and write the files the game made in this run
//! ([`files`]) and the Windows directory of the player's table ([`d3d9::windows_directory`]), so
//! they are in the platform layer rather than the core (moved from `engine-core`'s
//! `units/platform.rs`). A write changes the run's copy of the file only, never the player's; the
//! host copies the game's `FalloutPrefs.ini` out where a launcher asks (`NV_PREFS_OUT`).
//!
//! `WritePrivateProfileStringA` is based on Lhomer's nv-rs commit 8f4cc9ba (GPL-3.0,
//! https://github.com/Lhomer/nv-rs/commit/8f4cc9ba7fa66fe5855a828150343ec86e5ef60b): the write
//! through the run's copy of the file ([`files::write_whole`]), its null arguments and its
//! tests. The text rules are the fork's measured ones (`engine_core::mod_files`), which the
//! extenders' mod files already use.

#[allow(unused_imports)]
use crate::prelude::*;

use super::{d3d9, files};

/// The value of a key as `GetPrivateProfileStringA` reads it: the core's (`engine_core::mod_files`,
/// which the extenders' mod files share); its measured answers are this file's tests.
pub use engine_core::mod_files::ini_value;

use engine_core::mod_files::text::Decoded;
use engine_core::mod_files::{removed_key, removed_section, written_profile, ERROR_NOT_SUPPORTED};

/// Where the profile functions look for the file `name` names (measured on scratch files,
/// 2026-11-16, `build43-scratch/measure/mcm/ini-measure/ini-2026-11-16.txt`): a bare name (no `\`
/// or `/`) is in the Windows directory (`d3d9::windows_directory`, from the player's table); a
/// full path is itself; any other name with a folder part (`Data\Config\MCM.ini`,
/// `Data/Config/MCM.ini`, `.\name.ini`) is relative to the current directory. Before 2026-11-16
/// every name that was not a full path was looked for in the Windows directory.
fn profile_path(e: &mut Engine, name: &[u8]) -> Vec<u8> {
    if files::is_full_path(name) {
        name.to_vec()
    } else if name.iter().any(|&b| b == b'\\' || b == b'/') {
        files::full_path(e, name)
    } else {
        let mut path = d3d9::windows_directory(e);
        path.push(b'\\');
        path.extend_from_slice(name);
        path
    }
}

/// The text of the profile file `file` names ([`profile_path`]): the file as the game made it
/// in this run when it did (`files`), else the player's file, read only. `None` when there is
/// none.
fn profile_text(e: &mut Engine, file: Ptr) -> Option<Vec<u8>> {
    let name = e.mem.cstr(file.addr());
    let path = profile_path(e, &name);
    files::read_whole(e, &path)
}

// Platform: replaces KERNEL32.DLL GetPrivateProfileIntA (import)
/// `GetPrivateProfileIntA(section, key, default, file)`: the key's value as an integer, or
/// `default` when the file, the section or the key isn't there ([`profile_text`]). The value is
/// read as [`profile_int`] does, which follows Windows' answers measured on the player's machine
/// (2026-10-21, 38 values).
///
/// One key answers 1 whatever the file says: `[General] bUseMyGamesDirectory`
/// ([`MY_GAMES_KEY`]), which the game reads at start-up from `.\Custom.ini` and `.\FALLOUT.INI`
/// in its own folder (`00876ad0`, call sites `00876b13` and `00876b55`): 0 there would put its INI
/// files, `plugins.txt` and saves in the game's folder, where nv-rs never writes. Before 2026-11-16
/// those names were looked for in the Windows directory, found nothing and gave the default, 1;
/// with names now read from the current directory as Windows reads them, the key is pinned
/// instead: a deliberate difference, said once in the log when a file holds 0.
pub fn get_private_profile_int_a(
    e: &mut Engine,
    section: Ptr,
    key: Ptr,
    default: u32,
    file: Ptr,
) -> u32 {
    let section = e.mem.cstr(section.addr());
    let key = e.mem.cstr(key.addr());
    let value = profile_text(e, file).and_then(|text| ini_value(&text, &section, &key));
    let trim = engine_core::mod_files::trim_spaces;
    if MY_GAMES_KEY.0.eq_ignore_ascii_case(trim(&section))
        && MY_GAMES_KEY.1.eq_ignore_ascii_case(trim(&key))
    {
        if value.as_deref().map(profile_int) == Some(0) {
            static SAID: std::sync::Once = std::sync::Once::new();
            let name = String::from_utf8_lossy(&e.mem.cstr(file.addr())).into_owned();
            SAID.call_once(|| {
                println!(
                    "{name}: bUseMyGamesDirectory=0 is set in the game folder's settings; nv-rs keeps settings and saves in its own folder"
                )
            });
        }
        return 1;
    }
    let Some(value) = value else {
        return default;
    };
    profile_int(&value)
}

/// The section and key [`get_private_profile_int_a`] answers 1 for.
pub const MY_GAMES_KEY: (&[u8], &[u8]) = (b"General", b"bUseMyGamesDirectory");

/// A profile value as Windows' `GetPrivateProfileIntA` reads it (measured): leading spaces and
/// tabs, then one optional `+` or `-`, then an optional lower-case base prefix `0x` (16), `0o`
/// (8) or `0b` (2), else base 10; digits of that base up to the first other character, kept
/// modulo 2^32; a `-` negates the result modulo 2^32. No digits (`-`, `0x`, `0b2`, `x5`) give 0;
/// upper-case prefixes are not prefixes (`0X1F` is 0).
fn profile_int(value: &[u8]) -> u32 {
    let mut rest = value;
    while let [b' ' | b'\t', tail @ ..] = rest {
        rest = tail;
    }
    let negative = match rest {
        [b'-', tail @ ..] => {
            rest = tail;
            true
        }
        [b'+', tail @ ..] => {
            rest = tail;
            false
        }
        _ => false,
    };
    let base = match rest {
        [b'0', b'x', tail @ ..] => {
            rest = tail;
            16
        }
        [b'0', b'o', tail @ ..] => {
            rest = tail;
            8
        }
        [b'0', b'b', tail @ ..] => {
            rest = tail;
            2
        }
        _ => 10,
    };
    let mut n = 0u32;
    for &c in rest {
        let Some(digit) = char::from(c).to_digit(base) else {
            break;
        };
        n = n.wrapping_mul(base).wrapping_add(digit);
    }
    if negative {
        n.wrapping_neg()
    } else {
        n
    }
}

// Platform: replaces KERNEL32.DLL GetPrivateProfileStringA (import)
/// The value of `key` in `section` of the INI file `file` ([`profile_text`]), or
/// `default` (none: empty) when the file, the section or the key isn't there: copied into
/// `buffer`, cut to `size - 1` bytes and zero-terminated; answers the bytes copied. Listing a
/// file's sections or keys (a null section or key) is not provided.
pub fn get_private_profile_string_a(
    e: &mut Engine,
    section: Ptr,
    key: Ptr,
    default: Ptr,
    buffer: Ptr,
    size: u32,
    file: Ptr,
) -> u32 {
    if section.is_null() || key.is_null() {
        panic!("GetPrivateProfileStringA with a null section or key is not provided");
    }
    let section = e.mem.cstr(section.addr());
    let key = e.mem.cstr(key.addr());
    let found = profile_text(e, file).and_then(|text| ini_value(&text, &section, &key));
    let value = match found {
        Some(value) => value,
        None if default.is_null() => Vec::new(),
        None => e.mem.cstr(default.addr()),
    };
    if size == 0 {
        return 0;
    }
    let copied = value.len().min(size as usize - 1);
    e.mem.set_cstr(buffer.addr(), &value[..copied]);
    copied as u32
}

// Platform: replaces KERNEL32.DLL WritePrivateProfileStringA (import)
/// `WritePrivateProfileStringA(section, key, value, file)` on the run's copy of the file
/// ([`profile_path`], [`files::write_whole`]; the player's file is read once and never changed),
/// with Windows' text rules as measured on scratch files (`engine_core::mod_files`):
/// - `key=value` set in the first section of the name ([`written_profile`]): an existing key
///   keeps its line up to `=` and gets the new value; a new key goes after the section's last
///   key line; a missing section is added at the end;
/// - a null `value` deletes the key's line ([`removed_key`]), a null `key` the section
///   ([`removed_section`]); a missing file is then made empty, as Windows makes it;
/// - a missing file is made when its folder exists; when it does not, nothing is written and the
///   answer is 0 with `ERROR_PATH_NOT_FOUND`; a file that is there but cannot be read is not
///   written either (0, its error);
/// - a UTF-16 LE file is changed as text and written back so; a file with a 0 byte that is not one
///   is left as it is (0, `ERROR_NOT_SUPPORTED`, one line in the log), as the mod files' writes do.
///
/// A null section (Windows' flush of its cache) or file (`win.ini`) changes nothing and answers
/// 1. Until 2026-11-17 every call answered 1 and wrote nothing, so the Settings menu's choices
/// (`INISettingCollection::WriteSetting`, `00c43e90`, run when a changed page is left) were gone
/// at the next start.
pub fn write_private_profile_string_a(
    e: &mut Engine,
    section: Ptr,
    key: Ptr,
    value: Ptr,
    file: Ptr,
) -> u32 {
    if section.is_null() || file.is_null() {
        return 1;
    }
    let name = e.mem.cstr(file.addr());
    let path = profile_path(e, &name);
    let section = e.mem.cstr(section.addr());
    let key = (!key.is_null()).then(|| e.mem.cstr(key.addr()));
    let value = (!value.is_null()).then(|| e.mem.cstr(value.addr()));
    let old = match files::read_whole_or_error(e, &path) {
        Ok(bytes) => Some(bytes),
        Err(files::ERROR_FILE_NOT_FOUND | files::ERROR_PATH_NOT_FOUND) => None,
        Err(code) => {
            println!(
                "WritePrivateProfileStringA: {} could not be read (Windows error {code}); not written",
                String::from_utf8_lossy(&path)
            );
            e.kernel.last_error = code;
            return 0;
        }
    };
    if old.is_none() && !files::directory_exists(e, &path) {
        e.kernel.last_error = files::ERROR_PATH_NOT_FOUND;
        return 0;
    }
    let file = Decoded::of(old.as_deref().unwrap_or_default());
    if !file.writable || (file.mark_only() && (key.is_none() || value.is_none())) {
        println!(
            "WritePrivateProfileStringA: {} is not a text file Windows' profile calls were measured \
             on; not written",
            String::from_utf8_lossy(&path)
        );
        e.kernel.last_error = ERROR_NOT_SUPPORTED;
        return 0;
    }
    let section = file.arg(&section);
    let new = match (key, value) {
        (Some(key), Some(value)) => {
            let mut new = written_profile(&file.text, &section, &file.arg(&key), &file.arg(&value));
            // A UTF-16 file that is only its mark gets a line end first (measured, as
            // `engine_core::mod_files::write_profile_string`).
            if file.mark_only() {
                new.splice(0..0, b"\r\n".iter().copied());
            }
            Some(new)
        }
        (Some(key), None) => removed_key(&file.text, &section, &file.arg(&key)),
        (None, _) => removed_section(&file.text, &section),
    };
    match new {
        Some(new) => {
            let bytes = file.encode(&new);
            if old.as_deref() != Some(&bytes[..]) {
                files::write_whole(e, &path, bytes);
            }
        }
        // Nothing to change; a missing file is made empty (measured).
        None if old.is_none() => files::write_whole(e, &path, Vec::new()),
        None => {}
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> Engine {
        let mut e = Engine::new();
        e.mem.map(0x1000, 0x1000);
        e
    }

    #[test]
    fn ini_values_are_found_by_section_and_key() {
        let text = b"; comment\r\n[General]\r\n sLanguage = ENGLISH \r\n[Controls]\r\nForward=\"0011FF00\"\r\nBack=1F\r\n";
        assert_eq!(
            ini_value(text, b"controls", b"forward").unwrap(),
            b"0011FF00"
        );
        assert_eq!(ini_value(text, b"Controls", b"Back").unwrap(), b"1F");
        assert_eq!(
            ini_value(text, b"General", b"sLanguage").unwrap(),
            b"ENGLISH"
        );
        assert_eq!(ini_value(text, b"General", b"Back"), None);
    }

    /// A `;` line is never found: a key nv-rs's `Fallout.ini` writes commented out
    /// (`distribution/nv-rs-sections.ini`) or a player commented out is not the key, and the
    /// `[nv-rs ...]` sections are sections like any other.
    #[test]
    fn a_commented_key_is_never_found() {
        let text = b"[nv-rs Fixes]\r\n; Uncapped frame rate: a note\r\n;bUncap=1\r\n\
                     [Display]\r\n;iPresentInterval=0\r\niPresentInterval=1\r\n";
        assert_eq!(ini_value(text, b"nv-rs Fixes", b"bUncap"), None);
        assert_eq!(
            ini_value(text, b"Display", b"iPresentInterval").unwrap(),
            b"1"
        );
    }

    /// A file that starts with a UTF-8 byte order mark, as Windows' `GetPrivateProfileStringA`
    /// reads it (measured on scratch files, 2026-11-09, measure/ini/DESIGN.md): a heading glued
    /// to the mark is no heading, so the keys under it are in no section and not found, and the
    /// sections after it read as usual. Play.exe puts nv-rs's block at the end of such a file
    /// (the launcher, crates/nv-launcher: `game_ini::with_sections`), where it changes none of the player's keys.
    #[test]
    fn a_heading_glued_to_a_byte_order_mark_is_no_heading() {
        let marked = b"\xef\xbb\xbf[General]\r\nsA=1\r\nsB=two\r\n[Display]\r\nfB=2\r\n\
                       \r\n[nv-rs General]\r\nsPreset=fixes\r\n";
        assert_eq!(ini_value(marked, b"General", b"sA"), None);
        assert_eq!(ini_value(marked, b"General", b"sB"), None);
        assert_eq!(ini_value(marked, b"Display", b"fB").unwrap(), b"2");
        assert_eq!(
            ini_value(marked, b"nv-rs General", b"sPreset").unwrap(),
            b"fixes"
        );
        // The mark alone on the first line: the next line is a heading.
        let alone = b"\xef\xbb\xbf\r\n[General]\r\nsA=1\r\n";
        assert_eq!(ini_value(alone, b"General", b"sA").unwrap(), b"1");
    }

    /// Windows' answers on a file with a repeated section name, measured on scratch files on the
    /// player's machine (2026-11-09; `GetPrivateProfileStringA` and `GetPrivateProfileIntA` alike):
    /// only the first section of a name is read, the name compared without case and with the
    /// spaces in the brackets and in the caller's name trimmed. An empty first section hides a
    /// later one; an empty key in the first one is found, empty.
    #[test]
    fn only_the_first_section_of_a_name_is_read() {
        let text = b"[A]\r\nx=1\r\n[B]\r\nz=3\r\n[a]\r\ny=2\r\nx=9\r\n[ A ]\r\nw=6\r\n";
        assert_eq!(ini_value(text, b"A", b"x").unwrap(), b"1");
        assert_eq!(ini_value(text, b"A", b"y"), None);
        assert_eq!(ini_value(text, b"A", b"w"), None);
        assert_eq!(ini_value(text, b"B", b"z").unwrap(), b"3");
        assert_eq!(ini_value(text, b" A ", b"x").unwrap(), b"1");
        let text = b"[a]\r\nx=1\r\n[A]\r\ny=2\r\n[C]\r\nq=1\r\n[C]\r\nq=2\r\nr=3\r\n";
        assert_eq!(ini_value(text, b"A", b"x").unwrap(), b"1");
        assert_eq!(ini_value(text, b"A", b"y"), None);
        assert_eq!(ini_value(text, b"C", b"q").unwrap(), b"1");
        assert_eq!(ini_value(text, b"C", b"r"), None);
        assert_eq!(ini_value(b"[A]\r\n[A]\r\nx=1\r\n", b"A", b"x"), None);
        assert_eq!(
            ini_value(b"[A]\r\n; note\r\n\r\n[A]\r\nx=1\r\n", b"A", b"x"),
            None
        );
        assert_eq!(
            ini_value(b"[A]\r\nx=\r\n[A]\r\nx=1\r\n", b"A", b"x").unwrap(),
            b""
        );
    }

    /// Three more of Windows' `GetPrivateProfileStringA` answers, measured on scratch files on
    /// the player's machine (2026-11-10): a key repeated in one section gives its first line,
    /// whatever its case and spaces; a heading is the text up to its first `]` (text after it, or
    /// no `]` at all, changes nothing, and `[A]x=9` is a heading, not a key); a key before any
    /// heading is in no section, not even the one named "", which `[]` opens.
    #[test]
    fn repeated_keys_headings_and_keys_before_any_section_read_as_windows_does() {
        assert_eq!(
            ini_value(b"[A]\r\nx=1\r\nx=2\r\n", b"A", b"x").unwrap(),
            b"1"
        );
        assert_eq!(
            ini_value(b"[A]\r\nX=1\r\n x =2\r\n", b"A", b"x").unwrap(),
            b"1"
        );
        let headings: [&[u8]; 6] = [
            b"[A] trailing\r\nx=1\r\n",
            b"[A]; note\r\nx=1\r\n",
            b"[A]]\r\nx=1\r\n",
            b"[A\r\nx=1\r\n",
            b"[A]x=9\r\nx=1\r\n",
            b" [A] \r\nx=1\r\n",
        ];
        for text in headings {
            assert_eq!(ini_value(text, b"A", b"x").unwrap(), b"1");
        }
        assert_eq!(ini_value(b"[A]]\r\nx=1\r\n", b"A]", b"x"), None);
        assert_eq!(ini_value(b"x=1\r\n[A]\r\ny=2\r\n", b"", b"x"), None);
        assert_eq!(ini_value(b"x=1\r\n[A]\r\ny=2\r\n", b"A", b"x"), None);
        assert_eq!(ini_value(b"x=1\r\n[]\r\nz=3\r\n", b"", b"z").unwrap(), b"3");
        assert_eq!(ini_value(b"x=1\r\n[]\r\nz=3\r\n", b"", b"x"), None);
    }

    /// Windows' `GetPrivateProfileIntA` answers, measured on the player's machine (2026-10-21).
    #[test]
    fn profile_ints_read_as_windows_does() {
        let measured: &[(&[u8], u32)] = &[
            (b"-5", 0xffff_fffb),
            (b"-0", 0),
            (b"0x10", 16),
            (b"0X1F", 0),
            (b"-0x10", 0xffff_fff0),
            (b"12abc", 12),
            (b"  7", 7),
            (b"+3", 3),
            (b"4294967295", 0xffff_ffff),
            (b"4294967296", 0),
            (b"99999999999", 1_215_752_191),
            (b"1e3", 1),
            (b"0", 0),
            (b"08", 8),
            (b"010", 10),
            (b"-", 0),
            (b"x5", 0),
            (b"0b101", 5),
            (b"0o17", 15),
            (b"0x", 0),
            (b"0xg", 0),
            (b"-  5", 0),
            (b" -5", 0xffff_fffb),
            (b"0b2", 0),
            (b"0x10abc", 0x10abc),
            (b"0xFFFFFFFF1", 0xffff_fff1),
            (b"0B11", 0),
            (b"0O7", 0),
            (b"\t9", 9),
            (b"--5", 0),
            (b"+-5", 0),
            (b"- 5", 0),
            (b"00x10", 0),
            (b"0x-5", 0),
            (b"0b", 0),
            (b"0o8", 0),
            (b"5 6", 5),
        ];
        for &(value, want) in measured {
            assert_eq!(profile_int(value), want, "{:?}", std::str::from_utf8(value));
        }
    }

    /// Where a name is looked for, as Windows looked on scratch files (2026-11-16): a name with a
    /// folder part from the current directory (both separators, `.\` too), a bare name from the
    /// Windows directory, a full path as it is.
    #[test]
    fn a_name_with_a_folder_part_is_relative_to_the_current_directory() {
        let mut e = engine();
        e.kernel.current_directory = br"C:\Games\FNV".to_vec();
        assert_eq!(
            profile_path(&mut e, br"Data\Config\MCM.ini"),
            br"C:\Games\FNV\Data\Config\MCM.ini"
        );
        assert_eq!(
            profile_path(&mut e, b"Data/Config/MCM.ini"),
            b"C:\\Games\\FNV\\Data/Config/MCM.ini"
        );
        assert_eq!(
            profile_path(&mut e, br".\bare.ini"),
            br"C:\Games\FNV\bare.ini"
        );
        assert_eq!(profile_path(&mut e, br"D:\x\y.ini"), br"D:\x\y.ini");
        // A bare name: the Windows directory, from the machine's table.
        windows_table(&mut e);
        assert_eq!(
            profile_path(&mut e, b"bare-nvrs-measure.ini"),
            br"C:\WINDOWS\bare-nvrs-measure.ini"
        );
    }

    /// A machine table with only its Windows directory, `C:\WINDOWS`.
    fn windows_table(e: &mut Engine) {
        let hex: String = br"C:\WINDOWS".iter().map(|b| format!("{b:02x}")).collect();
        d3d9::load_adapter_table(e, &format!("GetWindowsDirectoryA\t-\t0\t{hex}\n"));
    }

    /// A scratch folder removed when the test ends, however it ends.
    #[cfg(windows)]
    struct Scratch(std::path::PathBuf);
    #[cfg(windows)]
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[cfg(windows)]
    fn scratch(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("nv-ini-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    /// `GetPrivateProfileIntA` through a name with a folder part (`.\FALLOUT.INI`, as the game
    /// reads it at start-up) reads the current directory's file; `[General]
    /// bUseMyGamesDirectory` answers 1 whatever the file says (the game's folder keeps nothing of
    /// nv-rs's), other keys their values; a bare name reads the Windows directory.
    // Windows only: it builds the game's names with backslashes and drive letters, which the platform's
    // file layer maps to host paths on Windows alone (`files::real_path` keeps a backslash
    // as part of a name elsewhere; starting the game there is refused today).
    #[cfg(windows)]
    #[test]
    fn profile_ints_read_relative_names_and_pin_the_my_games_key() {
        let game = scratch("game");
        std::fs::write(
            game.0.join("FALLOUT.INI"),
            b"[General]\r\nbUseMyGamesDirectory=0\r\niOther=7\r\n",
        )
        .unwrap();
        let mut e = engine();
        windows_table(&mut e);
        e.kernel.current_directory = game.0.to_string_lossy().replace('/', "\\").into_bytes();
        e.mem.set_cstr(0x1000, b"General");
        e.mem.set_cstr(0x1020, b"bUseMyGamesDirectory");
        e.mem.set_cstr(0x1040, br".\FALLOUT.INI");
        e.mem.set_cstr(0x1060, b"iOther");
        e.mem.set_cstr(0x1080, b"FALLOUT.INI");
        let int = |e: &mut Engine, key: u32, file: u32| {
            get_private_profile_int_a(e, Ptr::new(0x1000), Ptr::new(key), 5, Ptr::new(file))
        };
        assert_eq!(int(&mut e, 0x1020, 0x1040), 1, "pinned, the file says 0");
        assert_eq!(int(&mut e, 0x1060, 0x1040), 7, "another key reads the file");
        assert_eq!(
            int(&mut e, 0x1060, 0x1080),
            5,
            "a bare name: the Windows directory"
        );
        std::fs::write(game.0.join("FALLOUT.INI"), b"[General]\r\niOther=7\r\n").unwrap();
        assert_eq!(int(&mut e, 0x1020, 0x1040), 1, "no key: 1 too");
    }

    /// Quotes around a value, as Windows read them on scratch files (2026-11-16,
    /// `build43-scratch/measure/mcm/ini-measure/ini3-2026-11-16.txt`).
    #[test]
    fn quotes_around_a_value_read_as_windows_does() {
        let measured: &[(&[u8], &[u8])] = &[
            (b"x='1'", b"1"),
            (b"x=\"1\"", b"1"),
            (b"x=\"1'", b"\"1'"),
            (b"x=\"", b"\""),
            (b"x=\"\"", b""),
            (b"x= \"1\" ", b"1"),
            (b"x=''", b""),
            (b"x='", b"'"),
            (b"x=\"a\"b\"", b"a\"b"),
            (b"x=\"1\" ;c", b"\"1\" ;c"),
            (b"x=\"1\"\"", b"1\""),
        ];
        for &(line, want) in measured {
            let text = [b"[A]\r\n", line, b"\r\n"].concat();
            assert_eq!(
                ini_value(&text, b"A", b"x").unwrap(),
                want,
                "{}",
                String::from_utf8_lossy(line)
            );
        }
    }

    /// The game's own profile reads do not look in the mods' folder: a value a mod's command
    /// wrote (`engine_core::mod_files`) is not what the import reads; the game's file is.
    // Windows only: it builds the game's names with backslashes and drive letters, which the platform's
    // file layer maps to host paths on Windows alone (`files::real_path` keeps a backslash
    // as part of a name elsewhere; starting the game there is refused today).
    #[cfg(windows)]
    #[test]
    fn the_games_profile_import_does_not_read_mod_files() {
        let game = scratch("own");
        std::fs::create_dir_all(game.0.join(r"Data\Config")).unwrap();
        std::fs::write(game.0.join(r"Data\Config\X.ini"), b"[A]\r\nx=game\r\n").unwrap();
        let mut e = engine();
        e.kernel.current_directory = game.0.to_string_lossy().replace('/', "\\").into_bytes();
        engine_core::mod_files::write_profile_string(
            &mut e,
            br"Data\Config\X.ini",
            b"A",
            b"x",
            b"mod",
        )
        .unwrap();
        assert_eq!(
            engine_core::mod_files::profile_string(&e, br"Data\Config\X.ini", b"A", b"x").unwrap(),
            b"mod"
        );
        e.mem.set_cstr(0x1000, b"A");
        e.mem.set_cstr(0x1020, b"x");
        e.mem.set_cstr(0x1040, br"Data\Config\X.ini");
        let n = get_private_profile_string_a(
            &mut e,
            Ptr::new(0x1000),
            Ptr::new(0x1020),
            Ptr::NULL,
            Ptr::new(0x1100),
            0x40,
            Ptr::new(0x1040),
        );
        assert_eq!((n, e.mem.cstr(0x1100)), (4, b"game".to_vec()));
    }

    /// A missing file gives the default, cut to the buffer; a write into a folder that exists
    /// nowhere writes nothing and answers 0 with `ERROR_PATH_NOT_FOUND`.
    #[test]
    fn profile_strings_fall_back_to_the_default() {
        let mut e = engine();
        e.mem.set_cstr(0x1000, b"Controls");
        e.mem.set_cstr(0x1020, b"Forward");
        e.mem.set_cstr(0x1040, b"default");
        e.mem.set_cstr(0x1060, br"Z:\no\such\file.ini");
        let n = get_private_profile_string_a(
            &mut e,
            Ptr::new(0x1000),
            Ptr::new(0x1020),
            Ptr::new(0x1040),
            Ptr::new(0x1100),
            4,
            Ptr::new(0x1060),
        );
        assert_eq!(n, 3);
        assert_eq!(e.mem.cstr(0x1100), b"def");
        let n = get_private_profile_string_a(
            &mut e,
            Ptr::new(0x1000),
            Ptr::new(0x1020),
            Ptr::NULL,
            Ptr::new(0x1100),
            0x40,
            Ptr::new(0x1060),
        );
        assert_eq!((n, e.mem.u8(0x1100)), (0, 0));
        assert_eq!(
            write_private_profile_string_a(
                &mut e,
                Ptr::new(0x1000),
                Ptr::new(0x1020),
                Ptr::new(0x1040),
                Ptr::new(0x1060)
            ),
            0
        );
        assert_eq!(e.kernel.last_error, files::ERROR_PATH_NOT_FOUND);
        assert_eq!(files::read_whole(&e, br"Z:\no\such\file.ini"), None);
    }

    /// The game's settings file in a folder made in this run (nothing on the host's disk).
    const PREFS: &[u8] = br"Z:\nv-rs-ini-test\FalloutPrefs.ini";

    /// An engine whose run has made `PREFS`'s folder.
    fn engine_with_folder() -> Engine {
        let mut e = engine();
        e.mem.set_cstr(0x1f00, br"Z:\nv-rs-ini-test");
        assert_eq!(files::create_directory_a(&mut e, Ptr::new(0x1f00), 0), 1);
        e
    }

    /// `text` as a C string in the engine's memory; `None` is a null pointer.
    fn put(e: &mut Engine, text: Option<&[u8]>) -> Ptr {
        let Some(text) = text else {
            return Ptr::NULL;
        };
        let at = e.mem.alloc(text.len() as u32 + 1);
        e.mem.set_cstr(at, text);
        Ptr::new(at)
    }

    /// `WritePrivateProfileStringA(section, key, value, path)`.
    fn write(
        e: &mut Engine,
        section: &[u8],
        key: Option<&[u8]>,
        value: Option<&[u8]>,
        path: &[u8],
    ) -> u32 {
        let (s, k, v, f) = (
            put(e, Some(section)),
            put(e, key),
            put(e, value),
            put(e, Some(path)),
        );
        write_private_profile_string_a(e, s, k, v, f)
    }

    /// `GetPrivateProfileStringA(section, key, "", ..., path)`'s text.
    fn read(e: &mut Engine, section: &[u8], key: &[u8], path: &[u8]) -> Vec<u8> {
        let buffer = e.mem.alloc(0x100);
        let (s, k, f) = (put(e, Some(section)), put(e, Some(key)), put(e, Some(path)));
        let n = get_private_profile_string_a(e, s, k, Ptr::NULL, Ptr::new(buffer), 0x100, f);
        e.mem.bytes(buffer, n)
    }

    /// `before` written as the run's `PREFS`, then one write; the file after it.
    fn after_write(
        before: &[u8],
        section: &[u8],
        key: Option<&[u8]>,
        value: Option<&[u8]>,
    ) -> Vec<u8> {
        let mut e = engine_with_folder();
        files::write_whole(&mut e, PREFS, before.to_vec());
        assert_eq!(write(&mut e, section, key, value, PREFS), 1);
        files::read_whole(&e, PREFS).unwrap()
    }

    const TEXT: &[u8] = b"[Display]\r\niSize W=800\r\n\r\n[GamePlay]\r\n;iKill=9\r\nbSaveOnRest=1\r\n\r\n[Audio]\r\nfVol=1\r\n";

    /// An existing key's value is replaced where it stands: the line keeps everything up to `=`
    /// (the file's case and spaces) and its end; names match without case; every other byte stays.
    #[test]
    fn a_written_value_replaces_the_key_in_place() {
        assert_eq!(
            after_write(TEXT, b"gameplay", Some(b"BSAVEONREST"), Some(b"0")),
            b"[Display]\r\niSize W=800\r\n\r\n[GamePlay]\r\n;iKill=9\r\nbSaveOnRest=0\r\n\r\n[Audio]\r\nfVol=1\r\n"
        );
        let general =
            b"; comment\r\n[General]\r\n sLanguage = ENGLISH \r\n[Controls]\r\nBack=1F\r\n";
        assert_eq!(
            after_write(general, b"General", Some(b"slanguage"), Some(b"FRENCH")),
            b"; comment\r\n[General]\r\n sLanguage =FRENCH \r\n[Controls]\r\nBack=1F\r\n"
        );
        // An LF-only file stays LF-only; a last line without an end gets CR LF.
        assert_eq!(
            after_write(b"[A]\nK=1\nL=2\n", b"A", Some(b"K"), Some(b"9")),
            b"[A]\nK=9\nL=2\n"
        );
        assert_eq!(
            after_write(b"[A]\nK=1", b"A", Some(b"K"), Some(b"9")),
            b"[A]\nK=9\r\n"
        );
        // Only the first section of a name, as the reads find it.
        assert_eq!(
            after_write(
                b"[A]\r\nx=1\r\n[A]\r\nx=5\r\n",
                b"a",
                Some(b"x"),
                Some(b"2")
            ),
            b"[A]\r\nx=2\r\n[A]\r\nx=5\r\n"
        );
    }

    /// A new key goes after its section's last key line, before comments and blank lines; a
    /// commented line is never the key.
    #[test]
    fn a_new_key_follows_its_sections_last_key() {
        let added = after_write(TEXT, b"GamePlay", Some(b"iKill"), Some(b"0"));
        assert_eq!(
            added,
            b"[Display]\r\niSize W=800\r\n\r\n[GamePlay]\r\n;iKill=9\r\nbSaveOnRest=1\r\niKill=0\r\n\r\n[Audio]\r\nfVol=1\r\n"
        );
        assert_eq!(ini_value(&added, b"GamePlay", b"iKill").unwrap(), b"0");
        assert_eq!(
            after_write(
                b"[A]\r\nx=1\r\n; note\r\n\r\n[B]\r\n",
                b"A",
                Some(b"y"),
                Some(b"2")
            ),
            b"[A]\r\nx=1\r\ny=2\r\n; note\r\n\r\n[B]\r\n"
        );
        assert_eq!(
            after_write(b"[A]\r\n[B]\r\n", b"A", Some(b"y"), Some(b"2")),
            b"[A]\r\ny=2\r\n[B]\r\n"
        );
    }

    /// A missing section goes at the end, after a line end when the text has none.
    #[test]
    fn a_new_section_is_appended() {
        assert!(after_write(TEXT, b"Controls", Some(b"fSens"), Some(b"0.5"))
            .ends_with(b"fVol=1\r\n[Controls]\r\nfSens=0.5\r\n"));
        assert_eq!(
            after_write(
                b"[General]\r\nsA=1",
                b"Controls",
                Some(b"Back"),
                Some(b"1F")
            ),
            b"[General]\r\nsA=1\r\n[Controls]\r\nBack=1F\r\n"
        );
    }

    /// A null value deletes the key's line; a null key the section; nothing to delete leaves
    /// the file as it was.
    #[test]
    fn a_null_value_deletes_the_key_and_a_null_key_the_section() {
        assert_eq!(
            after_write(TEXT, b"GamePlay", Some(b"bSaveOnRest"), None),
            b"[Display]\r\niSize W=800\r\n\r\n[GamePlay]\r\n;iKill=9\r\n\r\n[Audio]\r\nfVol=1\r\n"
        );
        assert_eq!(
            after_write(TEXT, b"GamePlay", None, None),
            b"[Display]\r\niSize W=800\r\n\r\n\r\n[Audio]\r\nfVol=1\r\n"
        );
        assert_eq!(after_write(TEXT, b"Nope", Some(b"k"), None), TEXT);
        assert_eq!(after_write(TEXT, b"Nope", None, None), TEXT);
    }

    /// A missing file in a folder that exists is made, with its section; a deletion makes it
    /// empty. Nothing reaches the host's disk.
    #[test]
    fn a_missing_file_in_an_existing_folder_is_made() {
        let mut e = engine_with_folder();
        assert_eq!(
            write(&mut e, b"Controls", Some(b"Back"), Some(b"1F"), PREFS),
            1
        );
        assert_eq!(
            files::read_whole(&e, PREFS).unwrap(),
            b"[Controls]\r\nBack=1F\r\n"
        );
        assert_eq!(read(&mut e, b"Controls", b"Back", PREFS), b"1F");
        let other = br"Z:\nv-rs-ini-test\Other.ini";
        assert_eq!(write(&mut e, b"Controls", Some(b"Back"), None, other), 1);
        assert_eq!(files::read_whole(&e, other).unwrap(), b"");
        assert!(!std::path::Path::new("/nv-rs-ini-test").exists());
        assert!(!std::path::Path::new(r"Z:\nv-rs-ini-test").exists());
    }

    /// A write reaches the run's copy of the file and the next read, stamped for the copy-out
    /// with its new size and times; the value the Settings menu saved is what the next read gets.
    #[test]
    fn a_profile_write_is_read_back_and_stamped() {
        let mut e = engine_with_folder();
        state!(e).disk.clock = Some(0x1234);
        let changes = state_ref!(e).disk.changes;
        assert_eq!(
            write(&mut e, b"GamePlay", Some(b"iKillCam"), Some(b"0"), PREFS),
            1
        );
        let k = br"z:\nv-rs-ini-test\falloutprefs.ini".to_vec();
        let stamp = {
            let disk = &state_ref!(e).disk;
            assert!(disk.changes > changes);
            assert_eq!(disk.files[&k], b"[GamePlay]\r\niKillCam=0\r\n");
            disk.stamps[&k]
        };
        let (s, key, f) = (
            put(&mut e, Some(b"GamePlay")),
            put(&mut e, Some(b"iKillCam")),
            put(&mut e, Some(PREFS)),
        );
        assert_eq!(get_private_profile_int_a(&mut e, s, key, 2, f), 0);
        state!(e).disk.clock = Some(0x5678);
        assert_eq!(
            write(&mut e, b"GamePlay", Some(b"iKillCam"), Some(b"2"), PREFS),
            1
        );
        assert_eq!(get_private_profile_int_a(&mut e, s, key, 0, f), 2);
        let disk = &state_ref!(e).disk;
        assert!(disk.stamps[&k] > stamp);
        let entry = &disk.entries[&k];
        assert_eq!(
            (entry.size, entry.created, entry.accessed, entry.written),
            (24, 0x1234, 0x5678, 0x5678)
        );
        assert_ne!(entry.attributes & files::FILE_ATTRIBUTE_ARCHIVE, 0);
        // The same value again changes nothing: no new stamp.
        let stamp = disk.stamps[&k];
        assert_eq!(
            write(&mut e, b"GamePlay", Some(b"iKillCam"), Some(b"2"), PREFS),
            1
        );
        assert_eq!(state_ref!(e).disk.stamps[&k], stamp);
    }

    /// A null section (the flush of Windows' cache) or a null file (`win.ini`) changes nothing
    /// and answers 1.
    #[test]
    fn a_null_section_or_file_changes_nothing() {
        let mut e = engine_with_folder();
        let (k, v, f) = (
            put(&mut e, Some(b"Back")),
            put(&mut e, Some(b"1F")),
            put(&mut e, Some(PREFS)),
        );
        assert_eq!(
            write_private_profile_string_a(&mut e, Ptr::NULL, k, v, f),
            1
        );
        let s = put(&mut e, Some(b"Controls"));
        assert_eq!(
            write_private_profile_string_a(&mut e, s, k, v, Ptr::NULL),
            1
        );
        assert_eq!(files::read_whole(&e, PREFS), None);
    }

    /// The game path of a host path: the path itself on Windows, under `Z:` elsewhere.
    fn game_path(path: &std::path::Path) -> Vec<u8> {
        let text = path.to_string_lossy().into_owned();
        if cfg!(windows) {
            text.into_bytes()
        } else {
            format!("Z:{}", text.replace('/', "\\")).into_bytes()
        }
    }

    /// The player's own file is read once and never written: the change is in the run's copy,
    /// which keeps the player's other lines; the file on disk keeps its bytes.
    #[test]
    fn the_players_file_is_never_written() {
        let dir = std::env::temp_dir().join(format!("nv-rs-ini-player-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let host = dir.join("FalloutPrefs.ini");
        let before = b"[GamePlay]\r\niKillCam=2\r\nbSaveOnRest=1\r\n";
        std::fs::write(&host, before).unwrap();
        let mut path = game_path(&dir);
        path.extend_from_slice(b"\\FalloutPrefs.ini");
        let mut e = engine();
        let written = write(&mut e, b"GamePlay", Some(b"iKillCam"), Some(b"0"), &path);
        let run = files::read_whole(&e, &path);
        let on_disk = std::fs::read(&host).unwrap();
        let listed = std::fs::read_dir(&dir).unwrap().count();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(written, 1);
        assert_eq!(
            run.unwrap(),
            b"[GamePlay]\r\niKillCam=0\r\nbSaveOnRest=1\r\n"
        );
        assert_eq!(on_disk, before);
        assert_eq!(listed, 1, "nothing made beside it");
    }

    /// A UTF-16 LE file is changed as text and written back as UTF-16 LE; a file with a 0 byte
    /// that is not one is left as it is (0, `ERROR_NOT_SUPPORTED`).
    #[test]
    fn a_utf16_file_stays_utf16_and_a_binary_file_is_left_alone() {
        let utf16 = |text: &str| {
            let mut out = vec![0xff, 0xfe];
            for unit in text.encode_utf16() {
                out.extend_from_slice(&unit.to_le_bytes());
            }
            out
        };
        assert_eq!(
            after_write(&utf16("[A]\r\nx=1\r\n"), b"A", Some(b"x"), Some(b"2")),
            utf16("[A]\r\nx=2\r\n")
        );
        let mut e = engine_with_folder();
        files::write_whole(&mut e, PREFS, b"[A]\r\nx=1\0\r\n".to_vec());
        assert_eq!(write(&mut e, b"A", Some(b"x"), Some(b"2"), PREFS), 0);
        assert_eq!(e.kernel.last_error, ERROR_NOT_SUPPORTED);
        assert_eq!(files::read_whole(&e, PREFS).unwrap(), b"[A]\r\nx=1\0\r\n");
        // A UTF-16 file that is only its mark: a key write puts a line end first (measured, as
        // the mod files' writes); a deletion is not one of the measured cases and is refused.
        assert_eq!(
            after_write(&utf16(""), b"A", Some(b"x"), Some(b"1")),
            utf16("\r\n[A]\r\nx=1\r\n")
        );
        let mut e = engine_with_folder();
        files::write_whole(&mut e, PREFS, utf16(""));
        assert_eq!(write(&mut e, b"A", Some(b"x"), None, PREFS), 0);
        assert_eq!(write(&mut e, b"A", None, None, PREFS), 0);
        assert_eq!(files::read_whole(&e, PREFS).unwrap(), utf16(""));
    }

    /// Writes one after another build on the run's copy: each key the game wrote stays; a file
    /// the game deleted in this run is made again with the new key only; an empty value is a
    /// value (`x=`), not a deletion.
    #[test]
    fn writes_build_on_the_runs_copy() {
        let mut e = engine_with_folder();
        assert_eq!(write(&mut e, b"A", Some(b"x"), Some(b"1"), PREFS), 1);
        assert_eq!(write(&mut e, b"A", Some(b"y"), Some(b"2"), PREFS), 1);
        assert_eq!(write(&mut e, b"B", Some(b"z"), Some(b""), PREFS), 1);
        assert_eq!(
            files::read_whole(&e, PREFS).unwrap(),
            b"[A]\r\nx=1\r\ny=2\r\n[B]\r\nz=\r\n"
        );
        let name = put(&mut e, Some(PREFS));
        assert_eq!(files::delete_file_a(&mut e, name), 1);
        assert_eq!(files::read_whole(&e, PREFS), None);
        assert_eq!(write(&mut e, b"A", Some(b"y"), Some(b"3"), PREFS), 1);
        assert_eq!(files::read_whole(&e, PREFS).unwrap(), b"[A]\r\ny=3\r\n");
    }
}
