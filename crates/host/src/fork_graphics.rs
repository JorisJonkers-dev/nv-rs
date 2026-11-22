// SPDX-License-Identifier: GPL-3.0-only
//! The graphics settings at start (nv-rs#592, ADR-0019): the profile's settings file
//! (`fork_gfx::FILE_NAME` beside the run's `Fallout.ini`, read in a launcher's play run only),
//! `NV_GFX_FILE` (a file named for any run), `NV_GFX_PRESET` and `NV_GFX` (a scripted run's preset
//! and values), resolved with the built-in modules and presets (`fork_gfx::builtin`) and applied to
//! the run's choice of the switches they own.
//!
//! A module whose value comes from nowhere (every value its original) leaves the run's choice as the
//! older keys made it (`[nv-rs Display] bAmbientOcclusion`, `NV_AO`, ...): with no file, no preset
//! and no `NV_GFX` nothing here changes anything.

use crate::fixes::Choice;
use fork_gfx::{builtin, resolve, Inputs, Place, Resolved, SettingsFile, Source};
use std::path::{Path, PathBuf};

/// Names a settings file for any run (also a scripted one): read instead of the profile's.
pub const FILE_ENV: &str = "NV_GFX_FILE";
/// A preset for the run: wins over the file's.
pub const PRESET_ENV: &str = "NV_GFX_PRESET";
/// Values for the run (`ao.quality=high,shadows.enabled=true`): win over everything.
pub const VALUES_ENV: &str = "NV_GFX";

/// What was read at start.
#[derive(Debug)]
pub struct Read {
    /// The file read, if any.
    pub file: Option<(PathBuf, SettingsFile)>,
    pub override_preset: Option<String>,
    pub overrides: Option<SettingsFile>,
}

/// Reads the inputs: `profile_file` (the profile's file in a play run, else `None`) unless
/// `NV_GFX_FILE` names another, then `NV_GFX_PRESET` and `NV_GFX`. A file that is not there is no
/// file; one that cannot be read, a malformed `NV_GFX` or an unknown `NV_GFX_PRESET` stops the start
/// with a plain message, so a misspelt choice never runs a different picture.
pub fn read(
    env: impl Fn(&str) -> Option<String>,
    profile_file: Option<PathBuf>,
) -> Result<Read, String> {
    let named = env(FILE_ENV)
        .filter(|v| !v.trim().is_empty())
        .map(PathBuf::from);
    let path = named.clone().or(profile_file);
    let file = match path {
        None => None,
        Some(p) => match std::fs::read(&p) {
            Ok(bytes) => {
                let text = String::from_utf8(bytes).map_err(|_| {
                    format!("the graphics settings {} are not UTF-8 text", p.display())
                })?;
                Some((p, SettingsFile::parse(&text)))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && named.is_none() => None,
            Err(e) => {
                return Err(format!(
                    "can't read the graphics settings {}: {e}",
                    p.display()
                ))
            }
        },
    };
    let override_preset = env(PRESET_ENV)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    if let Some(p) = &override_preset {
        if builtin::presets().get(p).is_none() {
            let known: Vec<String> = builtin::presets()
                .list()
                .iter()
                .map(|p| p.id.clone())
                .collect();
            return Err(format!(
                "{PRESET_ENV}={p}: not a preset (known: {})",
                known.join(", ")
            ));
        }
    }
    let overrides = match env(VALUES_ENV).filter(|v| !v.trim().is_empty()) {
        None => None,
        Some(v) => {
            Some(SettingsFile::parse_overrides(&v).map_err(|why| format!("{VALUES_ENV}: {why}"))?)
        }
    };
    Ok(Read {
        file,
        override_preset,
        overrides,
    })
}

/// The settings at `place` from what was read.
pub fn resolve_at(read: &Read, place: &Place) -> Resolved {
    let specs = builtin::specs();
    let presets = builtin::presets();
    resolve(
        &Inputs {
            specs: &specs,
            presets: &presets,
            file: read.file.as_ref().map(|(_, f)| f),
            override_preset: read.override_preset.as_deref(),
            overrides: read.overrides.as_ref(),
        },
        place,
    )
}

/// What the start does with the settings beside the run's choice: the levels set in the renderer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Levels {
    /// `ao.quality` (set while the game runs too).
    pub ao_quality: d3d9_wgpu::scene_light::Quality,
    /// `shadows.quality`: the map size.
    pub shadow_map_size: u32,
}

/// The run's choice with the modules' values over it, and the levels. A value whose source is the
/// original leaves the choice's field as the older keys set it.
pub fn apply(mut choice: Choice, r: &Resolved) -> (Choice, Levels) {
    let set = |m: &fork_gfx::ModuleSettings, key: &str| {
        let i = m
            .spec
            .settings
            .iter()
            .position(|s| s.key == key)
            .expect("a built-in key");
        m.sources[i] != Source::Original
    };
    let ao = r.module("ao").expect("built in");
    if set(ao, "enabled") {
        choice.ambient_occlusion = ao.toggle("enabled");
    }
    if set(ao, "strength") {
        choice.ambient_occlusion_strength =
            d3d9_wgpu::scene_light::clamp_strength(ao.slider("strength") as f32);
    }
    let sh = r.module("shadows").expect("built in");
    if set(sh, "enabled") {
        choice.sun_shadows = sh.toggle("enabled");
    }
    if set(sh, "characters") {
        choice.sun_shadow_characters = sh.toggle("characters");
    }
    if set(sh, "light_from_sky") {
        choice.sun_light_from_sky = sh.toggle("light_from_sky");
    }
    let ao_quality =
        d3d9_wgpu::scene_light::Quality::parse(ao.choice("quality")).expect("the schema's options");
    let shadow_map_size = match sh.choice("quality") {
        "low" => d3d9_wgpu::sun_shadow::MAP_SIZES[0],
        "high" => d3d9_wgpu::sun_shadow::MAP_SIZES[2],
        _ => d3d9_wgpu::sun_shadow::MAP_SIZES[1],
    };
    (
        choice,
        Levels {
            ao_quality,
            shadow_map_size,
        },
    )
}

/// The profile's settings file: beside the run's `Fallout.ini`.
pub fn profile_file(game_ini: &Path) -> Option<PathBuf> {
    game_ini.parent().map(|d| d.join(fork_gfx::FILE_NAME))
}

/// At start: reads and resolves the settings (the place is not known yet: the global layer and the
/// presets' global layer), applies them to `choice` and sets the renderer's levels. Returns the new
/// choice and the lines to log.
pub fn start(
    choice: &Choice,
    env: impl Fn(&str) -> Option<String>,
) -> Result<(Choice, Vec<String>), String> {
    let profile = crate::fixes::is_play_run(&env)
        .then(crate::game_ini::path)
        .flatten()
        .and_then(|ini| profile_file(&ini));
    let read = read(&env, profile)?;
    let r = resolve_at(&read, &Place::default());
    let mut lines = Vec::new();
    if let Some((path, file)) = &read.file {
        lines.push(format!(
            "nv-rs: graphics settings from {} (preset {})",
            path.display(),
            r.preset
        ));
        lines.extend(
            file.notes
                .iter()
                .map(|n| format!("nv-rs: graphics settings {}: {n}", path.display())),
        );
    } else if read.override_preset.is_some() || read.overrides.is_some() {
        lines.push(format!(
            "nv-rs: graphics settings from {PRESET_ENV} / {VALUES_ENV} (preset {})",
            r.preset
        ));
    }
    lines.extend(
        r.notes
            .iter()
            .map(|n| format!("nv-rs: graphics settings: {n}")),
    );
    let (choice, levels) = apply(choice.clone(), &r);
    d3d9_wgpu::scene_light::set_quality(levels.ao_quality);
    d3d9_wgpu::sun_shadow::set_map_size(levels.shadow_map_size)?;
    if !r.is_original() {
        lines.push(format!(
            "nv-rs: graphics: ambient occlusion {} ({:?}, strength {}), sun shadows {} (maps {})",
            if choice.ambient_occlusion {
                "on"
            } else {
                "off"
            },
            levels.ao_quality,
            choice.ambient_occlusion_strength,
            if choice.sun_shadows { "on" } else { "off" },
            levels.shadow_map_size
        ));
    }
    Ok((choice, lines))
}

#[cfg(test)]
mod tests {
    use super::*;
    use d3d9_wgpu::scene_light::Quality;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| v.to_string())
        }
    }

    fn levels(ao: Quality, size: u32) -> Levels {
        Levels {
            ao_quality: ao,
            shadow_map_size: size,
        }
    }

    #[test]
    fn nothing_chosen_changes_nothing() {
        let before = Choice::vanilla();
        let read = read(env(&[]), None).unwrap();
        let r = resolve_at(&read, &Place::default());
        assert!(r.is_original());
        let (after, l) = apply(before.clone(), &r);
        assert_eq!(after, before);
        assert_eq!(l, levels(Quality::Medium, 2048));
    }

    #[test]
    fn the_older_keys_still_decide_when_the_modules_say_nothing() {
        let mut before = Choice::vanilla();
        before.ambient_occlusion = true;
        before.ambient_occlusion_strength = 0.9;
        before.sun_shadows = true;
        let r = resolve_at(&read(env(&[]), None).unwrap(), &Place::default());
        let (after, _) = apply(before.clone(), &r);
        assert_eq!(after, before);
        // A value set in the settings wins over them.
        let r = resolve_at(
            &read(env(&[("NV_GFX", "ao.enabled=false")]), None).unwrap(),
            &Place::default(),
        );
        let (after, _) = apply(before, &r);
        assert!(!after.ambient_occlusion && after.sun_shadows);
    }

    #[test]
    fn a_preset_turns_its_modules_on_with_their_levels() {
        let r = resolve_at(
            &read(env(&[("NV_GFX_PRESET", "modern")]), None).unwrap(),
            &Place::default(),
        );
        let (c, l) = apply(Choice::vanilla(), &r);
        assert!(c.ambient_occlusion && c.sun_shadows);
        assert_eq!(c.ambient_occlusion_strength, 0.7);
        assert_eq!(l, levels(Quality::Medium, 2048));
        let r = resolve_at(
            &read(
                env(&[
                    ("NV_GFX_PRESET", "performance"),
                    (
                        "NV_GFX",
                        "shadows.enabled=true,shadows.quality=high,ao.strength=0.4",
                    ),
                ]),
                None,
            )
            .unwrap(),
            &Place::default(),
        );
        let (c, l) = apply(Choice::vanilla(), &r);
        assert!(c.ambient_occlusion && c.sun_shadows);
        assert!((c.ambient_occlusion_strength - 0.4).abs() < 1e-6);
        assert_eq!(l, levels(Quality::Low, 4096));
    }

    #[test]
    fn a_misspelt_choice_stops_the_start() {
        assert!(read(env(&[("NV_GFX_PRESET", "moden")]), None)
            .unwrap_err()
            .contains("not a preset"));
        assert!(read(env(&[("NV_GFX", "ao")]), None)
            .unwrap_err()
            .starts_with("NV_GFX:"));
        let missing = std::env::temp_dir().join("nv-rs-gfx-test-missing-file.toml");
        assert!(
            read(env(&[("NV_GFX_FILE", missing.to_str().unwrap())]), None).is_err(),
            "a named file must exist"
        );
        // The profile's file may be missing: no file.
        assert!(read(env(&[]), Some(missing)).unwrap().file.is_none());
    }

    #[test]
    fn the_profile_file_is_read_and_the_environment_wins_over_it() {
        let dir = std::env::temp_dir().join(format!("nv-rs-gfx-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ini = dir.join("Fallout.ini");
        let file = profile_file(&ini).unwrap();
        assert_eq!(file, dir.join("nv-rs-graphics.toml"));
        std::fs::write(&file, "preset = \"modern\"\n[global.ao]\nquality = \"high\"\n[global.shadows]\nenabled = false\n").unwrap();
        let r = resolve_at(
            &read(env(&[]), Some(file.clone())).unwrap(),
            &Place::default(),
        );
        let (c, l) = apply(Choice::vanilla(), &r);
        assert!(c.ambient_occlusion && !c.sun_shadows);
        assert_eq!(l.ao_quality, Quality::High);
        let r = resolve_at(
            &read(env(&[("NV_GFX_PRESET", "original")]), Some(file)).unwrap(),
            &Place::default(),
        );
        let (c, l) = apply(Choice::vanilla(), &r);
        assert!(
            !c.ambient_occlusion,
            "the run's preset wins over the file's"
        );
        assert_eq!(
            l.ao_quality,
            Quality::High,
            "the file's own value still counts"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn every_built_in_module_is_applied() {
        // A module added to fork_gfx::builtin needs its switches here: this list is the check.
        let ids: Vec<&str> = builtin::specs().iter().map(|s| s.id).collect();
        assert_eq!(ids, ["ao", "shadows"]);
    }
}
