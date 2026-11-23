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
use engine::sky_sun::AmbientShares;
use fork_gfx::{builtin, resolve, Inputs, Place, Resolved, SettingsFile, Source};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

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
    /// `light.ambient_exterior` and `light.ambient_interior` (set while the game runs too).
    pub ambient: AmbientShares,
}

/// The light balance's shares as `f32` bits (the original's), read by the sky's sun light once a
/// frame ([`ambient_shares`]) and set at start and by the overlay ([`set_ambient_shares`]).
static AMBIENT: [AtomicU32; 8] = {
    const ONE: AtomicU32 = AtomicU32::new(0x3f80_0000);
    [
        ONE,
        ONE,
        ONE,
        ONE,
        ONE,
        AtomicU32::new(0),
        AtomicU32::new(0),
        // 90 degrees: the moons' own path.
        AtomicU32::new(0x42b4_0000),
    ]
};

/// The shares the sun light takes now (`engine::sky_sun::want_ambient_share`).
pub fn ambient_shares() -> AmbientShares {
    let at = |i: usize| f32::from_bits(AMBIENT[i].load(Ordering::Relaxed));
    AmbientShares {
        exterior: at(0),
        interior: at(1),
        night: at(2),
        sun: at(3),
        moon: at(4),
        dome: at(5),
        shade: at(6),
        moon_height: at(7),
    }
}

/// Sets the shares the sun light takes from the next frame on.
pub fn set_ambient_shares(s: AmbientShares) {
    for (slot, v) in AMBIENT
        .iter()
        .zip([
            s.exterior,
            s.interior,
            s.night,
            s.sun,
            s.moon,
            s.dome,
            s.shade,
            s.moon_height,
        ])
    {
        slot.store(v.to_bits(), Ordering::Relaxed);
    }
}

/// The sky light's colours, handed to the renderer's lookup.
pub fn sky_fill(f: engine::sky_sun::SkyFill) {
    d3d9_wgpu::sun_shadow::set_sky_fill(f.top, f.ground, f.dome, f.ambient_over_direct, f.shade);
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
    let light = r.module("light").expect("built in");
    let ambient = AmbientShares {
        exterior: light.slider("ambient_exterior") as f32,
        interior: light.slider("ambient_interior") as f32,
        night: light.slider("ambient_night") as f32,
        sun: light.slider("sun") as f32,
        moon: light.slider("moon") as f32,
        dome: light.slider("sky_dome") as f32,
        shade: light.slider("shade_fill") as f32,
        moon_height: light.slider("moon_max_height") as f32,
    };
    (
        choice,
        Levels {
            ao_quality,
            shadow_map_size,
            ambient,
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
    let read = read(&env, profile.clone())?;
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
    let started = choice.clone();
    let (choice, levels) = apply(choice.clone(), &r);
    // The overlay (F10) edits the same settings while the game runs and writes them to the file
    // read (or, in a play run without one, the profile's).
    let path = read.file.as_ref().map(|(p, _)| p.clone()).or(profile);
    d3d9_wgpu::gfx_overlay::install(Some(session(read, path, started)));
    d3d9_wgpu::scene_light::set_quality(levels.ao_quality);
    d3d9_wgpu::sun_shadow::set_map_size(levels.shadow_map_size)?;
    set_ambient_shares(levels.ambient);
    if !r.is_original() {
        lines.push(format!(
            "nv-rs: graphics: ambient occlusion {} ({:?}, strength {}), sun shadows {} (maps {}), fill light outdoors {} indoors {}",
            if choice.ambient_occlusion {
                "on"
            } else {
                "off"
            },
            levels.ao_quality,
            choice.ambient_occlusion_strength,
            if choice.sun_shadows { "on" } else { "off" },
            levels.shadow_map_size,
            levels.ambient.exterior,
            levels.ambient.interior
        ));
    }
    Ok((choice, lines))
}

/// The overlay's session: the settings as read, the file a change is written to, and the live
/// levels set again from the run's start-up `choice` with the new values over it.
pub fn session(
    read: Read,
    path: Option<PathBuf>,
    choice: Choice,
) -> d3d9_wgpu::gfx_overlay::Session {
    let mut live = fork_gfx::Live::new(builtin::specs(), builtin::presets());
    live.set_file(read.file.map(|(_, f)| f));
    live.set_overrides(read.override_preset, read.overrides);
    d3d9_wgpu::gfx_overlay::Session::new(
        live,
        path,
        Box::new(move |r: &Resolved| {
            let (c, l) = apply(choice.clone(), r);
            d3d9_wgpu::scene_light::set_quality(l.ao_quality);
            d3d9_wgpu::scene_light::set_live_strength(Some(c.ambient_occlusion_strength));
            set_ambient_shares(l.ambient);
        }),
    )
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
            ambient: AmbientShares::ORIGINAL,
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
        assert_eq!((l.ao_quality, l.shadow_map_size), (Quality::Medium, 2048));
        // Modern takes less of the game's fill light outdoors than indoors (nv-rs#768, #772).
        assert!(l.ambient.exterior < l.ambient.interior && l.ambient.interior <= 1.0);
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
    fn the_light_balance_reaches_the_sun_lights_shares() {
        // Nothing chosen: the original's shares, whole.
        let r = resolve_at(&read(env(&[]), None).unwrap(), &Place::default());
        assert_eq!(
            apply(Choice::vanilla(), &r).1.ambient,
            AmbientShares::ORIGINAL
        );
        let r = resolve_at(
            &read(
                env(&[(
                    "NV_GFX",
                    "light.ambient_exterior=0.35,light.ambient_interior=0.9",
                )]),
                None,
            )
            .unwrap(),
            &Place::default(),
        );
        assert!(!r.is_original());
        let (c, l) = apply(Choice::vanilla(), &r);
        assert_eq!(c, Choice::vanilla(), "the light balance owns no older key");
        assert_eq!(
            l.ambient,
            AmbientShares {
                exterior: 0.35,
                interior: 0.9,
                ..AmbientShares::ORIGINAL
            }
        );
    }

    #[test]
    fn every_built_in_module_is_applied() {
        // A module added to fork_gfx::builtin needs its switches here: this list is the check.
        let ids: Vec<&str> = builtin::specs().iter().map(|s| s.id).collect();
        assert_eq!(ids, ["ao", "shadows", "light"]);
    }
}
