// SPDX-License-Identifier: GPL-3.0-only
//! The graphics module trait, the registry that hands each module its settings, and the live state
//! that re-resolves the settings when the place or the file changes.
//!
//! The engine calls [`Live::update`] once a frame with the player's place: it resolves again only
//! when the place, the file, the preset or the overrides changed, and [`Registry::apply`] then calls
//! [`GraphicsModule::apply`] only on the modules whose values changed. A change made in the overlay
//! or in the file is therefore in the next frame, and a frame with nothing changed costs a compare.

use crate::schema::ModuleSpec;
use crate::settings::{resolve, Inputs, ModuleSettings, Place, Presets, Resolved, SettingsFile};

/// A graphics module: a named group of settings and the part of the frame it owns.
///
/// The pure part lives here; the drawing side (the passes a module records) lives with the
/// renderer, which this crate does not depend on. A module of phase 1 owns switches of code that
/// exists already (ambient occlusion, sun shadows) or values of the game's own data (bloom, depth of
/// field, fog): its `apply` sets them.
pub trait GraphicsModule: Send {
    /// Its schema: id, settings, what it owns, a note on its cost.
    fn spec(&self) -> &'static ModuleSpec;

    /// The settings to use from the next frame on. Called once when the module is registered and
    /// again whenever one of its values changes. With every value at its original
    /// ([`ModuleSettings::is_original`]) the module must leave the frame exactly as the original
    /// draws it: nothing installed, read or recorded.
    fn apply(&mut self, settings: &ModuleSettings);
}

/// The registered modules, in the order the overlay shows them.
pub struct Registry {
    modules: Vec<Box<dyn GraphicsModule>>,
    /// The values each module was last given (`None`: not yet).
    given: Vec<Option<ModuleSettings>>,
}

impl Registry {
    /// Registers `modules`; their schemas must pass [`crate::schema::check`].
    pub fn new(modules: Vec<Box<dyn GraphicsModule>>) -> Result<Registry, String> {
        let specs: Vec<&'static ModuleSpec> = modules.iter().map(|m| m.spec()).collect();
        crate::schema::check(&specs)?;
        let given = vec![None; modules.len()];
        Ok(Registry { modules, given })
    }

    /// The schemas, in order.
    pub fn specs(&self) -> Vec<&'static ModuleSpec> {
        self.modules.iter().map(|m| m.spec()).collect()
    }

    /// Hands each module its settings from `resolved` when they differ from what it was last given.
    /// Returns the ids of the modules that were called.
    pub fn apply(&mut self, resolved: &Resolved) -> Vec<&'static str> {
        let mut called = Vec::new();
        for (m, given) in self.modules.iter_mut().zip(&mut self.given) {
            let id = m.spec().id;
            let Some(now) = resolved.module(id) else {
                continue;
            };
            if given.as_ref().map(|g| &g.values) != Some(&now.values) {
                m.apply(now);
                *given = Some(now.clone());
                called.push(id);
            }
        }
        called
    }
}

/// The inputs of the settings, kept between frames, and the last resolution.
pub struct Live {
    specs: Vec<&'static ModuleSpec>,
    presets: Presets,
    file: Option<SettingsFile>,
    override_preset: Option<String>,
    overrides: Option<SettingsFile>,
    place: Option<Place>,
    resolved: Option<Resolved>,
    changed: bool,
}

impl Live {
    pub fn new(specs: Vec<&'static ModuleSpec>, presets: Presets) -> Live {
        Live {
            specs,
            presets,
            file: None,
            override_preset: None,
            overrides: None,
            place: None,
            resolved: None,
            changed: true,
        }
    }

    /// The profile's file (`None`: none is read).
    pub fn file(&self) -> Option<&SettingsFile> {
        self.file.as_ref()
    }

    /// Replaces the profile's file (read again after another program wrote it).
    pub fn set_file(&mut self, file: Option<SettingsFile>) {
        self.file = file;
        self.changed = true;
    }

    /// The profile's file to change (the overlay); a new one when none was read.
    pub fn file_mut(&mut self) -> &mut SettingsFile {
        self.changed = true;
        self.file.get_or_insert_with(SettingsFile::new)
    }

    /// A scripted run's preset and values.
    pub fn set_overrides(&mut self, preset: Option<String>, values: Option<SettingsFile>) {
        self.override_preset = preset;
        self.overrides = values;
        self.changed = true;
    }

    pub fn presets(&self) -> &Presets {
        &self.presets
    }

    pub fn specs(&self) -> &[&'static ModuleSpec] {
        &self.specs
    }

    /// The settings at `place`: `Some` when they were resolved again (the first call, a new place,
    /// changed inputs), `None` when nothing changed since the last call.
    pub fn update(&mut self, place: &Place) -> Option<&Resolved> {
        if !self.changed && self.place.as_ref() == Some(place) {
            return None;
        }
        let inputs = Inputs {
            specs: &self.specs,
            presets: &self.presets,
            file: self.file.as_ref(),
            override_preset: self.override_preset.as_deref(),
            overrides: self.overrides.as_ref(),
        };
        self.resolved = Some(resolve(&inputs, place));
        self.place = Some(place.clone());
        self.changed = false;
        self.resolved.as_ref()
    }

    /// The last resolution.
    pub fn resolved(&self) -> Option<&Resolved> {
        self.resolved.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::tests::{AO, FOG};
    use crate::settings::{Layer, PlaceName};
    use crate::value::Value;
    use std::sync::{Arc, Mutex};

    struct Recorder {
        spec: &'static ModuleSpec,
        log: Arc<Mutex<Vec<String>>>,
    }

    impl GraphicsModule for Recorder {
        fn spec(&self) -> &'static ModuleSpec {
            self.spec
        }
        fn apply(&mut self, s: &ModuleSettings) {
            self.log.lock().unwrap().push(format!(
                "{} {:?} original={}",
                self.spec.id,
                s.values,
                s.is_original()
            ));
        }
    }

    #[test]
    fn modules_are_called_only_when_their_values_change() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut reg = Registry::new(vec![
            Box::new(Recorder {
                spec: &AO,
                log: log.clone(),
            }),
            Box::new(Recorder {
                spec: &FOG,
                log: log.clone(),
            }),
        ])
        .unwrap();
        let mut live = Live::new(reg.specs(), Presets::new(vec![]).unwrap());
        let goodsprings = Place {
            worldspace: Some(PlaceName::editor_id("WastelandNV")),
            interior: false,
            cell: None,
        };
        let house = Place {
            worldspace: None,
            interior: true,
            cell: Some(PlaceName::editor_id("GSDocMitchellHouse")),
        };

        // First frame: every module gets its (original) settings once.
        let r = live.update(&goodsprings).unwrap().clone();
        assert!(r.is_original());
        assert_eq!(reg.apply(&r), vec!["ao", "fog"]);
        // Nothing changed: no resolution, no call.
        assert!(live.update(&goodsprings).is_none());
        // The overlay changes a value: in the next frame only that module is called.
        live.file_mut().set(
            &Layer::Interior,
            "ao",
            "quality",
            &Value::Text("high".into()),
        );
        let r = live.update(&goodsprings).unwrap().clone();
        assert_eq!(
            reg.apply(&r),
            Vec::<&str>::new(),
            "an interior layer changes nothing outdoors"
        );
        // Walking inside: the interior layer applies.
        let r = live.update(&house).unwrap().clone();
        assert_eq!(reg.apply(&r), vec!["ao"]);
        // And back out: the original again.
        let r = live.update(&goodsprings).unwrap().clone();
        assert_eq!(reg.apply(&r), vec!["ao"]);
        let log = log.lock().unwrap();
        assert_eq!(
            *log,
            vec![
                "ao [Choice(\"off\"), Slider(0.7)] original=true",
                "fog [Toggle(true), Slider(1.0)] original=true",
                "ao [Choice(\"high\"), Slider(0.7)] original=false",
                "ao [Choice(\"off\"), Slider(0.7)] original=true",
            ]
        );
    }

    #[test]
    fn a_scripted_preset_and_values_reach_the_modules() {
        let modern = crate::settings::Preset {
            id: "modern".into(),
            label: "Modern".into(),
            file: SettingsFile::parse("[global.ao]\nquality = \"low\"\n"),
        };
        let mut live = Live::new(vec![&AO, &FOG], Presets::new(vec![modern]).unwrap());
        live.set_overrides(
            Some("modern".into()),
            Some(SettingsFile::parse_overrides("fog.distance=2").unwrap()),
        );
        let r = live.update(&Place::default()).unwrap();
        assert_eq!(r.preset, "modern");
        assert_eq!(r.module("ao").unwrap().choice("quality"), "low");
        assert_eq!(r.module("fog").unwrap().slider("distance"), 2.0);
    }

    #[test]
    fn a_registry_refuses_a_bad_schema() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let twice: Vec<Box<dyn GraphicsModule>> = vec![
            Box::new(Recorder {
                spec: &AO,
                log: log.clone(),
            }),
            Box::new(Recorder { spec: &AO, log }),
        ];
        assert!(Registry::new(twice).is_err());
    }
}
