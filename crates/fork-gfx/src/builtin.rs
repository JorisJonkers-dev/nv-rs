// SPDX-License-Identifier: GPL-3.0-only
//! The engine's graphics modules as data, and its built-in presets (`presets/*.toml`).
//!
//! The schemas live here, not with the code that applies them, so a program that edits the
//! settings (raul) reads them without linking the engine. The engine's side (`host::gfx`) holds a
//! test that every module here is applied and nothing else is.

use crate::schema::{Applies, Choice, Insertion, Kind, ModuleSpec, Owns, Setting};
use crate::settings::{Preset, Presets, SettingsFile};

static AO_QUALITY: [Choice; 3] = [
    Choice {
        id: "low",
        label: "Low",
    },
    Choice {
        id: "medium",
        label: "Medium",
    },
    Choice {
        id: "high",
        label: "High",
    },
];

/// Ambient occlusion: the port's GTAO step (`d3d9_wgpu::scene_light`).
pub static AO: ModuleSpec = ModuleSpec {
    id: "ao",
    label: "Ambient occlusion",
    help: "A soft shade in corners, creases and where things meet the ground. Not in the original game. Vulkan only.",
    settings: &[
        Setting {
            key: "enabled",
            label: "Ambient occlusion",
            help: "On: the shade is drawn. Off: the original's lighting.",
            applies: Applies::NextStart,
            kind: Kind::Toggle { default: false },
        },
        Setting {
            key: "quality",
            label: "Quality",
            help: "How many directions and steps each pixel searches: low 1 by 4, medium 2 by 4, high 3 by 6. Higher is smoother and costs more.",
            applies: Applies::Live,
            kind: Kind::Choice { options: &AO_QUALITY, default: "medium" },
        },
        Setting {
            key: "strength",
            label: "Strength",
            help: "How dark the shade is, from 0.1 (faint) to 1.0 (all of it).",
            applies: Applies::Live,
            kind: Kind::Slider { min: 0.1, max: 1.0, step: 0.05, default: 0.7 },
        },
    ],
    owns: &[
        Owns::Switch("NV_AO"),
        Owns::Switch("NV_AO_STRENGTH"),
        Owns::Pass { name: "gtao", at: Insertion::AfterWorld },
    ],
    cost: "docs/fork/GRAPHICS.md, ambient occlusion",
};

static SHADOW_QUALITY: [Choice; 3] = [
    Choice {
        id: "low",
        label: "Low",
    },
    Choice {
        id: "medium",
        label: "Medium",
    },
    Choice {
        id: "high",
        label: "High",
    },
];

/// Cascaded sun shadows (`d3d9_wgpu::sun_shadow`).
pub static SHADOWS: ModuleSpec = ModuleSpec {
    id: "shadows",
    label: "Sun shadows",
    help: "Objects cast shadows from the game's sun and moon light, out to about 140 m. Not in the original game. Vulkan only.",
    settings: &[
        Setting {
            key: "enabled",
            label: "Sun shadows",
            help: "On: the world casts sun and moon shadows. Off: the original's shadows.",
            applies: Applies::NextStart,
            kind: Kind::Toggle { default: false },
        },
        Setting {
            key: "quality",
            label: "Quality",
            help: "The size of each of the four shadow maps: low 1024, medium 2048, high 4096 pixels. Higher is sharper and costs more memory and time.",
            applies: Applies::NextStart,
            kind: Kind::Choice { options: &SHADOW_QUALITY, default: "medium" },
        },
        Setting {
            key: "characters",
            label: "Characters cast",
            help: "People, creatures and you in third person cast sun shadows too, in place of the game's own character shadows.",
            applies: Applies::NextStart,
            kind: Kind::Toggle { default: true },
        },
        Setting {
            key: "light_from_sky",
            label: "Light from the sky",
            help: "The shadows point away from the sun disc by day and the moon by night. Off: the original's sun light direction.",
            applies: Applies::NextStart,
            kind: Kind::Toggle { default: true },
        },
    ],
    owns: &[
        Owns::Switch("NV_SUN_SHADOWS"),
        Owns::Switch("NV_SUN_SHADOWS_CHARACTERS"),
        Owns::Switch("NV_SUN_LIGHT_FROM_SKY"),
        Owns::Pass { name: "sun shadow maps", at: Insertion::AfterWorld },
    ],
    cost: "docs/fork/GRAPHICS.md, sun shadows",
};

/// The game's own light, rebalanced (nv-rs#768): its ambient fill (`engine::sky_sun::AmbientShare`).
pub static LIGHT: ModuleSpec = ModuleSpec {
    id: "light",
    label: "Light balance",
    help: "How much of the game's flat fill light (its ambient colour) reaches surfaces. The original lifts everything by it; with ambient occlusion, sun shadows and the light from the sky less of it keeps the picture from washing out.",
    settings: &[
        Setting {
            key: "ambient_exterior",
            label: "Fill light outdoors",
            help: "The share of the weather's ambient colour, from 0 (none: only the sun, the moon and lamps light) to 1 (the original).",
            applies: Applies::Live,
            kind: Kind::Slider { min: 0.0, max: 1.0, step: 0.05, default: 1.0 },
        },
        Setting {
            key: "ambient_interior",
            label: "Fill light indoors",
            help: "The share of the cell's ambient colour, from 0 (none: only the cell's directional light and lamps light) to 1 (the original).",
            applies: Applies::Live,
            kind: Kind::Slider { min: 0.0, max: 1.0, step: 0.05, default: 1.0 },
        },
        Setting {
            key: "ambient_night",
            label: "Fill light at night",
            help: "The share of the weather's ambient colour at night, from 0 to 1 (the original).",
            applies: Applies::Live,
            kind: Kind::Slider { min: 0.0, max: 1.0, step: 0.05, default: 1.0 },
        },
        Setting {
            key: "sun",
            label: "Sunlight",
            help: "The share of the sun's direct light by day, from 0.3 to 1 (the original).",
            applies: Applies::Live,
            kind: Kind::Slider { min: 0.3, max: 1.0, step: 0.05, default: 1.0 },
        },
        Setting {
            key: "moon",
            label: "Moonlight",
            help: "The share of the moon's direct light at night, from 0.3 to 1 (the original).",
            applies: Applies::Live,
            kind: Kind::Slider { min: 0.3, max: 1.0, step: 0.05, default: 1.0 },
        },
        Setting {
            key: "sky_dome",
            label: "Light from the sky dome",
            help: "How much of the fill light follows the sky: faces toward the sky take its colour, faces toward the ground the warm bounce, in shadow too. 0 is the game's flat light. Needs sun shadows.",
            applies: Applies::Live,
            kind: Kind::Slider { min: 0.0, max: 1.0, step: 0.05, default: 0.0 },
        },
        Setting {
            key: "shade_fill",
            label: "Light in the shade",
            help: "How much more of the sky's light reaches surfaces the sun or moon does not: shadows and walls turned away get lighter, sunlit surfaces stay as they are. Needs the light from the sky dome.",
            applies: Applies::Live,
            kind: Kind::Slider { min: 0.0, max: 2.0, step: 0.05, default: 0.0 },
        },
        Setting {
            key: "moon_max_height",
            label: "Highest moon",
            help: "How high the moon climbs, in degrees above the horizon. The drawn moon and its light move together. 90 is the game's own path. Needs the light from the sun and moon discs.",
            applies: Applies::Live,
            kind: Kind::Slider { min: 15.0, max: 90.0, step: 5.0, default: 90.0 },
        },
        Setting {
            key: "moon_specular",
            label: "Moon highlight",
            help: "How strongly glossy materials catch the moon, from 0 (none) to 8; 1 is the game's. It follows the moon's direction and phase, so a new moon gives none. Needs the light from the sky dome and from the sun and moon discs.",
            applies: Applies::Live,
            kind: Kind::Slider { min: 0.0, max: 8.0, step: 0.25, default: 1.0 },
        },
        Setting {
            key: "lamp_distance",
            label: "Lamp light distance",
            help: "How far away lamps, street lights and lit signs still light their surroundings, as a multiple of the game's light fade distance (the Options menu's light fade). 1 is the game's; higher shows the pools of light further off at some cost per lamp.",
            applies: Applies::Live,
            kind: Kind::Slider { min: 1.0, max: 16.0, step: 0.5, default: 1.0 },
        },
        Setting {
            key: "far_lights",
            label: "Far lights at night",
            help: "Shows the small light points a far-view tool may put into the far view where the lamps of the world stand: only at night, fading in over the sunset and out over the sunrise. Off, such points are never drawn (the game's far view). Needs a far view made with light points.",
            applies: Applies::Live,
            kind: Kind::Toggle { default: false },
        },
    ],
    owns: &[
        Owns::GameValue("the moons' path across the sky (only below the game's own height)"),
        Owns::GameValue("the sun light's direct colours outdoors"),
        Owns::GameValue("the weather's ambient colour as the sun light hands it to the shaders (outdoors)"),
        Owns::GameValue("the cell's ambient colour as the cell's light hands it to the shaders (indoors)"),
        Owns::GameValue("the distance at which lamps fade out (the light fade setting, scaled)"),
        Owns::GameValue("whether a far-view block's light-point shape (marked FarLights) is drawn"),
    ],
    cost: "docs/fork/GRAPHICS.md, light balance",
};

static ANISOTROPY: [Choice; 5] = [
    Choice {
        id: "game",
        label: "The game's",
    },
    Choice {
        id: "x2",
        label: "2x",
    },
    Choice {
        id: "x4",
        label: "4x",
    },
    Choice {
        id: "x8",
        label: "8x",
    },
    Choice {
        id: "x16",
        label: "16x",
    },
];

/// Texture filtering (nv-rs#829): anisotropic filtering forced on the game's samplers
/// (`d3d9_wgpu::state::forced_anisotropy`), as DXVK's `d3d9.samplerAnisotropy` does.
pub static TEXTURES: ModuleSpec = ModuleSpec {
    id: "textures",
    label: "Textures",
    help: "How textures are filtered. The game filters them anisotropically at the level of its own setting (iMaxAnisotropy in FalloutPrefs.ini: 8 when unset and at High, 1 at Low and Medium); at 1, ground and roads blur at grazing angles.",
    settings: &[Setting {
        key: "anisotropy",
        label: "Anisotropic filtering",
        help: "Keeps ground, roads and walls sharp when seen at a shallow angle. The game's: as the game sets each texture. 2x to 16x: at least that level on every mipmapped texture that is filtered, whatever the game's setting; higher is sharper and costs a little more.",
        applies: Applies::Live,
        kind: Kind::Choice { options: &ANISOTROPY, default: "game" },
    }, Setting {
        key: "precise_targets",
        label: "Precise render targets",
        help: "Keeps the game's 8-bit colour render targets in 16 bits per channel (still clamped to 0..1 like the game's): smooth gradients (night sky, fog, dark rooms) show fewer bands. Costs video memory (twice the bytes for those targets) and some bandwidth.",
        applies: Applies::NextStart,
        kind: Kind::Toggle { default: false },
    }],
    owns: &[
        Owns::GameValue("the texture samplers' filtering (minification and anisotropy)"),
        Owns::GameValue("the format of the game's 8-bit colour render targets (A8R8G8B8, X8R8G8B8)"),
    ],
    cost: "docs/fork/GRAPHICS.md, textures",
};

static PLACED_REFLECTIONS: [Choice; 3] = [
    Choice {
        id: "game",
        label: "Sky only (original)",
    },
    Choice {
        id: "low",
        label: "Far land",
    },
    Choice {
        id: "high",
        label: "Far land and nearby",
    },
];

/// Water (nv-rs#844): what the game's water reflects.
pub static WATER: ModuleSpec = ModuleSpec {
    id: "water",
    label: "Water",
    help: "What lakes and rivers mirror. The original mirrors the land only in water at the world's own water level; lakes and rivers placed at another height (Lake Mead, the Colorado) mirror the sky alone.",
    settings: &[Setting {
        key: "placed_reflections",
        label: "Lakes and rivers reflect",
        help: "Sky only: the original. Far land: lakes and rivers away from the world's water level mirror the sky and the far hills, built and objects the water's type names, through the game's own reflection of a water group. Far land and nearby: also the land, buildings and objects near the water; costs most.",
        applies: Applies::Live,
        kind: Kind::Choice { options: &PLACED_REFLECTIONS, default: "game" },
    }],
    owns: &[Owns::GameValue(
        "the reflection outdoor water away from the world's water level draws (the game's sky reflection, or its own reflection of the water group)",
    )],
    cost: "docs/fork/GRAPHICS.md, water",
};

/// The engine's modules, in the order the overlay shows them.
pub fn specs() -> Vec<&'static ModuleSpec> {
    vec![&AO, &SHADOWS, &LIGHT, &WATER, &TEXTURES]
}

/// The built-in presets' files: id, label, text.
pub const PRESET_FILES: [(&str, &str, &str); 2] = [
    ("modern", "Modern", include_str!("../presets/modern.toml")),
    (
        "performance",
        "Performance",
        include_str!("../presets/performance.toml"),
    ),
];

/// `original`, then the built-in presets.
pub fn presets() -> Presets {
    Presets::new(
        PRESET_FILES
            .iter()
            .map(|(id, label, text)| Preset {
                id: id.to_string(),
                label: label.to_string(),
                file: SettingsFile::parse(text),
            })
            .collect(),
    )
    .expect("the built-in presets have distinct names")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{resolve, Inputs, Place};

    #[test]
    fn the_modules_pass_the_check() {
        crate::schema::check(&specs()).unwrap();
    }

    #[test]
    fn every_preset_file_is_read_without_a_note_and_uses_only_known_settings() {
        let specs = specs();
        let presets = presets();
        for (id, ..) in PRESET_FILES {
            let p = presets.get(id).unwrap();
            assert!(p.file.notes.is_empty(), "{id}: {:?}", p.file.notes);
            let r = resolve(
                &Inputs {
                    specs: &specs,
                    presets: &presets,
                    file: None,
                    override_preset: Some(id),
                    overrides: None,
                },
                &Place::default(),
            );
            assert_eq!(r.preset, id);
            assert!(r.notes.is_empty(), "{id}: {:?}", r.notes);
            assert!(!r.is_original(), "{id} changes something");
        }
    }

    #[test]
    fn original_is_every_module_off() {
        let specs = specs();
        let presets = presets();
        let r = resolve(
            &Inputs {
                specs: &specs,
                presets: &presets,
                file: None,
                override_preset: None,
                overrides: None,
            },
            &Place::default(),
        );
        assert_eq!(r.preset, "original");
        assert!(r.is_original());
        assert!(!r.module("ao").unwrap().toggle("enabled"));
        assert!(!r.module("shadows").unwrap().toggle("enabled"));
    }

    #[test]
    fn the_presets_say_what_they_set() {
        let specs = specs();
        let presets = presets();
        let at = |id: &str| {
            resolve(
                &Inputs {
                    specs: &specs,
                    presets: &presets,
                    file: None,
                    override_preset: Some(id),
                    overrides: None,
                },
                &Place::default(),
            )
        };
        let modern = at("modern");
        let (ao, sh) = (
            modern.module("ao").unwrap(),
            modern.module("shadows").unwrap(),
        );
        assert!(ao.toggle("enabled") && sh.toggle("enabled"));
        assert_eq!(
            (ao.choice("quality"), sh.choice("quality")),
            ("medium", "medium")
        );
        let perf = at("performance");
        let (ao, sh) = (perf.module("ao").unwrap(), perf.module("shadows").unwrap());
        assert!(ao.toggle("enabled") && !sh.toggle("enabled"));
        assert_eq!(ao.choice("quality"), "low");
        // nv-rs#829: Modern forces 16x anisotropic filtering; the others keep the game's.
        assert_eq!(
            modern.module("textures").unwrap().choice("anisotropy"),
            "x16"
        );
        assert_eq!(
            perf.module("textures").unwrap().choice("anisotropy"),
            "game"
        );
        assert_eq!(
            at("original")
                .module("textures")
                .unwrap()
                .choice("anisotropy"),
            "game"
        );
    }
}
