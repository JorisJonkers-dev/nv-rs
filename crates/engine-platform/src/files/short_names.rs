//! Windows-like 8.3 short names for hosts that keep none (everything but Windows).
//!
//! NTFS gives a long name that does not fit 8.3 a short one: the name upper-cased, spaces and
//! dots left out of the base, a character a short name cannot hold replaced by `_`, the base cut
//! to six characters and `~N` after it (`SAVE1~1.FOS`), the extension cut to three. After
//! four collisions the base is cut to two characters followed by four hex digits of a checksum
//! of the long name, then `~N`. [`generate`] makes one name against the names taken in the
//! directory; [`assign`] gives every name of a directory its short name, in sorted order, so a
//! directory's short names are the same on every run.

use std::collections::{BTreeMap, HashSet};

/// Characters a short name cannot hold, besides control characters and non-ASCII ones.
const ILLEGAL: &str = "\"/\\[]:;=,+*?<>|";

/// The short-name form of a part of a name: upper case, spaces and dots left out, an illegal or
/// non-ASCII character as `_`.
fn clean(part: &str) -> String {
    part.chars()
        .filter(|&c| c != ' ' && c != '.')
        .map(|c| {
            let c = c.to_ascii_uppercase();
            if !c.is_ascii() || c.is_ascii_control() || ILLEGAL.contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// A checksum of the long name, four hex digits (any fixed function of the name will do: it only
/// has to differ between names that share a prefix, and stay the same between runs).
fn checksum(name: &str) -> u16 {
    let mut hash: u32 = 0;
    for unit in name.to_ascii_uppercase().encode_utf16() {
        hash = hash.wrapping_mul(0x25).wrapping_add(u32::from(unit));
    }
    (hash ^ (hash >> 16)) as u16
}

/// The short name of `name` (a name in a directory, not a path) that no name in `taken` (upper
/// case; the long and short names the directory holds) already is: `""` for a name that needs
/// none. `None` only when `~1` to `~9999` are all taken.
pub fn generate(name: &str, taken: &HashSet<String>) -> Option<String> {
    if super::short_extension(name).is_none() {
        return Some(String::new());
    }
    // A leading dot is not an extension's: `.gitignore` is `GITIGN~1`.
    let trimmed = name.trim_start_matches('.');
    let (base, extension) = match trimmed.rfind('.') {
        Some(at) => (&trimmed[..at], &trimmed[at + 1..]),
        None => (trimmed, ""),
    };
    let base = clean(base);
    let base = if base.is_empty() {
        "_".to_string()
    } else {
        base
    };
    let extension: String = clean(extension).chars().take(3).collect();
    let suffix = if extension.is_empty() {
        String::new()
    } else {
        format!(".{extension}")
    };
    let candidate = |prefix: &str, n: u32| {
        let tail = format!("~{n}");
        let room = 8 - tail.len();
        let prefix: String = prefix.chars().take(room).collect();
        format!("{prefix}{tail}{suffix}")
    };
    for n in 1..=4 {
        let short = candidate(&base.chars().take(6).collect::<String>(), n);
        if !taken.contains(&short) {
            return Some(short);
        }
    }
    let head: String = base.chars().take(2).collect();
    let hashed = format!("{head}{:04X}", checksum(name));
    (1..=9999)
        .map(|n| candidate(&hashed, n))
        .find(|short| !taken.contains(short))
}

/// The short name of every name in `names` (a directory's entries) that needs one, in the order
/// of their upper-case names, each unique against every long and short name of the directory.
/// Names that need none are not in the answer.
pub fn assign(names: &[String]) -> BTreeMap<String, String> {
    let mut sorted: Vec<&String> = names.iter().collect();
    sorted.sort_by_key(|n| (n.to_ascii_uppercase(), n.to_string()));
    let mut taken: HashSet<String> = names.iter().map(|n| n.to_ascii_uppercase()).collect();
    let mut out = BTreeMap::new();
    for name in sorted {
        if let Some(short) = generate(name, &taken) {
            if !short.is_empty() {
                taken.insert(short.clone());
                out.insert(name.clone(), short);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_save_name_from_the_game() {
        let long = "Save 1   Courier  Doc Mitchell s House  00 04 40.fos";
        assert_eq!(
            generate(long, &HashSet::new()).as_deref(),
            Some("SAVE1C~1.FOS")
        );
    }

    #[test]
    fn short_names_need_none() {
        assert_eq!(generate("nvfix.fos", &HashSet::new()).as_deref(), Some(""));
        assert_eq!(generate("A.esm", &HashSet::new()).as_deref(), Some(""));
    }

    #[test]
    fn same_prefix_collides_into_counters() {
        let all = names(&["Save 1 a.fos", "Save 1 b.fos", "Save 1 c.fos"]);
        let got = assign(&all);
        assert_eq!(got["Save 1 a.fos"], "SAVE1A~1.FOS");
        let shorts: HashSet<&String> = got.values().collect();
        assert_eq!(shorts.len(), 3);
        let all = names(&["Save 10 a.fos", "Save 10 b.fos"]);
        let got = assign(&all);
        assert_eq!(got["Save 10 a.fos"], "SAVE10~1.FOS");
        assert_eq!(got["Save 10 b.fos"], "SAVE10~2.FOS");
    }

    #[test]
    fn after_four_collisions_a_checksum_name() {
        let all: Vec<String> = (0..6).map(|i| format!("Long Name Here {i}.txt")).collect();
        let got = assign(&all);
        assert_eq!(got.len(), 6);
        let mut shorts: Vec<&String> = got.values().collect();
        shorts.sort();
        shorts.dedup();
        assert_eq!(shorts.len(), 6);
        assert!(got.values().filter(|s| s.starts_with("LONGNA~")).count() == 4);
        for s in got.values() {
            assert!(s.len() <= 12 && s.ends_with(".TXT"), "{s}");
        }
    }

    #[test]
    fn no_extension_extension_cut_and_dotfiles() {
        let none = HashSet::new();
        assert_eq!(
            generate("A very long folder", &none).as_deref(),
            Some("AVERYL~1")
        );
        assert_eq!(generate("d.esmx", &none).as_deref(), Some("D~1.ESM"));
        assert_eq!(generate(".gitignore", &none).as_deref(), Some("GITIGN~1"));
        assert_eq!(
            generate("my.save.file.name.fos", &none).as_deref(),
            Some("MYSAVE~1.FOS")
        );
        assert_eq!(
            generate("a+b c[d].txt", &none).as_deref(),
            Some("A_BC_D~1.TXT")
        );
    }

    #[test]
    fn a_taken_long_name_is_not_reused_and_assignment_is_stable() {
        // "SAVE1~1.FOS" exists as a long name: the generated one steps over it.
        let all = names(&["SAVE1~1.FOS", "Save 1 x.fos"]);
        assert_eq!(assign(&all)["Save 1 x.fos"], "SAVE1X~1.FOS");
        let all = names(&["SAVE1~1.FOS", "Save 1.fos"]);
        let again: Vec<String> = all.iter().rev().cloned().collect();
        assert_eq!(assign(&all), assign(&again));
        assert_eq!(assign(&all)["Save 1.fos"], "SAVE1~2.FOS");
    }
}
