// SPDX-License-Identifier: GPL-3.0-only
//! The graphics settings file of a profile, its layers by place, and what it resolves to at a place.
//!
//! ```toml
//! version = 1
//! preset = "modern"            # original (the default), modern, performance, or a preset file's name
//!
//! [global.ao]                  # every place
//! quality = "medium"
//!
//! [worldspace.WastelandNV.shadows]   # outdoors in one world space (editor id, or form id in hex)
//! quality = "low"
//!
//! [interior.ao]                # every interior cell
//! strength = 0.5
//!
//! [cell.GSDocMitchellHouse.ao] # one cell, inside or outside (editor id, or form id in hex)
//! quality = "off"
//! ```
//!
//! Resolution at a place, weakest first: each setting's original value (the schema's default), the
//! chosen preset (itself a settings file, resolved at the same place), then this file's `global`,
//! `worldspace`, `interior` and `cell` layers, then the overrides of a scripted run. The most
//! specific place wins. A key the schema does not know, a value of the wrong type and every line the
//! reader cannot use are kept when the file is written again, and named once in the notes.

use crate::doc::{self, Doc, Note};
use crate::schema::{ModuleSpec, Typed};
use crate::value::Value;

/// The version of the file format this crate writes.
pub const VERSION: u32 = 1;

/// The file's name, in the profile's settings folder (beside its `Fallout.ini`).
pub const FILE_NAME: &str = "nv-rs-graphics.toml";

/// A layer of the file, weakest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Layer {
    Global,
    /// Outdoors in one world space, by editor id or form id.
    Worldspace(String),
    /// Every interior cell.
    Interior,
    /// One cell, by editor id or form id.
    Cell(String),
}

impl Layer {
    /// The layer's rank: 0 (global) to 3 (cell); a higher one wins.
    pub fn rank(&self) -> u8 {
        match self {
            Layer::Global => 0,
            Layer::Worldspace(_) => 1,
            Layer::Interior => 2,
            Layer::Cell(_) => 3,
        }
    }

    /// The table path of the layer: `["global"]`, `["worldspace", "WastelandNV"]`, ...
    pub fn path(&self) -> Vec<&str> {
        match self {
            Layer::Global => vec!["global"],
            Layer::Worldspace(w) => vec!["worldspace", w],
            Layer::Interior => vec!["interior"],
            Layer::Cell(c) => vec!["cell", c],
        }
    }

    /// As text: `global`, `worldspace.WastelandNV`, `interior`, `cell.GSDocMitchellHouse`.
    pub fn name(&self) -> String {
        doc::dotted(
            &self
                .path()
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
        )
    }

    /// Whether the layer applies at `place`.
    pub fn applies(&self, place: &Place) -> bool {
        match self {
            Layer::Global => true,
            Layer::Worldspace(w) => {
                !place.interior && place.worldspace.as_ref().is_some_and(|n| n.matches(w))
            }
            Layer::Interior => place.interior,
            Layer::Cell(c) => place.cell.as_ref().is_some_and(|n| n.matches(c)),
        }
    }
}

/// The names of the layers' tables.
pub const LAYER_WORDS: [&str; 4] = ["global", "worldspace", "interior", "cell"];

/// A world space or cell, as the engine knows it: by editor id, form id, or both.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlaceName {
    pub editor_id: Option<String>,
    pub form_id: Option<u32>,
}

impl PlaceName {
    pub fn editor_id(id: &str) -> PlaceName {
        PlaceName {
            editor_id: Some(id.to_string()),
            form_id: None,
        }
    }

    /// Whether a layer's name in the file names this place: the editor id compared without case (as
    /// the game compares them), or a form id in hex (`0x` optional, six digits or more, as in the
    /// scene tool's place files).
    pub fn matches(&self, name: &str) -> bool {
        if self
            .editor_id
            .as_deref()
            .is_some_and(|e| e.eq_ignore_ascii_case(name))
        {
            return true;
        }
        let hex = name
            .strip_prefix("0x")
            .or_else(|| name.strip_prefix("0X"))
            .unwrap_or(name);
        self.form_id.is_some_and(|id| {
            hex.len() >= 6 && hex.len() <= 8 && u32::from_str_radix(hex, 16).is_ok_and(|v| v == id)
        })
    }
}

/// Where the player is, for the layers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Place {
    /// The world space, outdoors.
    pub worldspace: Option<PlaceName>,
    /// Whether the cell is an interior.
    pub interior: bool,
    /// The cell.
    pub cell: Option<PlaceName>,
}

/// One setting found in a file.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub layer: Layer,
    pub module: String,
    pub key: String,
    /// `None`: a value that is not one of the file's kinds (an array, a date).
    pub value: Option<Value>,
    pub line: usize,
}

/// A graphics settings file: a profile's, a preset's, or a scripted run's overrides.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingsFile {
    doc: Doc,
    /// What the reader could not use, with lines.
    pub notes: Vec<Note>,
}

/// Layer names are compared without case (the game's names), the rest exactly.
fn layer_eq(i: usize, a: &str, b: &str) -> bool {
    if i == 1 {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

impl SettingsFile {
    /// An empty file: no preset named (so `original`), nothing set.
    pub fn new() -> SettingsFile {
        SettingsFile::default()
    }

    /// Reads a file's text. Never fails: what cannot be used is kept and named in [`Self::notes`].
    pub fn parse(text: &str) -> SettingsFile {
        let (doc, mut notes) = Doc::parse(text);
        match doc.get(&["version"], doc::exact) {
            None | Some(Value::Number(_)) => {}
            Some(other) => notes.push(Note {
                line: 0,
                text: format!("version {} is not a number", other.to_toml()),
            }),
        }
        if let Some(Value::Number(v)) = doc.get(&["version"], doc::exact) {
            if *v > VERSION as f64 {
                notes.push(Note {
                    line: 0,
                    text: format!("version {v} is newer than this engine's {VERSION}: what it knows is read, the rest kept"),
                });
            }
        }
        let mut file = SettingsFile { doc, notes };
        for (path, _, line) in file.doc.entries() {
            if Self::entry_of(path, None, line).is_none()
                && !matches!(path, [k] if k == "version" || k == "preset")
            {
                let path = doc::dotted(path);
                let text = format!("`{path}` is not a setting of this file; kept as it is");
                file.notes.push(Note { line, text });
            }
        }
        file
    }

    /// The file's text: as read, with the changes made since.
    pub fn to_text(&self) -> String {
        self.doc.to_text()
    }

    /// The preset the file names (`preset = "..."`), if any.
    pub fn preset(&self) -> Option<&str> {
        match self.doc.get(&["preset"], doc::exact) {
            Some(Value::Text(s)) => Some(s),
            _ => None,
        }
    }

    /// Names the preset. A new file also gets its `version` line first.
    pub fn set_preset(&mut self, name: &str) {
        self.ensure_version();
        self.doc
            .set(&["preset"], &Value::Text(name.to_string()), doc::exact);
    }

    /// The value of `module.key` in `layer`, as written.
    pub fn get(&self, layer: &Layer, module: &str, key: &str) -> Option<&Value> {
        let mut path = layer.path();
        path.extend([module, key]);
        self.doc.get(&path, layer_eq)
    }

    /// Sets `module.key` in `layer`. Everything else in the file stays as it is.
    pub fn set(&mut self, layer: &Layer, module: &str, key: &str, value: &Value) {
        self.ensure_version();
        let mut path = layer.path();
        path.extend([module, key]);
        self.doc.set(&path, value, layer_eq);
    }

    /// Takes `module.key` out of `layer`, so the weaker layers decide. Whether it was there.
    pub fn remove(&mut self, layer: &Layer, module: &str, key: &str) -> bool {
        let mut path = layer.path();
        path.extend([module, key]);
        self.doc.remove(&path, layer_eq)
    }

    /// Every setting the file holds, in the file's order (keys that are not settings left out).
    pub fn entries(&self) -> Vec<Entry> {
        self.doc
            .entries()
            .into_iter()
            .filter_map(|(path, value, line)| Self::entry_of(path, value, line))
            .collect()
    }

    fn entry_of(path: &[String], value: Option<&Value>, line: usize) -> Option<Entry> {
        let (layer, rest) = match path {
            [l, rest @ ..] if l == "global" => (Layer::Global, rest),
            [l, rest @ ..] if l == "interior" => (Layer::Interior, rest),
            [l, name, rest @ ..] if l == "worldspace" => (Layer::Worldspace(name.clone()), rest),
            [l, name, rest @ ..] if l == "cell" => (Layer::Cell(name.clone()), rest),
            _ => return None,
        };
        match rest {
            [module, key] => Some(Entry {
                layer,
                module: module.clone(),
                key: key.clone(),
                value: value.cloned(),
                line,
            }),
            _ => None,
        }
    }

    fn ensure_version(&mut self) {
        if self.doc.get(&["version"], doc::exact).is_none() {
            self.doc
                .set(&["version"], &Value::Number(VERSION as f64), doc::exact);
        }
    }

    /// Reads overrides of a scripted run: `module.key=value` pairs split by `,` or `;`, a layer in
    /// front when it is not global (`cell.GSDocMitchellHouse.ao.quality=off`). A value is TOML
    /// (`true`, `0.5`, `"high"`) or a bare word, read as a string (`quality=high`).
    pub fn parse_overrides(text: &str) -> Result<SettingsFile, String> {
        let mut file = SettingsFile::new();
        for pair in text
            .split([',', ';'])
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            let (key, raw) = pair
                .split_once('=')
                .ok_or_else(|| format!("`{pair}`: write module.key=value"))?;
            let parts: Vec<&str> = key.trim().split('.').map(str::trim).collect();
            let (layer, module, setting) = match parts.as_slice() {
                [m, k] => (Layer::Global, *m, *k),
                ["global", m, k] => (Layer::Global, *m, *k),
                ["interior", m, k] => (Layer::Interior, *m, *k),
                ["worldspace", w, m, k] => (Layer::Worldspace(w.to_string()), *m, *k),
                ["cell", c, m, k] => (Layer::Cell(c.to_string()), *m, *k),
                _ => {
                    return Err(format!(
                        "`{key}`: write module.key, or a layer and module.key"
                    ))
                }
            };
            let raw = raw.trim();
            let value = match crate::value::parse(raw) {
                Ok((Some(v), len)) if len == raw.len() => v,
                _ => Value::Text(raw.to_string()),
            };
            file.doc.set(
                &{
                    let mut p = layer.path();
                    p.extend([module, setting]);
                    p
                },
                &value,
                layer_eq,
            );
        }
        Ok(file)
    }
}

/// A preset: a name, a label, and the settings file that is its data.
#[derive(Clone, Debug)]
pub struct Preset {
    pub id: String,
    pub label: String,
    pub file: SettingsFile,
}

/// The presets a run knows: `original` always, first; the engine's built-in ones; later the
/// player's own preset files and imported looks.
#[derive(Clone, Debug)]
pub struct Presets {
    list: Vec<Preset>,
}

/// The id of the preset that draws the original picture.
pub const ORIGINAL: &str = "original";

impl Presets {
    /// `original` (an empty file: every setting at its original value) and then `others`. A preset
    /// named `original` among them is refused, as is a name given twice.
    pub fn new(others: Vec<Preset>) -> Result<Presets, String> {
        let mut list = vec![Preset {
            id: ORIGINAL.to_string(),
            label: "Original".to_string(),
            file: SettingsFile::new(),
        }];
        for p in others {
            if list.iter().any(|q| q.id.eq_ignore_ascii_case(&p.id)) {
                return Err(format!("preset `{}` twice (or named original)", p.id));
            }
            list.push(p);
        }
        Ok(Presets { list })
    }

    /// The presets in order, `original` first.
    pub fn list(&self) -> &[Preset] {
        &self.list
    }

    /// The preset named `id`, compared without case.
    pub fn get(&self, id: &str) -> Option<&Preset> {
        self.list
            .iter()
            .find(|p| p.id.eq_ignore_ascii_case(id.trim()))
    }
}

/// Where a resolved value came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// The schema's default: the original.
    Original,
    /// The preset, at this layer of its file.
    Preset(String, Layer),
    /// The profile's file, at this layer.
    File(Layer),
    /// A scripted run's override, at this layer.
    Override(Layer),
}

/// One module's settings at a place.
#[derive(Clone, Debug, PartialEq)]
pub struct ModuleSettings {
    pub spec: &'static ModuleSpec,
    /// One value per setting, in the schema's order.
    pub values: Vec<Typed>,
    /// Where each came from.
    pub sources: Vec<Source>,
}

impl ModuleSettings {
    /// Every setting at its original value.
    pub fn original(spec: &'static ModuleSpec) -> ModuleSettings {
        ModuleSettings {
            spec,
            values: spec.settings.iter().map(|s| s.original()).collect(),
            sources: vec![Source::Original; spec.settings.len()],
        }
    }

    /// The value of `key`. Panics on a key the schema does not have: a module asks only for its own.
    pub fn get(&self, key: &str) -> Typed {
        let i = self
            .spec
            .settings
            .iter()
            .position(|s| s.key == key)
            .unwrap_or_else(|| panic!("module `{}` has no setting `{key}`", self.spec.id));
        self.values[i]
    }

    pub fn toggle(&self, key: &str) -> bool {
        match self.get(key) {
            Typed::Toggle(b) => b,
            other => panic!("`{}.{key}` is {other:?}, not a toggle", self.spec.id),
        }
    }

    pub fn slider(&self, key: &str) -> f64 {
        match self.get(key) {
            Typed::Slider(n) => n,
            other => panic!("`{}.{key}` is {other:?}, not a slider", self.spec.id),
        }
    }

    pub fn choice(&self, key: &str) -> &'static str {
        match self.get(key) {
            Typed::Choice(c) => c,
            other => panic!("`{}.{key}` is {other:?}, not a choice", self.spec.id),
        }
    }

    /// Whether every setting is at its original value: the module must then leave the frame
    /// exactly as the original draws it (ADR-0019, "Original is identical").
    pub fn is_original(&self) -> bool {
        self.spec
            .settings
            .iter()
            .zip(&self.values)
            .all(|(s, v)| s.original() == *v)
    }
}

/// Every module's settings at one place, and what could not be used on the way.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    /// The preset used.
    pub preset: String,
    pub modules: Vec<ModuleSettings>,
    /// Plain messages: an unknown preset, module or key, a value that could not be used.
    pub notes: Vec<String>,
}

impl Resolved {
    pub fn module(&self, id: &str) -> Option<&ModuleSettings> {
        self.modules.iter().find(|m| m.spec.id == id)
    }

    /// Whether every module is at its original values.
    pub fn is_original(&self) -> bool {
        self.modules.iter().all(ModuleSettings::is_original)
    }
}

/// The inputs of a resolution.
pub struct Inputs<'a> {
    pub specs: &'a [&'static ModuleSpec],
    pub presets: &'a Presets,
    /// The profile's file (`None`: a run that reads none, such as a scripted run).
    pub file: Option<&'a SettingsFile>,
    /// A scripted run's choices (`NV_GFX_PRESET`, `NV_GFX`, a place file's `gfx`): a preset, which
    /// wins over the file's, and values, which win over everything.
    pub override_preset: Option<&'a str>,
    pub overrides: Option<&'a SettingsFile>,
}

/// Resolves every module's settings at `place`.
pub fn resolve(inputs: &Inputs<'_>, place: &Place) -> Resolved {
    let mut notes = Vec::new();
    let wanted = inputs
        .override_preset
        .or_else(|| inputs.file.and_then(SettingsFile::preset))
        .unwrap_or(ORIGINAL);
    let preset = inputs.presets.get(wanted).unwrap_or_else(|| {
        notes.push(format!(
            "preset `{wanted}` is not known (known: {}); original is used",
            inputs
                .presets
                .list()
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        &inputs.presets.list()[0]
    });
    let mut modules: Vec<ModuleSettings> = inputs
        .specs
        .iter()
        .map(|s| ModuleSettings::original(s))
        .collect();
    #[derive(Clone, Copy)]
    enum From {
        Preset,
        File,
        Override,
    }
    let sources = [
        (Some(&preset.file), From::Preset),
        (inputs.file, From::File),
        (inputs.overrides, From::Override),
    ];
    for (file, from) in sources {
        let Some(file) = file else { continue };
        let source = |l: Layer| match from {
            From::Preset => Source::Preset(preset.id.clone(), l),
            From::File => Source::File(l),
            From::Override => Source::Override(l),
        };
        let mut entries = file.entries();
        // Weakest layer first; the file's order within a layer (the first of a key set twice counts,
        // and the reader lists it once).
        entries.sort_by_key(|e| e.layer.rank());
        for e in entries {
            let Some(m) = modules.iter_mut().find(|m| m.spec.id == e.module) else {
                notes.push(format!(
                    "line {}: no module `{}`; `{}.{}.{}` not used",
                    e.line,
                    e.module,
                    e.layer.name(),
                    e.module,
                    e.key
                ));
                continue;
            };
            let Some(i) = m.spec.settings.iter().position(|s| s.key == e.key) else {
                notes.push(format!(
                    "line {}: module `{}` has no setting `{}`; not used",
                    e.line, e.module, e.key
                ));
                continue;
            };
            if !e.layer.applies(place) {
                continue;
            }
            let Some(value) = &e.value else {
                notes.push(format!(
                    "line {}: `{}.{}` is not a value this file reads; not used",
                    e.line, e.module, e.key
                ));
                continue;
            };
            match m.spec.settings[i].accept(value) {
                Ok((v, note)) => {
                    if let Some(n) = note {
                        notes.push(format!("line {}: `{}.{}`: {n}", e.line, e.module, e.key));
                    }
                    m.values[i] = v;
                    m.sources[i] = source(e.layer.clone());
                }
                Err(why) => notes.push(format!(
                    "line {}: `{}.{}`: {why}; not used",
                    e.line, e.module, e.key
                )),
            }
        }
    }
    Resolved {
        preset: preset.id.clone(),
        modules,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::tests::{AO, FOG};

    static SPECS: [&ModuleSpec; 2] = [&AO, &FOG];

    fn presets() -> Presets {
        Presets::new(vec![Preset {
            id: "modern".into(),
            label: "Modern".into(),
            file: SettingsFile::parse(
                "[global.ao]\nquality = \"high\"\nstrength = 0.9\n[interior.ao]\nstrength = 0.5\n",
            ),
        }])
        .unwrap()
    }

    fn outdoors(ws: &str, cell: &str) -> Place {
        Place {
            worldspace: Some(PlaceName::editor_id(ws)),
            interior: false,
            cell: Some(PlaceName::editor_id(cell)),
        }
    }

    fn inside(cell: &str) -> Place {
        Place {
            worldspace: None,
            interior: true,
            cell: Some(PlaceName::editor_id(cell)),
        }
    }

    fn at(
        file: Option<&SettingsFile>,
        overrides: Option<&SettingsFile>,
        place: &Place,
    ) -> Resolved {
        let presets = presets();
        resolve(
            &Inputs {
                specs: &SPECS,
                presets: &presets,
                file,
                override_preset: None,
                overrides,
            },
            place,
        )
    }

    #[test]
    fn no_file_resolves_to_the_original_everywhere() {
        for place in [
            Place::default(),
            outdoors("WastelandNV", "x"),
            inside("GSDocMitchellHouse"),
        ] {
            let r = at(None, None, &place);
            assert_eq!(r.preset, "original");
            assert!(r.is_original());
            assert!(r.notes.is_empty());
            assert!(r
                .modules
                .iter()
                .all(|m| m.sources.iter().all(|s| *s == Source::Original)));
        }
        // An empty file and one naming the original preset are the same.
        assert!(at(Some(&SettingsFile::parse("")), None, &Place::default()).is_original());
        assert!(at(
            Some(&SettingsFile::parse("preset = \"Original\"\n")),
            None,
            &Place::default()
        )
        .is_original());
    }

    #[test]
    fn the_most_specific_place_wins() {
        let file = SettingsFile::parse(
            "preset = \"modern\"\n\
             [cell.GSDocMitchellHouse.ao]\nquality = \"off\"\n\
             [interior.ao]\nquality = \"low\"\n\
             [worldspace.WastelandNV.ao]\nquality = \"low\"\nstrength = 0.3\n\
             [global.ao]\nstrength = 0.6\n\
             [global.fog]\ndistance = 2.0\n\
             [cell.0010A214.fog]\nenabled = false\n",
        );
        assert!(file.notes.is_empty(), "{:?}", file.notes);
        // Outdoors in the world space: its layer over global over the preset.
        let r = at(Some(&file), None, &outdoors("wastelandnv", "SomeCell"));
        let ao = r.module("ao").unwrap();
        assert_eq!((ao.choice("quality"), ao.slider("strength")), ("low", 0.3));
        assert_eq!(
            ao.sources[0],
            Source::File(Layer::Worldspace("WastelandNV".into()))
        );
        // Another world space: global's strength, the preset's quality.
        let r = at(Some(&file), None, &outdoors("Zion", "SomeCell"));
        let ao = r.module("ao").unwrap();
        assert_eq!((ao.choice("quality"), ao.slider("strength")), ("high", 0.6));
        assert_eq!(
            ao.sources[0],
            Source::Preset("modern".into(), Layer::Global)
        );
        // Inside: the interior layer (the file's over the preset's), and the one cell over it.
        let r = at(Some(&file), None, &inside("VaultX"));
        assert_eq!(r.module("ao").unwrap().choice("quality"), "low");
        assert_eq!(
            r.module("ao").unwrap().slider("strength"),
            0.6,
            "the file's global beats the preset's interior"
        );
        let r = at(Some(&file), None, &inside("GSDocMitchellHouse"));
        assert_eq!(r.module("ao").unwrap().choice("quality"), "off");
        // A cell by form id.
        let cell = Place {
            cell: Some(PlaceName {
                editor_id: None,
                form_id: Some(0x0010a214),
            }),
            ..outdoors("WastelandNV", "")
        };
        let r = at(Some(&file), None, &cell);
        assert!(!r.module("fog").unwrap().toggle("enabled"));
        assert_eq!(r.module("fog").unwrap().slider("distance"), 2.0);
    }

    #[test]
    fn a_worldspace_layer_does_not_reach_interiors() {
        let file = SettingsFile::parse("[worldspace.WastelandNV.ao]\nquality = \"high\"\n");
        let place = Place {
            worldspace: Some(PlaceName::editor_id("WastelandNV")),
            interior: true,
            cell: None,
        };
        assert!(at(Some(&file), None, &place).is_original());
    }

    #[test]
    fn overrides_win_over_everything() {
        let file = SettingsFile::parse("[cell.GSDocMitchellHouse.ao]\nquality = \"off\"\n");
        let over = SettingsFile::parse_overrides(
            "ao.quality=high, fog.enabled=false; cell.GSDocMitchellHouse.ao.strength=0.2",
        )
        .unwrap();
        let r = at(Some(&file), Some(&over), &inside("GSDocMitchellHouse"));
        let ao = r.module("ao").unwrap();
        assert_eq!((ao.choice("quality"), ao.slider("strength")), ("high", 0.2));
        assert_eq!(ao.sources[0], Source::Override(Layer::Global));
        assert!(!r.module("fog").unwrap().toggle("enabled"));
        assert!(SettingsFile::parse_overrides("ao=1").is_err());
        assert!(SettingsFile::parse_overrides("quality").is_err());
        // A scripted preset wins over the file's.
        let presets = presets();
        let file = SettingsFile::parse("preset = \"modern\"\n");
        let inputs = Inputs {
            specs: &SPECS,
            presets: &presets,
            file: Some(&file),
            override_preset: Some("original"),
            overrides: None,
        };
        assert!(resolve(&inputs, &Place::default()).is_original());
    }

    #[test]
    fn what_cannot_be_used_is_named_and_left_to_the_weaker_layers() {
        let file = SettingsFile::parse(
            "preset = \"cinematic\"\n\
             [global.ao]\nquality = \"ultra\"\nstrength = 5\nsamples = 9\n\
             [global.fog]\ndistance = [1]\n\
             [global.bloom]\nstrength = 1\n",
        );
        let r = at(Some(&file), None, &Place::default());
        assert_eq!(r.preset, "original");
        let ao = r.module("ao").unwrap();
        assert_eq!(ao.choice("quality"), "off");
        assert_eq!(ao.slider("strength"), 1.0, "clamped");
        let text = r.notes.join("\n");
        for want in [
            "preset `cinematic`",
            "\"ultra\" is not one of",
            "outside",
            "no setting `samples`",
            "not a value this file reads",
            "no module `bloom`",
        ] {
            assert!(text.contains(want), "{want} in {text}");
        }
    }

    #[test]
    fn unknown_keys_are_kept_on_write() {
        let text = "# by hand\nversion = 1\nlook = \"mine\"\n\n[global.ao]\nquality = \"low\" # nice\nfuture = { a = 1 }\n\n[raul]\nwindow = [1, 2]\n";
        let mut file = SettingsFile::parse(text);
        let notes: Vec<String> = file.notes.iter().map(|n| n.to_string()).collect();
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(
            notes[0].contains("look") && notes[1].contains("raul.window"),
            "{notes:?}"
        );
        file.set(&Layer::Global, "ao", "quality", &Value::Text("high".into()));
        file.set(
            &Layer::Cell("GSDocMitchellHouse".into()),
            "ao",
            "quality",
            &Value::Text("off".into()),
        );
        file.set_preset("modern");
        let out = file.to_text();
        assert_eq!(
            out,
            "# by hand\nversion = 1\nlook = \"mine\"\npreset = \"modern\"\n\n[global.ao]\nquality = \"high\" # nice\nfuture = { a = 1 }\n\n[raul]\nwindow = [1, 2]\n\n[cell.GSDocMitchellHouse.ao]\nquality = \"off\"\n"
        );
        // Round trip: read again, nothing lost, the same values.
        let again = SettingsFile::parse(&out);
        assert_eq!(again.to_text(), out);
        assert_eq!(again.preset(), Some("modern"));
        assert_eq!(
            again.get(&Layer::Cell("gsdocmitchellhouse".into()), "ao", "quality"),
            Some(&Value::Text("off".into()))
        );
        assert_eq!(again.entries().len(), 3);
    }

    #[test]
    fn a_new_file_starts_with_its_version() {
        let mut file = SettingsFile::new();
        file.set(
            &Layer::Worldspace("WastelandNV".into()),
            "fog",
            "distance",
            &Value::Number(1.5),
        );
        assert_eq!(
            file.to_text(),
            "version = 1\n\n[worldspace.WastelandNV.fog]\ndistance = 1.5\n"
        );
        assert!(file.remove(&Layer::Worldspace("WASTELANDNV".into()), "fog", "distance"));
        assert_eq!(
            file.get(&Layer::Worldspace("WastelandNV".into()), "fog", "distance"),
            None
        );
        let newer = SettingsFile::parse("version = 2\n");
        assert!(newer.notes[0].text.contains("newer"));
    }

    #[test]
    fn place_names_match_by_editor_id_or_form_id() {
        let n = PlaceName {
            editor_id: Some("WastelandNV".into()),
            form_id: Some(0xda726),
        };
        assert!(n.matches("wastelandnv"));
        assert!(n.matches("0x000DA726") && n.matches("0da726") && n.matches("000da726"));
        assert!(!n.matches("da72") && !n.matches("Zion") && !n.matches("0x1da726"));
    }

    #[test]
    fn presets_have_unique_names_and_original_first() {
        let p = presets();
        assert_eq!(p.list()[0].id, "original");
        assert_eq!(p.get("MODERN").unwrap().label, "Modern");
        let twice = Preset {
            id: "Original".into(),
            label: "x".into(),
            file: SettingsFile::new(),
        };
        assert!(Presets::new(vec![twice]).is_err());
    }
}
