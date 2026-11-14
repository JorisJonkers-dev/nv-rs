//! The gate in front of [`crate::ANSWERS_INSTALLED`] (ADR-0016): the minimum set of JIP LN NVSE
//! commands that must have a handler before nv-rs answers "installed". The set is a data file,
//! `data/minimum-set.tsv`, names only with the source of each entry (the usage scan, or the gap
//! analysis until a scan exists); it is versioned in the repository. The tests below fail while
//! the flag is on and a name in the set has no handler, so the flag cannot be turned on early.

use crate::slots::SLOTS;

/// The set's file.
pub const SET: &str = include_str!("../data/minimum-set.tsv");

/// One entry of the set.
#[derive(Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: &'static str,
    /// `scan` (the usage scan, measured) or `analysis` (the gap analysis' estimate).
    pub source: &'static str,
    pub group: &'static str,
}

/// The set's entries, in file order.
pub fn entries() -> Vec<Entry> {
    SET.lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let mut cols = line.split('\t');
            let mut col = || cols.next().unwrap_or_default();
            Entry {
                name: col(),
                source: col(),
                group: col(),
            }
        })
        .collect()
}

/// The set's version (its `# version N` line).
pub fn version() -> u32 {
    SET.lines()
        .find_map(|line| line.strip_prefix("# version "))
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(0)
}

/// The opcode of the JIP command `name` (its long name or its alias, case does not matter).
pub fn opcode_of(name: &str) -> Option<u32> {
    SLOTS
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(name) || s.alias.eq_ignore_ascii_case(name))
        .map(|s| s.opcode)
}

/// Whether `opcode` has a handler once JIP answers "installed": a built command, the identity
/// commands and the event commands of the hooks.
pub fn has_handler(opcode: u32) -> bool {
    crate::commands::is_built(opcode)
        || opcode == 0x2200
        || opcode == 0x2210
        || opcode == crate::main_loop::SET_GAME_MAIN_LOOP_CALLBACK
        || crate::ui_hooks::COMMANDS.contains(&opcode)
        || crate::actor_hooks::COMMANDS.contains(&opcode)
        || crate::ai_hooks::COMMANDS.contains(&opcode)
}

/// The names in the set that have no handler yet.
pub fn missing() -> Vec<&'static str> {
    entries()
        .into_iter()
        .filter(|entry| opcode_of(entry.name).map_or(true, |o| !has_handler(o)))
        .map(|entry| entry.name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_set_is_well_formed() {
        let entries = entries();
        assert!(version() >= 1, "the file says its version");
        assert!(!entries.is_empty());
        let mut seen = std::collections::HashSet::new();
        for entry in &entries {
            assert!(
                matches!(entry.source, "scan" | "analysis"),
                "{}: the source is `scan` or `analysis`",
                entry.name
            );
            assert!(!entry.group.is_empty(), "{}: a group", entry.name);
            assert!(
                opcode_of(entry.name).is_some(),
                "{}: not a JIP command",
                entry.name
            );
            assert!(
                seen.insert(entry.name.to_ascii_lowercase()),
                "{}: twice",
                entry.name
            );
        }
    }

    /// The gate. It fails when a name in the set has no handler while JIP answers "installed";
    /// while the flag is off it only keeps the count in view.
    #[test]
    fn jip_answers_installed_only_with_the_whole_set() {
        let missing = missing();
        if crate::ANSWERS_INSTALLED {
            assert!(
                missing.is_empty(),
                "ANSWERS_INSTALLED is on but {} commands of the minimum set have no handler: {missing:?}",
                missing.len()
            );
        } else {
            eprintln!(
                "minimum set: {} of {} built; missing {missing:?}",
                entries().len() - missing.len(),
                entries().len()
            );
        }
    }
}
