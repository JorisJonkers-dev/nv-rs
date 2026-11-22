// SPDX-License-Identifier: GPL-3.0-only
//! The request the engine reads at start, from the environment (as `NV_STOP_AFTER_FRAMES` is).
//!
//! | Variable | Meaning |
//! | --- | --- |
//! | `NV_SCENE` | the place file; setting it turns the scene run on |
//! | `NV_SCENE_PLACES` | comma-separated place names, in the order to visit; default all |
//! | `NV_SCENE_OUT` | the folder the pictures and `scene.json` go to (made if missing) |
//! | `NV_SCENE_SETTLE_FRAMES` | frames rendered after "ready" before the picture (default 30) |
//! | `NV_SCENE_TIMEOUT_S` | seconds a place may take to be ready before the run fails (default 120) |
//! | `NV_SCENE_HOUR` | the game hour the frozen world holds (default 12) |
//! | `NV_SCENE_WEATHER` | the weather's form id in hex the frozen world holds (default 000FFC88) |
//! | `NV_SCENE_ACTORS` | `1` keeps creatures and people in the picture; by default their 3D is hidden |
//! | `NV_SCENE_SOAK_FRAMES` | frames to keep running at each place after it is ready, timing each (a soak or a bench) |
//! | `NV_SCENE_SOAK_WARMUP` | frames run at each place before the timed ones, not timed (default 120: the first frames upload textures and compile pipelines) |
//! | `NV_SCENE_SOAK_PASSES` | timed passes of `NV_SCENE_SOAK_FRAMES` each at a place (default 1); the reported average is the median of the passes' averages |
//! | `NV_SCENE_CAPTURE_FRAMES` | take the picture exactly this many main frames after the place was sent (when it is ready sooner it waits; later it fails): the picture then does not depend on the frame the place became ready at |
//! | `NV_SCENE_MENU` | `1` goes the old way: the main menu is built and the first place is sent from it (the default builds no menu: the player is sent to the first place from the first frame the game runs) |
//! | `NV_SCENE_FREEZE` | `0` leaves the world alone (time, weather, AI, interface) |

use crate::places::{Place, PlaceFile};
use std::path::PathBuf;

pub const FILE: &str = "NV_SCENE";
pub const PLACES: &str = "NV_SCENE_PLACES";
pub const OUT: &str = "NV_SCENE_OUT";
pub const SETTLE: &str = "NV_SCENE_SETTLE_FRAMES";
pub const TIMEOUT: &str = "NV_SCENE_TIMEOUT_S";
pub const HOUR: &str = "NV_SCENE_HOUR";
pub const WEATHER: &str = "NV_SCENE_WEATHER";
pub const FREEZE: &str = "NV_SCENE_FREEZE";
pub const MENU: &str = "NV_SCENE_MENU";
pub const ACTORS: &str = "NV_SCENE_ACTORS";
pub const SOAK: &str = "NV_SCENE_SOAK_FRAMES";
pub const SOAK_WARMUP: &str = "NV_SCENE_SOAK_WARMUP";
pub const SOAK_PASSES: &str = "NV_SCENE_SOAK_PASSES";
pub const CAPTURE_FRAMES: &str = "NV_SCENE_CAPTURE_FRAMES";

/// A scene run: which places, where the pictures go, and the ready condition's numbers.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub places: Vec<Place>,
    pub out: Option<PathBuf>,
    pub settle_frames: u32,
    pub timeout_s: u32,
    /// The frozen world: `Some((hour, weather form id))`, `None` for the world left alone.
    pub freeze: Option<(f32, u32)>,
    /// Hide the 3D of every actor (not the player) in the cells around the place.
    pub hide_actors: bool,
    /// Frames to run at each place after it is ready; the frame times are logged.
    pub soak_frames: Option<u32>,
    /// Frames before the timed ones that are not timed.
    pub soak_warmup: u32,
    /// Timed passes of `soak_frames` frames each.
    pub soak_passes: u32,
    /// Take the picture this many frames after the place was sent.
    pub capture_frames: Option<u32>,
    /// Build the main menu and send the first place from it (the old route); false builds no menu.
    pub menu: bool,
}

impl Request {
    /// From a lookup of variables: `Ok(None)` when `NV_SCENE` is unset or empty, an error with a plain
    /// message for a file that cannot be read or parsed or any bad value.
    pub fn read(var: impl Fn(&str) -> Option<String>) -> Result<Option<Request>, String> {
        let Some(file) = var(FILE)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
        else {
            return Ok(None);
        };
        let text = std::fs::read_to_string(&file).map_err(|e| format!("{FILE}={file}: {e}"))?;
        let parsed = PlaceFile::parse(&text).map_err(|e| format!("{FILE}={file}: {e}"))?;
        let names: Vec<String> = var(PLACES)
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(String::from)
            .collect();
        let places = parsed
            .select(&names)
            .map_err(|e| format!("{PLACES}: {e}"))?
            .into_iter()
            .cloned()
            .collect();
        let number = |name: &str, default: u32| match var(name)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
        {
            None => Ok(default),
            Some(v) => v
                .parse::<u32>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| format!("{name}={v}: a whole number above 0")),
        };
        let freeze = match var(FREEZE).as_deref().map(str::trim) {
            Some("0") => None,
            _ => {
                let hour = match var(HOUR)
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty())
                {
                    None => 12.0,
                    Some(v) => v
                        .parse::<f32>()
                        .ok()
                        .filter(|h| (0.0..24.0).contains(h))
                        .ok_or_else(|| format!("{HOUR}={v}: an hour from 0 up to 24"))?,
                };
                let weather = match var(WEATHER)
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty())
                {
                    None => 0x000f_fc88,
                    Some(v) => u32::from_str_radix(v.trim_start_matches("0x"), 16)
                        .ok()
                        .filter(|w| *w != 0)
                        .ok_or_else(|| format!("{WEATHER}={v}: a form id in hex"))?,
                };
                Some((hour, weather))
            }
        };
        Ok(Some(Request {
            places,
            out: var(OUT)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
            settle_frames: number(SETTLE, 30)?,
            timeout_s: number(TIMEOUT, 120)?,
            freeze,
            capture_frames: match var(CAPTURE_FRAMES)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
            {
                None => None,
                Some(v) => Some(
                    v.parse::<u32>()
                        .ok()
                        .filter(|n| *n > 0)
                        .ok_or_else(|| format!("{CAPTURE_FRAMES}={v}: a whole number above 0"))?,
                ),
            },
            soak_frames: match var(SOAK)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
            {
                None => None,
                Some(v) => Some(
                    v.parse::<u32>()
                        .ok()
                        .filter(|n| *n > 0)
                        .ok_or_else(|| format!("{SOAK}={v}: a whole number above 0"))?,
                ),
            },
            soak_warmup: match var(SOAK_WARMUP)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
            {
                None => 120,
                Some(v) => v
                    .parse::<u32>()
                    .map_err(|_| format!("{SOAK_WARMUP}={v}: a whole number"))?,
            },
            soak_passes: number(SOAK_PASSES, 1)?.max(1),
            menu: var(MENU).as_deref().map(str::trim) == Some("1"),
            hide_actors: var(ACTORS).as_deref().map(str::trim) != Some("1"),
        }))
    }

    /// [`Request::read`] of the process's environment.
    pub fn from_env() -> Result<Option<Request>, String> {
        Self::read(|n| std::env::var(n).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn file(dir: &std::path::Path) -> String {
        let p = dir.join("places.toml");
        std::fs::write(
            &p,
            "version = 1\n[[place]]\nname = \"a\"\nworld = \"W\"\nx=1\ny=2\nz=3\n[[place]]\nname = \"b\"\nworld = \"W\"\nx=4\ny=5\nz=6\n",
        )
        .unwrap();
        p.display().to_string()
    }

    fn with(vars: &[(&str, String)]) -> Result<Option<Request>, String> {
        let m: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        Request::read(|n| m.get(n).cloned())
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fork-scene-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn unset_or_empty_is_off() {
        assert_eq!(with(&[]).unwrap(), None);
        assert_eq!(with(&[(FILE, " ".into())]).unwrap(), None);
    }

    #[test]
    fn no_menu_is_built_unless_nv_scene_menu_is_1() {
        let d = tmp("menu");
        let f = file(&d);
        assert!(!with(&[(FILE, f.clone())]).unwrap().unwrap().menu);
        assert!(
            !with(&[(FILE, f.clone()), (MENU, "0".into())])
                .unwrap()
                .unwrap()
                .menu
        );
        assert!(
            with(&[(FILE, f), (MENU, "1".into())])
                .unwrap()
                .unwrap()
                .menu
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn all_places_and_the_defaults() {
        let d = tmp("all");
        let r = with(&[(FILE, file(&d))]).unwrap().unwrap();
        assert_eq!(r.places.len(), 2);
        assert_eq!((r.settle_frames, r.timeout_s, r.out), (30, 120, None));
        assert_eq!(r.freeze, Some((12.0, 0x000f_fc88)));
        assert!(r.hide_actors);
    }

    #[test]
    fn named_places_keep_their_order_and_the_numbers_are_read() {
        let d = tmp("named");
        let r = with(&[
            (FILE, file(&d)),
            (PLACES, "b, a".into()),
            (OUT, "/x".into()),
            (SETTLE, "5".into()),
            (TIMEOUT, "9".into()),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(
            r.places.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            ["b", "a"]
        );
        assert_eq!(
            (r.settle_frames, r.timeout_s, r.out),
            (5, 9, Some(PathBuf::from("/x")))
        );
    }

    #[test]
    fn the_frozen_world_has_its_hour_and_weather_or_is_off() {
        let d = tmp("freeze");
        let r = with(&[
            (FILE, file(&d)),
            (HOUR, "6.5".into()),
            (WEATHER, "0x1234".into()),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(r.freeze, Some((6.5, 0x1234)));
        let off = with(&[(FILE, file(&d)), (FREEZE, "0".into())])
            .unwrap()
            .unwrap();
        assert_eq!(off.freeze, None);
    }

    #[test]
    fn actors_stay_in_the_picture_only_when_asked() {
        let d = tmp("actors");
        let keep = with(&[(FILE, file(&d)), (ACTORS, "1".into())])
            .unwrap()
            .unwrap();
        assert!(!keep.hide_actors);
    }

    #[test]
    fn the_soak_length_is_a_positive_whole_number() {
        let d = tmp("soak");
        let r = with(&[(FILE, file(&d)), (SOAK, "500".into())])
            .unwrap()
            .unwrap();
        assert_eq!(r.soak_frames, Some(500));
        assert!(with(&[(FILE, file(&d)), (SOAK, "0".into())])
            .unwrap_err()
            .contains("NV_SCENE_SOAK_FRAMES=0"));
    }

    #[test]
    fn bad_input_is_refused_with_the_variable_named() {
        let d = tmp("bad");
        for (vars, want) in [
            (
                vec![(FILE, "/no/such/file".to_string())],
                "NV_SCENE=/no/such/file",
            ),
            (
                vec![(FILE, file(&d)), (PLACES, "zzz".into())],
                "NV_SCENE_PLACES",
            ),
            (
                vec![(FILE, file(&d)), (SETTLE, "0".into())],
                "NV_SCENE_SETTLE_FRAMES=0",
            ),
            (
                vec![(FILE, file(&d)), (TIMEOUT, "x".into())],
                "NV_SCENE_TIMEOUT_S=x",
            ),
        ] {
            let e = with(&vars).unwrap_err();
            assert!(e.contains(want), "{e}");
        }
    }
}
