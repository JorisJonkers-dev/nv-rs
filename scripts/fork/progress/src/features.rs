// SPDX-License-Identifier: GPL-3.0-only
//! Engine features: the graphics plan's capabilities (`docs/fork/graphics-features.tsv`, the
//! reference list) against the settings that declare them in code (`crates/fork-gfx/src/atlas.rs`,
//! whose own test checks every key is a real setting and every id is in the list). A capability
//! is *done* when a setting declares it and a test names that setting (`module.setting`, or the
//! setting's name quoted in a test that also names its module), *declared* when only the
//! declaration is there, *open* otherwise.

use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capability {
    pub id: String,
    pub group: String,
    pub name: String,
}

/// The reference list: `id<TAB>group<TAB>name` rows after `#` comments and the header.
pub fn read_list(tsv: &str) -> Result<Vec<Capability>, String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for (i, l) in tsv.lines().enumerate() {
        if l.starts_with('#') || l.trim().is_empty() || l.starts_with("id\t") {
            continue;
        }
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() < 3 || f[0].is_empty() {
            return Err(format!(
                "graphics-features.tsv line {}: want id, group, name",
                i + 1
            ));
        }
        if !seen.insert(f[0].to_string()) {
            return Err(format!(
                "graphics-features.tsv line {}: {} twice",
                i + 1,
                f[0]
            ));
        }
        out.push(Capability {
            id: f[0].into(),
            group: f[1].into(),
            name: f[2].into(),
        });
    }
    if out.is_empty() {
        return Err("graphics-features.tsv: no capability".into());
    }
    Ok(out)
}

/// The declarations of `atlas.rs`: `("module.setting", &["id", ...])`, by setting key.
pub fn read_declarations(atlas_rs: &str) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut rest = atlas_rs;
    while let Some(k) = rest.find("(\"") {
        rest = &rest[k + 2..];
        let Some(end) = rest.find('"') else { break };
        let key = rest[..end].to_string();
        let after = rest[end + 1..].trim_start();
        let Some(after) = after.strip_prefix(',') else {
            continue;
        };
        let Some(list) = after.trim_start().strip_prefix("&[") else {
            continue;
        };
        let Some(close) = list.find(']') else { break };
        let ids: Vec<String> = list[..close]
            .split(',')
            .map(|s| s.trim().trim_matches('"').to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if key.contains('.') {
            out.entry(key).or_default().extend(ids);
        }
    }
    out
}

/// A test text names the setting `module.setting`.
pub fn names_setting(text: &str, key: &str) -> bool {
    let Some((module, setting)) = key.split_once('.') else {
        return false;
    };
    let word = |w: &str| {
        text.match_indices(w).any(|(i, _)| {
            let before = text[..i].chars().next_back();
            let after = text[i + w.len()..].chars().next();
            let edge = |c: Option<char>| !c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
            edge(before) && edge(after)
        })
    };
    word(key)
        || (text.contains(&format!("\"{setting}\""))
            && (text.contains(&format!("\"{module}\""))
                || text.contains(&format!("[{module}]"))
                || text.contains(&format!(".{module}]"))))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Features {
    pub total: usize,
    /// (capability id, the setting keys that declare it), done and declared-only.
    pub done: Vec<(String, Vec<String>)>,
    pub declared: Vec<(String, Vec<String>)>,
    pub open: Vec<String>,
}

pub fn classify(
    list: &[Capability],
    decls: &BTreeMap<String, Vec<String>>,
    tests: &[(String, String)],
) -> Result<Features, String> {
    let ids: BTreeSet<&str> = list.iter().map(|c| c.id.as_str()).collect();
    let mut by_id: BTreeMap<&str, (Vec<String>, bool)> = BTreeMap::new();
    for (key, declared) in decls {
        let tested = tests.iter().any(|(_, t)| names_setting(t, key));
        for id in declared {
            if !ids.contains(id.as_str()) {
                return Err(format!(
                    "{key} declares {id}, which docs/fork/graphics-features.tsv does not list"
                ));
            }
            let e = by_id.entry(id.as_str()).or_default();
            e.0.push(key.clone());
            e.1 |= tested;
        }
    }
    let (mut done, mut declared, mut open) = (Vec::new(), Vec::new(), Vec::new());
    for c in list {
        match by_id.get(c.id.as_str()) {
            Some((keys, true)) => done.push((c.id.clone(), keys.clone())),
            Some((keys, false)) => declared.push((c.id.clone(), keys.clone())),
            None => open.push(c.id.clone()),
        }
    }
    Ok(Features {
        total: list.len(),
        done,
        declared,
        open,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = "# The plan.\nid\tgroup\tname\nao\tpost\tAmbient occlusion\naniso\tmaterials\tTexture filtering\nsky\tworld\tSky\n";

    #[test]
    fn done_declared_open() {
        let list = read_list(LIST).unwrap();
        let decls = read_declarations(
            "pub static ATLAS: &[(&str, &[&str])] = &[\n    (\"ao.enabled\", &[\"ao\"]),\n    (\"textures.anisotropy\", &[\"aniso\"]),\n];\n",
        );
        assert_eq!(decls.len(), 2);
        let tests = vec![(
            "t.rs".to_string(),
            "#[test] fn t() { env(\"NV_GFX\", \"ao.enabled=false\"); }".to_string(),
        )];
        let f = classify(&list, &decls, &tests).unwrap();
        assert_eq!(f.done, [("ao".to_string(), vec!["ao.enabled".to_string()])]);
        assert_eq!(f.declared.len(), 1);
        assert_eq!(f.open, ["sky"]);
        let bad = read_declarations("(\"x.y\", &[\"nope\"]),");
        assert!(
            classify(&list, &bad, &tests).is_err(),
            "an id not in the list"
        );
        assert!(read_list("id\tgroup\tname\nao\tpost\tA\nao\tpost\tB\n").is_err());
    }

    #[test]
    fn a_test_names_a_setting() {
        assert!(names_setting(
            "NV_GFX=water.placed_reflections=high",
            "water.placed_reflections"
        ));
        assert!(!names_setting(
            "water.placed_reflections_old",
            "water.placed_reflections"
        ));
        assert!(names_setting(
            "[global.textures]\n x.choice(\"anisotropy\")",
            "textures.anisotropy"
        ));
        assert!(!names_setting(
            "x.choice(\"anisotropy\")",
            "textures.anisotropy"
        ));
    }
}
