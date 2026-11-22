// SPDX-License-Identifier: GPL-3.0-only
//! What a graphics module declares: its settings (typed entries: a toggle, a slider or a choice),
//! what it owns in the frame, and a note on its cost.
//!
//! A module's schema is plain data (`static`s), so the overlay, raul's Settings page and the JSON
//! description ([`crate::dump`]) are all made from the same list and none of them hard-codes a
//! setting.

use crate::value::Value;

/// One graphics module, as data.
#[derive(Debug, PartialEq)]
pub struct ModuleSpec {
    /// The module's key in the settings file: lower case letters, digits and `_` (`ao`, `shadows`).
    pub id: &'static str,
    /// The name shown to the player.
    pub label: &'static str,
    /// One or two sentences: what it does to the picture.
    pub help: &'static str,
    /// Its settings, in the order they are shown. The first is the module's main switch when it is
    /// a toggle or a choice with an `off` option.
    pub settings: &'static [Setting],
    /// What the module switches or adds in the frame.
    pub owns: &'static [Owns],
    /// Where its measured cost is written down and what it depends on (`docs/fork/GRAPHICS.md`).
    pub cost: &'static str,
}

/// One setting of a module.
#[derive(Debug, PartialEq)]
pub struct Setting {
    /// Its key in the module's table: lower case letters, digits and `_`.
    pub key: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    /// When a change takes effect in a running game.
    pub applies: Applies,
    pub kind: Kind,
}

/// When a change of a setting takes effect in a running game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applies {
    /// In the next frame.
    Live,
    /// At the next start of the game (the overlay and raul say so).
    NextStart,
}

impl Applies {
    pub fn name(self) -> &'static str {
        match self {
            Applies::Live => "live",
            Applies::NextStart => "next_start",
        }
    }
}

/// The type of a setting, with its default: the value that draws the original picture.
#[derive(Debug, PartialEq)]
pub enum Kind {
    /// On or off.
    Toggle { default: bool },
    /// A number from `min` to `max`, in steps of `step` (the overlay's step; any value in the range
    /// is accepted from the file).
    Slider {
        min: f64,
        max: f64,
        step: f64,
        default: f64,
    },
    /// One of a list of named options.
    Choice {
        options: &'static [Choice],
        default: &'static str,
    },
}

/// One option of a [`Kind::Choice`].
#[derive(Debug, PartialEq)]
pub struct Choice {
    /// Its value in the file: lower case letters, digits and `_`.
    pub id: &'static str,
    pub label: &'static str,
}

/// What a module owns in the frame.
#[derive(Debug, PartialEq)]
pub enum Owns {
    /// A switch of code that exists already, named as its `Fallout.ini` key or environment variable
    /// (`NV_AO`): the module sets it, nothing else does while the module is registered.
    Switch(&'static str),
    /// A value of the game's own data the module scales or replaces (`fFogEnd`, an image-space
    /// field), named for the reader.
    GameValue(&'static str),
    /// A pass the module adds to the frame, and where.
    Pass { name: &'static str, at: Insertion },
}

/// Where a pass goes in the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Insertion {
    /// Right after the world is drawn, inside the game's HDR chain (where ambient occlusion and the
    /// sun shadows hook in today: the marker `SetTransform` after `world_drawn`).
    AfterWorld,
    /// At the present, on the finished frame, before the interface overlays.
    AtPresent,
}

impl Insertion {
    pub fn name(self) -> &'static str {
        match self {
            Insertion::AfterWorld => "after_world",
            Insertion::AtPresent => "at_present",
        }
    }
}

/// A setting's value after it was checked against its [`Kind`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Typed {
    Toggle(bool),
    Slider(f64),
    Choice(&'static str),
}

impl Typed {
    /// As a file value.
    pub fn to_value(self) -> Value {
        match self {
            Typed::Toggle(b) => Value::Bool(b),
            Typed::Slider(n) => Value::Number(n),
            Typed::Choice(s) => Value::Text(s.to_string()),
        }
    }
}

impl Setting {
    /// The default, the value that draws the original picture.
    pub fn original(&self) -> Typed {
        match self.kind {
            Kind::Toggle { default } => Typed::Toggle(default),
            Kind::Slider { default, .. } => Typed::Slider(default),
            Kind::Choice { default, .. } => Typed::Choice(default),
        }
    }

    /// The file's value as this setting's type. A number outside the slider's range is clamped (the
    /// message says so); a choice is matched without case. `Err` with a plain message when the
    /// value cannot be used: the setting then keeps what the weaker layers gave it.
    pub fn accept(&self, v: &Value) -> Result<(Typed, Option<String>), String> {
        match (&self.kind, v) {
            (Kind::Toggle { .. }, Value::Bool(b)) => Ok((Typed::Toggle(*b), None)),
            (Kind::Slider { min, max, .. }, Value::Number(n)) => {
                if !n.is_finite() {
                    return Err(format!("{n} is not a number from {min} to {max}"));
                }
                let c = n.clamp(*min, *max);
                let note = (c != *n).then(|| format!("{n} is outside {min} to {max}; {c} is used"));
                Ok((Typed::Slider(c), note))
            }
            (Kind::Choice { options, .. }, Value::Text(s)) => options
                .iter()
                .find(|o| o.id.eq_ignore_ascii_case(s.trim()))
                .map(|o| (Typed::Choice(o.id), None))
                .ok_or_else(|| {
                    format!(
                        "\"{s}\" is not one of {}",
                        options.iter().map(|o| o.id).collect::<Vec<_>>().join(", ")
                    )
                }),
            (kind, v) => Err(format!(
                "{} is {}, not {}",
                v.to_toml(),
                v.kind_name(),
                kind.wants()
            )),
        }
    }
}

impl Kind {
    fn wants(&self) -> &'static str {
        match self {
            Kind::Toggle { .. } => "true or false",
            Kind::Slider { .. } => "a number",
            Kind::Choice { .. } => "one of the named options",
        }
    }
}

impl ModuleSpec {
    /// The setting named `key`.
    pub fn setting(&self, key: &str) -> Option<&'static Setting> {
        self.settings.iter().find(|s| s.key == key)
    }
}

/// Checks a list of module schemas: ids and keys well formed and unique, every default inside its
/// range or among its options, slider ranges and steps sensible. The list the engine registers is
/// held to this by a test.
pub fn check(specs: &[&ModuleSpec]) -> Result<(), String> {
    fn word(what: &str, w: &str) -> Result<(), String> {
        if w.is_empty()
            || !w
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            || !w.as_bytes()[0].is_ascii_lowercase()
        {
            return Err(format!(
                "{what} `{w}`: lower case letters, digits and _, starting with a letter"
            ));
        }
        Ok(())
    }
    let mut ids = Vec::new();
    for spec in specs {
        word("module id", spec.id)?;
        if crate::LAYER_WORDS.contains(&spec.id) {
            return Err(format!("module id `{}` is the name of a layer", spec.id));
        }
        if ids.contains(&spec.id) {
            return Err(format!("module id `{}` twice", spec.id));
        }
        ids.push(spec.id);
        if spec.label.is_empty() || spec.help.is_empty() {
            return Err(format!("module `{}` without a label or help", spec.id));
        }
        let mut keys = Vec::new();
        for s in spec.settings {
            word(&format!("setting of `{}`", spec.id), s.key)?;
            if keys.contains(&s.key) {
                return Err(format!("setting `{}.{}` twice", spec.id, s.key));
            }
            keys.push(s.key);
            let at = format!("`{}.{}`", spec.id, s.key);
            if s.label.is_empty() || s.help.is_empty() {
                return Err(format!("{at} without a label or help"));
            }
            match &s.kind {
                Kind::Toggle { .. } => {}
                Kind::Slider {
                    min,
                    max,
                    step,
                    default,
                } => {
                    if !(min.is_finite() && max.is_finite() && min < max) {
                        return Err(format!("{at}: range {min} to {max}"));
                    }
                    if !(*step > 0.0 && *step <= max - min) {
                        return Err(format!("{at}: step {step}"));
                    }
                    if !(min..=max).contains(&default) {
                        return Err(format!("{at}: default {default} outside {min} to {max}"));
                    }
                }
                Kind::Choice { options, default } => {
                    if options.len() < 2 {
                        return Err(format!("{at}: a choice of fewer than two options"));
                    }
                    let mut seen = Vec::new();
                    for o in *options {
                        word(&format!("option of {at}"), o.id)?;
                        if seen.contains(&o.id) || o.label.is_empty() {
                            return Err(format!(
                                "{at}: option `{}` twice or without a label",
                                o.id
                            ));
                        }
                        seen.push(o.id);
                    }
                    if !seen.contains(default) {
                        return Err(format!("{at}: default `{default}` is not an option"));
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub static QUALITY: [Choice; 3] = [
        Choice {
            id: "off",
            label: "Off",
        },
        Choice {
            id: "low",
            label: "Low",
        },
        Choice {
            id: "high",
            label: "High",
        },
    ];

    /// A module made up for the tests.
    pub static AO: ModuleSpec = ModuleSpec {
        id: "ao",
        label: "Ambient occlusion",
        help: "Soft shade in corners.",
        settings: &[
            Setting {
                key: "quality",
                label: "Quality",
                help: "How many samples.",
                applies: Applies::Live,
                kind: Kind::Choice {
                    options: &QUALITY,
                    default: "off",
                },
            },
            Setting {
                key: "strength",
                label: "Strength",
                help: "How dark.",
                applies: Applies::Live,
                kind: Kind::Slider {
                    min: 0.1,
                    max: 1.0,
                    step: 0.05,
                    default: 0.7,
                },
            },
        ],
        owns: &[
            Owns::Switch("NV_AO"),
            Owns::Pass {
                name: "gtao",
                at: Insertion::AfterWorld,
            },
        ],
        cost: "measured in docs/fork/GRAPHICS.md",
    };

    /// A second one.
    pub static FOG: ModuleSpec = ModuleSpec {
        id: "fog",
        label: "Fog",
        help: "The weather's distance fog.",
        settings: &[
            Setting {
                key: "enabled",
                label: "Fog",
                help: "Off: no distance fog.",
                applies: Applies::NextStart,
                kind: Kind::Toggle { default: true },
            },
            Setting {
                key: "distance",
                label: "Distance",
                help: "Scales the fog's distances.",
                applies: Applies::Live,
                kind: Kind::Slider {
                    min: 0.5,
                    max: 4.0,
                    step: 0.25,
                    default: 1.0,
                },
            },
        ],
        owns: &[Owns::GameValue("fFogEnd")],
        cost: "none",
    };

    #[test]
    fn the_test_modules_pass_the_check() {
        check(&[&AO, &FOG]).unwrap();
        assert!(check(&[&AO, &AO]).unwrap_err().contains("twice"));
    }

    #[test]
    fn values_are_checked_against_their_kind() {
        let quality = AO.setting("quality").unwrap();
        let strength = AO.setting("strength").unwrap();
        let enabled = FOG.setting("enabled").unwrap();
        assert_eq!(
            quality.accept(&Value::Text("HIGH".into())).unwrap(),
            (Typed::Choice("high"), None)
        );
        assert!(quality
            .accept(&Value::Text("ultra".into()))
            .unwrap_err()
            .contains("off, low, high"));
        assert_eq!(
            strength.accept(&Value::Number(0.5)).unwrap(),
            (Typed::Slider(0.5), None)
        );
        let (v, note) = strength.accept(&Value::Number(3.0)).unwrap();
        assert_eq!(v, Typed::Slider(1.0));
        assert!(note.unwrap().contains("outside"));
        assert!(strength.accept(&Value::Number(f64::NAN)).is_err());
        assert!(strength
            .accept(&Value::Text("0.5".into()))
            .unwrap_err()
            .contains("not a number"));
        assert_eq!(
            enabled.accept(&Value::Bool(false)).unwrap().0,
            Typed::Toggle(false)
        );
        assert!(enabled.accept(&Value::Number(1.0)).is_err());
    }

    #[test]
    fn bad_schemas_are_refused() {
        static BAD_DEFAULT: ModuleSpec = ModuleSpec {
            id: "x",
            label: "X",
            help: "x",
            settings: &[Setting {
                key: "s",
                label: "S",
                help: "s",
                applies: Applies::Live,
                kind: Kind::Slider {
                    min: 0.0,
                    max: 1.0,
                    step: 0.1,
                    default: 2.0,
                },
            }],
            owns: &[],
            cost: "",
        };
        static BAD_ID: ModuleSpec = ModuleSpec {
            id: "Bad",
            label: "B",
            help: "b",
            settings: &[],
            owns: &[],
            cost: "",
        };
        static LAYER_ID: ModuleSpec = ModuleSpec {
            id: "cell",
            label: "C",
            help: "c",
            settings: &[],
            owns: &[],
            cost: "",
        };
        static BAD_CHOICE: ModuleSpec = ModuleSpec {
            id: "y",
            label: "Y",
            help: "y",
            settings: &[Setting {
                key: "c",
                label: "C",
                help: "c",
                applies: Applies::Live,
                kind: Kind::Choice {
                    options: &QUALITY,
                    default: "medium",
                },
            }],
            owns: &[],
            cost: "",
        };
        assert!(check(&[&BAD_DEFAULT]).unwrap_err().contains("outside"));
        assert!(check(&[&BAD_ID]).unwrap_err().contains("lower case"));
        assert!(check(&[&LAYER_ID]).unwrap_err().contains("layer"));
        assert!(check(&[&BAD_CHOICE]).unwrap_err().contains("not an option"));
    }
}
