// SPDX-License-Identifier: GPL-3.0-only
//! Which capabilities of the graphics plan each built-in setting implements: the plan's ids are the
//! rows of `docs/fork/graphics-features.tsv`. The engine-features progress bar
//! (`scripts/fork/progress`, docs/fork/PROGRESS.md) counts a capability as done when a setting
//! here declares it and a test names that setting. A new setting that implements a capability adds
//! its row here in the same pull request; the tests below refuse a key that is no setting and an id
//! the plan does not list.

/// (`module.setting`, the plan's capability ids it implements).
pub static ATLAS: &[(&str, &[&str])] = &[
    ("ao.enabled", &["ao"]),
    ("shadows.enabled", &["shadows"]),
    ("light.lamp_distance", &["night-light"]),
    ("water.placed_reflections", &["reflections"]),
    ("textures.anisotropy", &["aniso"]),
];

#[cfg(test)]
mod tests {
    use super::ATLAS;
    use std::collections::BTreeSet;

    const PLAN: &str = include_str!("../../../docs/fork/graphics-features.tsv");

    #[test]
    fn every_declared_key_is_a_built_in_setting() {
        let keys: BTreeSet<String> = crate::builtin::specs()
            .iter()
            .flat_map(|m| {
                m.settings
                    .iter()
                    .map(move |s| format!("{}.{}", m.id, s.key))
            })
            .collect();
        for (key, _) in ATLAS {
            assert!(keys.contains(*key), "{key} is no built-in setting");
        }
    }

    #[test]
    fn every_declared_id_is_in_the_plan_once() {
        let ids: BTreeSet<&str> = PLAN
            .lines()
            .filter(|l| !l.starts_with('#') && !l.starts_with("id\t") && !l.is_empty())
            .map(|l| l.split('\t').next().unwrap_or(""))
            .collect();
        assert!(ids.len() > 50, "the plan reads as {} rows", ids.len());
        let mut keys = BTreeSet::new();
        for (key, declared) in ATLAS {
            assert!(keys.insert(*key), "{key} is declared twice");
            assert!(!declared.is_empty(), "{key} declares nothing");
            for id in *declared {
                assert!(
                    ids.contains(id),
                    "{key} declares {id}, which the plan does not list"
                );
            }
        }
    }
}
