// SPDX-License-Identifier: GPL-3.0-only
//! The schema as JSON, for another program (raul's Settings page) that edits the same file: the
//! file's name and format version, its layers, the presets, and every module with its settings,
//! their types, ranges, options and original values. The engine prints it; raul reads it and does
//! not hard-code a setting.
//!
//! ```json
//! {"format":"nv-rs-graphics-schema","schema_version":1,"file":{"name":"nv-rs-graphics.toml","version":1,
//!  "layers":["global","worldspace","interior","cell"]},"presets":[{"id":"original","label":"Original"}],
//!  "modules":[{"id":"ao","label":"...","help":"...","cost":"...","owns":[{"switch":"NV_AO"}],
//!   "settings":[{"key":"quality","label":"...","help":"...","type":"choice","default":"off",
//!     "options":[{"id":"off","label":"Off"}]},
//!    {"key":"strength","type":"slider","min":0.1,"max":1,"step":0.05,"default":0.7}]}]}
//! ```
//!
//! `schema_version` changes only when a reader written for the old one would misread the new one;
//! new fields may be added without a change.

use crate::schema::{Kind, ModuleSpec, Owns};
use crate::settings::{Presets, FILE_NAME, LAYER_WORDS, VERSION};

/// The version of this JSON description.
pub const SCHEMA_VERSION: u32 = 1;

/// The description of `specs` and `presets` as one line of JSON.
pub fn schema_json(specs: &[&ModuleSpec], presets: &Presets) -> String {
    let mut o = String::new();
    o.push_str("{\"format\":\"nv-rs-graphics-schema\",\"schema_version\":");
    o.push_str(&SCHEMA_VERSION.to_string());
    o.push_str(",\"file\":{\"name\":");
    o.push_str(&string(FILE_NAME));
    o.push_str(",\"version\":");
    o.push_str(&VERSION.to_string());
    o.push_str(",\"layers\":[");
    o.push_str(
        &LAYER_WORDS
            .iter()
            .map(|l| string(l))
            .collect::<Vec<_>>()
            .join(","),
    );
    o.push_str("]},\"presets\":[");
    let presets: Vec<String> = presets
        .list()
        .iter()
        .map(|p| {
            format!(
                "{{\"id\":{},\"label\":{}}}",
                string(&p.id),
                string(&p.label)
            )
        })
        .collect();
    o.push_str(&presets.join(","));
    o.push_str("],\"modules\":[");
    let modules: Vec<String> = specs.iter().map(|m| module(m)).collect();
    o.push_str(&modules.join(","));
    o.push_str("]}");
    o
}

fn module(m: &ModuleSpec) -> String {
    let owns: Vec<String> = m
        .owns
        .iter()
        .map(|o| match o {
            Owns::Switch(s) => format!("{{\"switch\":{}}}", string(s)),
            Owns::GameValue(v) => format!("{{\"game_value\":{}}}", string(v)),
            Owns::Pass { name, at } => {
                format!("{{\"pass\":{},\"at\":{}}}", string(name), string(at.name()))
            }
        })
        .collect();
    let settings: Vec<String> = m
        .settings
        .iter()
        .map(|s| {
            let head = format!(
                "{{\"key\":{},\"label\":{},\"help\":{}",
                string(s.key),
                string(s.label),
                string(s.help)
            );
            let kind = match &s.kind {
                Kind::Toggle { default } => format!(",\"type\":\"toggle\",\"default\":{default}"),
                Kind::Slider {
                    min,
                    max,
                    step,
                    default,
                } => format!(
                    ",\"type\":\"slider\",\"min\":{},\"max\":{},\"step\":{},\"default\":{}",
                    number(*min),
                    number(*max),
                    number(*step),
                    number(*default)
                ),
                Kind::Choice { options, default } => format!(
                    ",\"type\":\"choice\",\"default\":{},\"options\":[{}]",
                    string(default),
                    options
                        .iter()
                        .map(|o| format!(
                            "{{\"id\":{},\"label\":{}}}",
                            string(o.id),
                            string(o.label)
                        ))
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            };
            format!("{head}{kind}}}")
        })
        .collect();
    format!(
        "{{\"id\":{},\"label\":{},\"help\":{},\"cost\":{},\"owns\":[{}],\"settings\":[{}]}}",
        string(m.id),
        string(m.label),
        string(m.help),
        string(m.cost),
        owns.join(","),
        settings.join(",")
    )
}

/// A JSON number. The schema's numbers are finite (`schema::check`).
fn number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{n:.0}")
    } else {
        format!("{n}")
    }
}

/// A JSON string.
fn string(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::tests::{AO, FOG};
    use crate::settings::{Preset, SettingsFile};

    #[test]
    fn the_description_names_every_setting_with_its_type() {
        let presets = Presets::new(vec![Preset {
            id: "modern".into(),
            label: "Modern".into(),
            file: SettingsFile::new(),
        }])
        .unwrap();
        let json = schema_json(&[&AO, &FOG], &presets);
        assert_eq!(
            json,
            concat!(
                "{\"format\":\"nv-rs-graphics-schema\",\"schema_version\":1,",
                "\"file\":{\"name\":\"nv-rs-graphics.toml\",\"version\":1,\"layers\":[\"global\",\"worldspace\",\"interior\",\"cell\"]},",
                "\"presets\":[{\"id\":\"original\",\"label\":\"Original\"},{\"id\":\"modern\",\"label\":\"Modern\"}],",
                "\"modules\":[",
                "{\"id\":\"ao\",\"label\":\"Ambient occlusion\",\"help\":\"Soft shade in corners.\",\"cost\":\"measured in docs/fork/GRAPHICS.md\",",
                "\"owns\":[{\"switch\":\"NV_AO\"},{\"pass\":\"gtao\",\"at\":\"after_world\"}],",
                "\"settings\":[",
                "{\"key\":\"quality\",\"label\":\"Quality\",\"help\":\"How many samples.\",\"type\":\"choice\",\"default\":\"off\",",
                "\"options\":[{\"id\":\"off\",\"label\":\"Off\"},{\"id\":\"low\",\"label\":\"Low\"},{\"id\":\"high\",\"label\":\"High\"}]},",
                "{\"key\":\"strength\",\"label\":\"Strength\",\"help\":\"How dark.\",\"type\":\"slider\",\"min\":0.1,\"max\":1,\"step\":0.05,\"default\":0.7}]},",
                "{\"id\":\"fog\",\"label\":\"Fog\",\"help\":\"The weather's distance fog.\",\"cost\":\"none\",",
                "\"owns\":[{\"game_value\":\"fFogEnd\"}],",
                "\"settings\":[",
                "{\"key\":\"enabled\",\"label\":\"Fog\",\"help\":\"Off: no distance fog.\",\"type\":\"toggle\",\"default\":true},",
                "{\"key\":\"distance\",\"label\":\"Distance\",\"help\":\"Scales the fog's distances.\",\"type\":\"slider\",\"min\":0.5,\"max\":4,\"step\":0.25,\"default\":1}]}",
                "]}"
            )
        );
        assert!(balanced(&json));
    }

    #[test]
    fn strings_are_escaped() {
        assert_eq!(string("a\"b\\c\nd\u{1}é"), "\"a\\\"b\\\\c\\nd\\u0001é\"");
    }

    /// Brackets and braces balance outside strings: a cheap check that the text is one JSON value.
    fn balanced(s: &str) -> bool {
        let (mut depth, mut in_str, mut esc) = (0i32, false, false);
        for c in s.chars() {
            match (in_str, esc, c) {
                (true, true, _) => esc = false,
                (true, false, '\\') => esc = true,
                (true, false, '"') => in_str = false,
                (false, _, '"') => in_str = true,
                (false, _, '{' | '[') => depth += 1,
                (false, _, '}' | ']') => depth -= 1,
                _ => {}
            }
            if depth < 0 {
                return false;
            }
        }
        depth == 0 && !in_str
    }
}
