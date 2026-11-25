// SPDX-License-Identifier: GPL-3.0-only
//! The engine features of the coverage catalogue (area `features`, docs/fork/ENGINE-FEATURES.md):
//! what players and modders know the engine by (LOD drawing, the Pip-Boy map, V.A.T.S., save games),
//! each one a set of the exe's compile units, and how much of each runs as native typed code.
//!
//! Two hand-kept files say what a feature is: `.fork/catalogue/features/features.tsv` (`id`, `group`,
//! `title`) and `.fork/catalogue/features/units.tsv` (`unit`, `feature`: every compile unit of the
//! ledger that belongs to a feature, a unit to at most one). From them and the ledger this module
//! writes one reference list per group (`ref/features_<group>.tsv`) and the evidence rows tally reads:
//! a feature is `implemented` only when every one of its functions is native, else `translated` (it
//! works, through the address-level translation of the game's own code), with its share
//! `native/translated/total` in functions.
//!
//! A function counts when its ledger status is `translated` and its subsystem is not the compiler's
//! static initialisers; it is native when the ledger finds a typed version (`typed`) and no family
//! that is off by default is declared in its marker's folder or a folder above it.

use crate::catalogue::segment;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The feature list, relative to the repository root.
pub const FEATURES: &str = ".fork/catalogue/features/features.tsv";
/// The unit map, relative to the repository root.
pub const UNITS: &str = ".fork/catalogue/features/units.tsv";

/// One engine feature: `features.<group>.<id>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feature {
    pub id: String,
    pub group: String,
    pub title: String,
}

impl Feature {
    pub fn key(&self) -> String {
        format!("{}.{}", self.group, self.id)
    }
}

/// How much of a feature is native: functions with a typed version that is on, of all its counted
/// functions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Share {
    pub native: usize,
    pub total: usize,
}

/// Rows of a hand-kept TSV: `#` lines and empty lines skipped, the first other line the header,
/// which must be `want`.
fn rows<'a>(what: &str, text: &'a str, want: &[&str]) -> Result<Vec<Vec<&'a str>>, String> {
    let mut lines = text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'));
    let head: Vec<&str> = lines.next().unwrap_or("").split('\t').collect();
    if head != want {
        return Err(format!("{what}: the header is not {}", want.join(" ")));
    }
    lines
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            if f.len() != want.len() || f.iter().any(|c| c.trim().is_empty()) {
                return Err(format!(
                    "{what}: the row {l:?} does not have {} cells",
                    want.len()
                ));
            }
            Ok(f)
        })
        .collect()
}

/// The feature list, in its order: ids and groups are id segments, an id is listed once per group.
pub fn read_features(text: &str) -> Result<Vec<Feature>, String> {
    let mut out: Vec<Feature> = Vec::new();
    let mut seen = BTreeSet::new();
    for f in rows(FEATURES, text, &["id", "group", "title"])? {
        let (id, group, title) = (f[0], f[1], f[2]);
        for s in [id, group] {
            if segment(s) != s {
                return Err(format!("{FEATURES}: `{s}` is not an id segment"));
            }
        }
        if !seen.insert(format!("{group}.{id}")) {
            return Err(format!("{FEATURES}: `{group}.{id}` is listed twice"));
        }
        out.push(Feature {
            id: id.into(),
            group: group.into(),
            title: title.into(),
        });
    }
    if out.is_empty() {
        return Err(format!("{FEATURES}: no feature"));
    }
    Ok(out)
}

/// The unit map: unit to `<group>.<id>`. A unit is listed once and names a listed feature.
pub fn read_units(text: &str, features: &[Feature]) -> Result<BTreeMap<String, String>, String> {
    let known: BTreeSet<String> = features.iter().map(Feature::key).collect();
    let mut out = BTreeMap::new();
    for f in rows(UNITS, text, &["unit", "feature"])? {
        if !known.contains(f[1]) {
            return Err(format!(
                "{UNITS}: `{}` is not a feature of {FEATURES}",
                f[1]
            ));
        }
        if out.insert(f[0].to_string(), f[1].to_string()).is_some() {
            return Err(format!("{UNITS}: `{}` is listed twice", f[0]));
        }
    }
    Ok(out)
}

/// What the features count, read from the ledger's TSV.
#[derive(Debug, Default)]
pub struct Counted {
    /// By `<group>.<id>`.
    pub shares: BTreeMap<String, Share>,
    /// Counted functions in no feature's unit.
    pub unmapped: usize,
    /// Mapped units the ledger does not have.
    pub unknown_units: Vec<String>,
}

/// Counts every feature's functions. `off` holds the addresses whose typed version belongs to a
/// family that is off by default ([`off_addresses`]).
pub fn count(
    ledger_tsv: &str,
    units: &BTreeMap<String, String>,
    off: &BTreeSet<u32>,
) -> Result<Counted, String> {
    let mut lines = ledger_tsv.lines();
    let head: Vec<&str> = lines.next().unwrap_or("").split('\t').collect();
    let col = |n: &str| {
        head.iter()
            .position(|c| *c == n)
            .ok_or(format!("ledger TSV: no column {n}"))
    };
    let (addr, sub, unit, status, typed) = (
        col("address")?,
        col("subsystem")?,
        col("unit")?,
        col("status")?,
        col("typed")?,
    );
    let mut out = Counted::default();
    let mut seen_units = BTreeSet::new();
    for l in lines.filter(|l| !l.is_empty()) {
        let f: Vec<&str> = l.split('\t').collect();
        let get = |k: usize| f.get(k).copied().unwrap_or("").trim();
        seen_units.insert(get(unit));
        if get(status) != "translated" || get(sub).starts_with("compiler:") {
            continue;
        }
        let Some(feature) = units.get(get(unit)) else {
            out.unmapped += 1;
            continue;
        };
        let a = u32::from_str_radix(get(addr).trim_start_matches("0x"), 16)
            .map_err(|_| format!("ledger TSV: address {:?}", get(addr)))?;
        let s = out.shares.entry(feature.clone()).or_default();
        s.total += 1;
        if !get(typed).is_empty() && !off.contains(&a) {
            s.native += 1;
        }
    }
    out.unknown_units = units
        .keys()
        .filter(|u| !seen_units.contains(u.as_str()))
        .cloned()
        .collect();
    Ok(out)
}

/// Whether a family declared in `text` (a `static NAME: Family = Family { ... }` before the file's
/// test module) is off by default: its `past_gate` is anything but `true`.
fn declares_off_family(text: &str) -> bool {
    let code = text.split("\n#[cfg(test)]").next().unwrap_or("");
    let mut rest = code;
    while let Some(at) = rest.find(": Family = Family {") {
        let body = &rest[at..];
        let end = body
            .find("\n};")
            .or_else(|| body.find("};"))
            .unwrap_or(body.len());
        // Anything but a plain `past_gate: true` (false, an expression, a missing field) counts as off, so
        // a family is never taken as on by mistake.
        let gate: String = body[..end]
            .lines()
            .find(|l| l.contains("past_gate"))
            .unwrap_or("")
            .split_whitespace()
            .collect();
        if gate != "past_gate:true," && gate != "past_gate:true" {
            return true;
        }
        rest = &body[end..];
    }
    false
}

/// The marker address of a line: `// Typed from <8 hex digits>` (the ledger's rule, without its
/// tail check: a malformed marker is refused by the hygiene test anyway).
fn marker(line: &str) -> Option<u32> {
    let rest = line.trim_start().strip_prefix("// Typed from ")?;
    let hex = rest.get(..8)?;
    if !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(hex, 16).ok()
}

fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

/// The addresses whose typed version is marked in the folder of a family that is off by default, or
/// below it, in the `src` folder of any crate.
pub fn off_addresses(root: &Path) -> BTreeSet<u32> {
    let mut files = Vec::new();
    if let Ok(crates) = std::fs::read_dir(root.join("crates")) {
        let mut crates: Vec<_> = crates.flatten().map(|e| e.path().join("src")).collect();
        crates.sort();
        for c in crates {
            rust_files(&c, &mut files);
        }
    }
    let texts: Vec<(std::path::PathBuf, String)> = files
        .into_iter()
        .filter_map(|p| std::fs::read_to_string(&p).ok().map(|t| (p, t)))
        .collect();
    let off_dirs: Vec<&Path> = texts
        .iter()
        .filter(|(_, t)| declares_off_family(t))
        .filter_map(|(p, _)| p.parent())
        .collect();
    let mut out = BTreeSet::new();
    for (p, t) in &texts {
        if off_dirs.iter().any(|d| p.starts_with(d)) {
            out.extend(t.lines().filter_map(marker));
        }
    }
    out
}

/// One reference list per group, in the feature list's order: `ref/features_<group>.tsv` with
/// `key<TAB>title<TAB>group`.
pub fn ref_lists(features: &[Feature]) -> Result<Vec<(String, String)>, String> {
    let mut groups: Vec<&str> = Vec::new();
    for f in features {
        if !groups.contains(&f.group.as_str()) {
            groups.push(&f.group);
        }
    }
    groups
        .into_iter()
        .map(|g| {
            let rows = features
                .iter()
                .filter(|f| f.group == g)
                .map(|f| (f.id.clone(), f.title.clone(), f.group.clone()));
            Ok((
                format!("ref/features_{g}.tsv"),
                crate::catalogue::list(&format!("features {g}"), "group", rows)?,
            ))
        })
        .collect()
}

/// The evidence rows: `features.<group>.<id>`, `implemented` when every counted function is native,
/// else `translated`, and the share `native/translated/total`. A feature with no counted function is
/// an error: its units are wrong.
pub fn evidence(features: &[Feature], counted: &Counted) -> Result<String, String> {
    let mut out = String::new();
    for f in features {
        let key = f.key();
        let s = counted.shares.get(&key).copied().unwrap_or_default();
        if s.total == 0 {
            return Err(format!(
                "feature `{key}` has no counted function: check {UNITS}"
            ));
        }
        let state = if s.native == s.total {
            "implemented"
        } else {
            "translated"
        };
        out.push_str(&format!(
            "features.{key}\t{state}\t{}/{}/{}\n",
            s.native,
            s.total - s.native,
            s.total
        ));
    }
    Ok(out)
}

/// Everything the catalogue command needs from the engine features: the reference lists and the
/// evidence rows, after checking that every mapped unit is in the ledger.
pub fn read(root: &Path, ledger_tsv: &str) -> Result<(Vec<(String, String)>, String), String> {
    let text =
        |rel: &str| std::fs::read_to_string(root.join(rel)).map_err(|e| format!("{rel}: {e}"));
    let features = read_features(&text(FEATURES)?)?;
    let units = read_units(&text(UNITS)?, &features)?;
    let counted = count(ledger_tsv, &units, &off_addresses(root))?;
    if let Some(u) = counted.unknown_units.first() {
        return Err(format!(
            "{UNITS}: unit `{u}` is not in the ledger ({} such units)",
            counted.unknown_units.len()
        ));
    }
    Ok((ref_lists(&features)?, evidence(&features, &counted)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEATURES_TSV: &str = "# SPDX-License-Identifier: GPL-3.0-only\nid\tgroup\ttitle\n\
        sky\trendering\tSky\nlod_terrain\trendering\tLOD drawing: terrain\nhud\tui\tHUD\n";
    const UNITS_TSV: &str = "unit\tfeature\nfallout shared/sky/sky.cpp\trendering.sky\n\
        fallout shared/distant terrain system/bgsterrainchunk.cpp\trendering.lod_terrain\n\
        fallout/interface/menus/hudmainmenu.cpp\tui.hud\n";
    const LEDGER: &str = "address\tsize\tname\tsubsystem\tunit\tstatus\trust\ttyped\n\
        00401000\t4\t\tfallout shared/sky\tfallout shared/sky/sky.cpp\ttranslated\tx\tyes\n\
        00401010\t4\t\tfallout shared/sky\tfallout shared/sky/sky.cpp\ttranslated\tx\t\n\
        00401020\t4\t\tcompiler: static init/exit\tfallout shared/sky/sky.cpp\ttranslated\tx\t\n\
        00401030\t4\t\tfallout shared/distant terrain system\tfallout shared/distant terrain system/bgsterrainchunk.cpp\ttranslated\tx\tyes\n\
        00401040\t4\t\tfallout/interface\tfallout/interface/menus/hudmainmenu.cpp\ttranslated\tx\tyes\n\
        00401050\t4\t\tfallout/interface\tfallout/interface/menus/hudmainmenu.cpp\tstatic init (table)\t\t\n\
        00401060\t4\t\tfallout/misc\tfallout/misc/main.cpp\ttranslated\tx\t\n";

    fn features() -> Vec<Feature> {
        read_features(FEATURES_TSV).unwrap()
    }

    #[test]
    fn a_feature_is_implemented_only_when_every_counted_function_is_native() {
        let units = read_units(UNITS_TSV, &features()).unwrap();
        let c = count(LEDGER, &units, &BTreeSet::new()).unwrap();
        // The static initialiser and the table-run function are not counted; main.cpp is in no feature.
        assert_eq!(c.unmapped, 1);
        assert!(c.unknown_units.is_empty());
        assert_eq!(
            evidence(&features(), &c).unwrap(),
            "features.rendering.sky\ttranslated\t1/1/2\n\
             features.rendering.lod_terrain\timplemented\t1/0/1\n\
             features.ui.hud\timplemented\t1/0/1\n"
        );
    }

    #[test]
    fn a_typed_function_of_a_family_that_is_off_is_not_native() {
        let units = read_units(UNITS_TSV, &features()).unwrap();
        let c = count(LEDGER, &units, &BTreeSet::from([0x0040_1040])).unwrap();
        assert_eq!(
            c.shares["ui.hud"],
            Share {
                native: 0,
                total: 1
            }
        );
        assert!(evidence(&features(), &c)
            .unwrap()
            .contains("features.ui.hud\ttranslated\t0/1/1\n"));
    }

    #[test]
    fn a_feature_without_functions_and_a_unit_the_ledger_lacks_are_errors() {
        let units = read_units(
            &format!("{UNITS_TSV}fallout/gone.cpp\tui.hud\n"),
            &features(),
        )
        .unwrap();
        let c = count(LEDGER, &units, &BTreeSet::new()).unwrap();
        assert_eq!(c.unknown_units, vec!["fallout/gone.cpp".to_string()]);
        let only_sky = read_units(
            "unit\tfeature\nfallout shared/sky/sky.cpp\trendering.sky\n",
            &features(),
        )
        .unwrap();
        let c = count(LEDGER, &only_sky, &BTreeSet::new()).unwrap();
        assert!(evidence(&features(), &c)
            .unwrap_err()
            .contains("rendering.lod_terrain"));
    }

    #[test]
    fn malformed_lists_are_errors() {
        assert!(read_features("id\tgroup\ttitle\n").is_err());
        assert!(read_features("id\tgroup\nsky\trendering\n").is_err());
        assert!(read_features("id\tgroup\ttitle\nSky\trendering\tSky\n").is_err());
        assert!(
            read_features("id\tgroup\ttitle\nsky\trendering\tSky\nsky\trendering\tSky 2\n")
                .is_err()
        );
        assert!(read_features("id\tgroup\ttitle\nsky\trendering\t \n").is_err());
        let f = features();
        assert!(read_units("unit\tfeature\na.cpp\trendering.nope\n", &f).is_err());
        assert!(read_units("unit\tfeature\na.cpp\tui.hud\na.cpp\tui.hud\n", &f).is_err());
    }

    #[test]
    fn one_reference_list_per_group_in_list_order() {
        let lists = ref_lists(&features()).unwrap();
        assert_eq!(
            lists,
            vec![
                (
                    "ref/features_rendering.tsv".to_string(),
                    "key\ttitle\tgroup\nsky\tSky\trendering\nlod_terrain\tLOD drawing: terrain\trendering\n".to_string()
                ),
                ("ref/features_ui.tsv".to_string(), "key\ttitle\tgroup\nhud\tHUD\tui\n".to_string()),
            ]
        );
    }

    #[test]
    fn only_families_off_before_the_test_module_count() {
        let on = "pub static A: Family = Family {\n    name: \"a\",\n    past_gate: true,\n};\n";
        let off = "pub static B: Family = Family {\n    name: \"b\",\n    past_gate: false,\n};\n";
        assert!(!declares_off_family(on));
        assert!(declares_off_family(&format!("{on}{off}")));
        assert!(!declares_off_family(&format!(
            "{on}\n#[cfg(test)]\nmod tests {{\n{off}}}\n"
        )));
        assert_eq!(
            marker("    // Typed from 00a1b2c3, read against fnv_decomp"),
            Some(0x00a1_b2c3)
        );
        assert_eq!(marker("// Typed from 0xa1b2c3"), None);
        assert_eq!(marker("let x = 1; // Typed from 00a1b2c3"), None);
    }

    #[test]
    fn the_repositorys_own_families_and_feature_files_read() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        // The families of the engine read (a family that is off holds its typed functions back).
        let _ = off_addresses(&root);
        let f = read_features(&std::fs::read_to_string(root.join(FEATURES)).unwrap()).unwrap();
        read_units(&std::fs::read_to_string(root.join(UNITS)).unwrap(), &f).unwrap();
    }
}
