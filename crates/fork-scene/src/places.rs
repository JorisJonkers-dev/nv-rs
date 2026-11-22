// SPDX-License-Identifier: GPL-3.0-only
//! The place file: a small, versioned subset of TOML (no dependency: the crate list is frozen by
//! the package age rule, and the file has one shape).
//!
//! ```toml
//! version = 1
//! [[place]]
//! name = "lake-mead"
//! world = "WastelandNV"
//! x = 53799.8
//! y = 38375.3
//! z = 2917.4
//! heading = 60
//! pitch = 0
//! path = [[0, 0, 0], [10, 0, 0]]   # optional, for a bench: points to walk through
//! ```
//! A place by reference instead of coordinates (the reference's editor id or form id in hex; the
//! world is the reference's own):
//! ```toml
//! [[place]]
//! name = "helios-from-the-south"
//! ref = "NovacMapMarker"          # stand at this reference ...
//! dx = 0                          # ... plus this offset (world axes, default 0)
//! dy = -9000
//! dz = 400
//! look_at = "VHeliosONEMapMarkerREF"   # face this reference (heading and pitch from the two positions)
//! ```
//! An interior, by the cell's editor id (the player is placed into the cell the way a door or `coc` does; no world space):
//! ```toml
//! [[place]]
//! name = "docs-house"
//! cell = "GSDocMitchellHouse"     # alone: at the cell's own start marker (what `coc` uses), facing its heading
//! # dx = 0  dy = 0  dz = 0        # ... plus an offset, or
//! # x = 0   y = 0   z = 0         # a position in the cell's own coordinates, or
//! # ref = "SomeMarkerRef"         # a marker or door reference placed in the cell (editor id or form id in hex), plus dx, dy, dz
//! ```
//! The view from the free camera (the player stands where the place says, the loaded cells around it are the
//! near land, the camera is elsewhere; no console text):
//! ```toml
//! camera_dz = 6000                 # 6000 units above the player (also camera_dx, camera_dy), or
//! # camera = [x, y, z]             # a world position
//! look = [x, y, z]                 # look at a point (or `look_at`, or `heading` and `pitch`)
//! ```
//! The lens (any place, with the player's eyes or the free camera):
//! ```toml
//! fov = 20          # vertical field of view in degrees (a narrow lens looks at one far thing as if near)
//! far = 400000      # far plane in units
//! fog = false       # no fog: far land and objects at full contrast
//! ```
//! A place for a bench can walk: `path = [[x, y, z], ...]` and `path_speed = 10` (units a frame) move the player along the
//! points (and back to the first) during the soak's frames.
//! Supported: `#` comments, `key = value` with a quoted string, a number, true or false, or an array of
//! numbers or of arrays of numbers on one line; `[[place]]` tables; the top-level `version`.

/// The newest format version this build reads.
pub const VERSION: u32 = 1;

/// One place: where the player stands and which way the camera looks.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub name: String,
    /// The world space's editor id (`WastelandNV`); empty for a place by reference (its world).
    pub world: String,
    /// Position in game units. `z` is the standing height: the engine checks the player stands. For a
    /// place by reference: the offset's origin is the reference, this is zero until the engine resolves it.
    pub pos: [f64; 3],
    /// Degrees: 0 north, 90 east. With `look_at`: the default is the direction to it.
    pub heading: f64,
    /// Degrees: positive looks down. With `look_at`: the default is the angle to it.
    pub pitch: f64,
    /// An interior: the editor id of the cell (`cell = "GSDocMitchellHouse"`). The player is sent there through the game's
    /// own placement into a cell (what a door or `coc` ends in); there is no world space. Empty for an exterior.
    pub cell: String,
    /// An interior place with no `x`, `y`, `z` and no `ref`: it starts at the cell's own start marker (the one `coc` uses),
    /// plus [`Place::offset`].
    pub at_start: bool,
    /// Stand at this reference (editor id, or form id in hex) plus [`Place::offset`].
    pub reference: Option<String>,
    /// The offset from the reference, world axes.
    pub offset: [f64; 3],
    /// Face this reference (editor id, or form id in hex).
    pub look_at: Option<String>,
    /// `heading` and `pitch` were given (they then win over `look_at`).
    pub heading_given: bool,
    pub pitch_given: bool,
    /// Move the player as the console's `player.setpos` / `setangle` do (nv-rs#474), not through the game's own
    /// placement (`move = "setpos"`; the default is `move = "place"`).
    pub setpos: bool,
    /// Move the player the way the game's own far moves do (`move = "request"`): a queued position request
    /// (`PlayerCharacter::RequestPositionPlayer`, `0093be30`), handled by the game's own player update, which places the
    /// player in the world, sets the character controller's height and runs its callback.
    pub request: bool,
    /// Points to walk through (a bench: in a soak or bench the player moves along them, `path_speed` units a frame, the
    /// whole path again when it ends); empty for a place to stand at.
    pub path: Vec<[f64; 3]>,
    /// Units the player moves each frame along `path` (default 10, about a run).
    pub path_speed: f64,
    /// The view is taken from the free camera, not from the player's eyes (the console's `tfc`, with no
    /// console text): the player stands where the place says and the cells around it stay loaded, the camera
    /// is somewhere else. `None` for the player's own view.
    pub camera: Option<Camera>,
    /// With a camera: look at this point (heading and pitch from the camera's position to it); `look_at`
    /// (a reference) and `heading` / `pitch` are the other ways to aim.
    pub look: Option<[f64; 3]>,
    /// The field of view in degrees (`fov = 20` is a narrow lens), for the player's eyes or the free camera. `None`:
    /// the game's own.
    pub fov: Option<f64>,
    /// The far plane's distance in units (`far = 400000`): the game's own `fNoLODFarDistance` settings' value.
    /// `None`: the game's own.
    pub far: Option<f64>,
    /// Fog on (`fog = false`: the sky's fog distances pushed out of reach, so far land and objects show at full
    /// contrast). Default true.
    pub fog: bool,
}

/// Where the free camera is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Camera {
    /// A world position (`camera = [x, y, z]`).
    At([f64; 3]),
    /// An offset from where the player stands (`camera_dx`, `camera_dy`, `camera_dz`; `camera_dz = 3000`
    /// is a view from 3000 units above the player).
    Above([f64; 3]),
}

impl Place {
    /// The exterior grid cell holding the position (a cell is 4096 units).
    pub fn cell(&self) -> (i32, i32) {
        cell_of(self.pos)
    }
}

/// The exterior grid cell holding a position.
pub fn cell_of(pos: [f64; 3]) -> (i32, i32) {
    (
        (pos[0] / 4096.0).floor() as i32,
        (pos[1] / 4096.0).floor() as i32,
    )
}

/// The point `dist` units along the polyline `path` (a loop: past its end it starts again from the first point, which
/// the last point is joined to); the first point for a path of one point or no length.
pub fn path_point(path: &[[f64; 3]], dist: f64) -> Option<[f64; 3]> {
    let first = *path.first()?;
    let n = path.len();
    let leg = |i: usize| {
        let (a, b) = (path[i], path[(i + 1) % n]);
        let d = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt();
        (a, b, d)
    };
    let total: f64 = (0..n).map(|i| leg(i).2).sum();
    if n == 1 || total <= 0.0 {
        return Some(first);
    }
    let mut left = dist.rem_euclid(total);
    for i in 0..n {
        let (a, b, d) = leg(i);
        if left <= d {
            let t = if d > 0.0 { left / d } else { 0.0 };
            return Some([
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t,
                a[2] + (b[2] - a[2]) * t,
            ]);
        }
        left -= d;
    }
    Some(first)
}

/// Heading (0 north, 90 east) and pitch (positive looks down), in degrees, for standing at `from` and
/// facing `to`.
pub fn aim(from: [f64; 3], to: [f64; 3]) -> (f64, f64) {
    let (dx, dy, dz) = (to[0] - from[0], to[1] - from[1], to[2] - from[2]);
    let heading = dx.atan2(dy).to_degrees().rem_euclid(360.0);
    let pitch = -dz.atan2(dx.hypot(dy)).to_degrees();
    (heading, pitch)
}

/// A parsed place file.
#[derive(Clone, Debug, PartialEq)]
pub struct PlaceFile {
    pub version: u32,
    pub places: Vec<Place>,
}

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Str(String),
    Bool(bool),
    Num(f64),
    List(Vec<Value>),
}

impl PlaceFile {
    /// Parses `text`; every error names its line.
    pub fn parse(text: &str) -> Result<PlaceFile, String> {
        let mut version = None;
        let mut tables: Vec<(usize, Vec<(String, Value)>)> = Vec::new();
        for (i, raw) in text.lines().enumerate() {
            let n = i + 1;
            let line = strip_comment(raw).trim();
            if line.is_empty() {
                continue;
            }
            if line == "[[place]]" {
                tables.push((n, Vec::new()));
                continue;
            }
            if line.starts_with('[') {
                return Err(format!(
                    "line {n}: only [[place]] tables are known, found {line}"
                ));
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| format!("line {n}: expected `key = value`"))?;
            let key = key.trim().to_string();
            let value = parse_value(value.trim()).map_err(|e| format!("line {n}: {e}"))?;
            match tables.last_mut() {
                Some((_, t)) => {
                    if t.iter().any(|(k, _)| *k == key) {
                        return Err(format!("line {n}: `{key}` is given twice in this place"));
                    }
                    t.push((key, value));
                }
                None if key == "version" => match value {
                    Value::Num(v) if v.fract() == 0.0 && v >= 1.0 => version = Some(v as u32),
                    _ => return Err(format!("line {n}: version is a whole number from 1")),
                },
                None => return Err(format!("line {n}: `{key}` outside a [[place]] table")),
            }
        }
        let version = version.ok_or("the file has no `version = 1` line")?;
        if version > VERSION {
            return Err(format!(
                "place file version {version} is newer than this build reads ({VERSION})"
            ));
        }
        let mut places = Vec::new();
        for (n, t) in tables {
            let p = place_of(n, &t)?;
            if places.iter().any(|q: &Place| q.name == p.name) {
                return Err(format!(
                    "line {n}: the place name `{}` is used twice",
                    p.name
                ));
            }
            places.push(p);
        }
        if places.is_empty() {
            return Err("the file has no [[place]]".into());
        }
        Ok(PlaceFile { version, places })
    }

    /// The places named in `names` in that order; all of them when `names` is empty.
    pub fn select(&self, names: &[String]) -> Result<Vec<&Place>, String> {
        if names.is_empty() {
            return Ok(self.places.iter().collect());
        }
        names
            .iter()
            .map(|n| {
                self.places.iter().find(|p| &p.name == n).ok_or_else(|| {
                    let known: Vec<&str> = self.places.iter().map(|p| p.name.as_str()).collect();
                    format!("no place `{n}` in the file (it has: {})", known.join(", "))
                })
            })
            .collect()
    }
}

fn place_of(n: usize, t: &[(String, Value)]) -> Result<Place, String> {
    let get = |k: &str| t.iter().find(|(key, _)| key == k).map(|(_, v)| v);
    let num = |k: &str, default: Option<f64>| match get(k) {
        Some(Value::Num(v)) => Ok(*v),
        Some(_) => Err(format!("line {n}: `{k}` is a number")),
        None => default.ok_or_else(|| format!("line {n}: the place has no `{k}`")),
    };
    let string = |k: &str| match get(k) {
        Some(Value::Str(s)) if !s.is_empty() => Ok(s.clone()),
        Some(_) => Err(format!("line {n}: `{k}` is a non-empty string")),
        None => Err(format!("line {n}: the place has no `{k}`")),
    };
    for (k, _) in t {
        if ![
            "name",
            "world",
            "cell",
            "x",
            "y",
            "z",
            "heading",
            "pitch",
            "path",
            "ref",
            "dx",
            "dy",
            "dz",
            "look_at",
            "move",
            "path_speed",
            "camera",
            "camera_dx",
            "camera_dy",
            "camera_dz",
            "look",
            "fov",
            "far",
            "fog",
        ]
        .contains(&k.as_str())
        {
            return Err(format!("line {n}: unknown key `{k}` in a place"));
        }
    }
    let name = string("name")?;
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!(
            "line {n}: the name `{name}` may hold letters, digits, - and _ only (it names a picture file)"
        ));
    }
    let path = match get("path") {
        None => Vec::new(),
        Some(Value::List(points)) => points
            .iter()
            .map(|p| match p {
                Value::List(v) if v.len() == 3 => {
                    let mut a = [0.0; 3];
                    for (i, c) in v.iter().enumerate() {
                        match c {
                            Value::Num(x) => a[i] = *x,
                            _ => return Err(format!("line {n}: a path point is three numbers")),
                        }
                    }
                    Ok(a)
                }
                _ => Err(format!("line {n}: a path point is three numbers")),
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(format!("line {n}: `path` is a list of [x, y, z] points")),
    };
    let optional_string = |k: &str| match get(k) {
        None => Ok(None),
        Some(Value::Str(s)) if !s.is_empty() => Ok(Some(s.clone())),
        Some(_) => Err(format!("line {n}: `{k}` is a non-empty string")),
    };
    let (setpos, request) = match optional_string("move")?.as_deref() {
        None | Some("place") => (false, false),
        Some("setpos") => (true, false),
        Some("request") => (false, true),
        Some(other) => {
            return Err(format!(
                "line {n}: `move` is \"place\", \"request\" or \"setpos\", not `{other}`"
            ))
        }
    };
    let reference = optional_string("ref")?;
    let look_at = optional_string("look_at")?;
    let cell = optional_string("cell")?.unwrap_or_default();
    let xyz = ["x", "y", "z"].iter().filter(|k| get(k).is_some()).count();
    let mut at_start = false;
    let (pos, offset, world) = if !cell.is_empty() {
        if get("world").is_some() {
            return Err(format!(
                "line {n}: a place has a `world` (an exterior) or a `cell` (an interior), not both"
            ));
        }
        if reference.is_some() {
            if xyz != 0 {
                return Err(format!(
                    "line {n}: a place by `ref` takes dx, dy, dz, not x, y, z"
                ));
            }
            (
                [0.0; 3],
                [
                    num("dx", Some(0.0))?,
                    num("dy", Some(0.0))?,
                    num("dz", Some(0.0))?,
                ],
                String::new(),
            )
        } else if xyz == 0 {
            at_start = true;
            (
                [0.0; 3],
                [
                    num("dx", Some(0.0))?,
                    num("dy", Some(0.0))?,
                    num("dz", Some(0.0))?,
                ],
                String::new(),
            )
        } else {
            if ["dx", "dy", "dz"].iter().any(|k| get(k).is_some()) {
                return Err(format!(
                    "line {n}: dx, dy and dz need a `ref` or no x, y, z"
                ));
            }
            (
                [num("x", None)?, num("y", None)?, num("z", None)?],
                [0.0; 3],
                String::new(),
            )
        }
    } else if reference.is_some() {
        if xyz != 0 {
            return Err(format!(
                "line {n}: a place by `ref` takes dx, dy, dz, not x, y, z"
            ));
        }
        let off = [
            num("dx", Some(0.0))?,
            num("dy", Some(0.0))?,
            num("dz", Some(0.0))?,
        ];
        let world = optional_string("world")?.unwrap_or_default();
        ([0.0; 3], off, world)
    } else {
        if ["dx", "dy", "dz"].iter().any(|k| get(k).is_some()) {
            return Err(format!("line {n}: dx, dy and dz need a `ref`"));
        }
        (
            [num("x", None)?, num("y", None)?, num("z", None)?],
            [0.0; 3],
            string("world")?,
        )
    };
    let point = |k: &str| -> Result<Option<[f64; 3]>, String> {
        match get(k) {
            None => Ok(None),
            Some(Value::List(v)) if v.len() == 3 => {
                let mut a = [0.0; 3];
                for (i, c) in v.iter().enumerate() {
                    match c {
                        Value::Num(x) => a[i] = *x,
                        _ => return Err(format!("line {n}: `{k}` is three numbers [x, y, z]")),
                    }
                }
                Ok(Some(a))
            }
            Some(_) => Err(format!("line {n}: `{k}` is three numbers [x, y, z]")),
        }
    };
    let offsets = ["camera_dx", "camera_dy", "camera_dz"];
    let camera = match (point("camera")?, offsets.iter().any(|k| get(k).is_some())) {
        (Some(_), true) => {
            return Err(format!(
                "line {n}: `camera` and camera_dx, camera_dy, camera_dz are two ways to say where the camera is"
            ))
        }
        (Some(at), false) => Some(Camera::At(at)),
        (None, true) => Some(Camera::Above([
            num("camera_dx", Some(0.0))?,
            num("camera_dy", Some(0.0))?,
            num("camera_dz", Some(0.0))?,
        ])),
        (None, false) => None,
    };
    let look = point("look")?;
    if look.is_some() && camera.is_none() {
        return Err(format!("line {n}: `look` needs a camera"));
    }
    let optional_num = |k: &str| match get(k) {
        None => Ok(None),
        Some(Value::Num(v)) if *v > 0.0 => Ok(Some(*v)),
        Some(_) => Err(format!("line {n}: `{k}` is a number above 0")),
    };
    let fov = optional_num("fov")?;
    if fov.is_some_and(|v| v >= 180.0) {
        return Err(format!(
            "line {n}: `fov` is the vertical angle in degrees, below 180"
        ));
    }
    let far = optional_num("far")?;
    let fog = match get("fog") {
        None => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(format!("line {n}: `fog` is true or false")),
    };
    let heading = num("heading", Some(0.0))?;
    let pitch = num("pitch", Some(0.0))?;
    if !(-360.0..=360.0).contains(&heading) || !(-89.0..=89.0).contains(&pitch) {
        return Err(format!(
            "line {n}: heading is within +-360 degrees, pitch within +-89"
        ));
    }
    Ok(Place {
        name,
        world,
        cell,
        at_start,
        pos,
        heading,
        pitch,
        reference,
        offset,
        look_at,
        heading_given: get("heading").is_some(),
        pitch_given: get("pitch").is_some(),
        setpos,
        request,
        path,
        path_speed: num("path_speed", Some(10.0))?,
        camera,
        look,
        fov,
        far,
        fog,
    })
}

fn strip_comment(line: &str) -> &str {
    let mut quoted = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '#' if !quoted => return &line[..i],
            _ => {}
        }
    }
    line
}

fn parse_value(s: &str) -> Result<Value, String> {
    if let Some(rest) = s.strip_prefix('"') {
        return match rest.strip_suffix('"') {
            Some(inner) if !inner.contains('"') && !inner.contains('\\') => {
                Ok(Value::Str(inner.into()))
            }
            _ => Err("a string is in double quotes, with no escapes".into()),
        };
    }
    if let Some(inner) = s.strip_prefix('[') {
        let inner = inner
            .strip_suffix(']')
            .ok_or("a list is closed with ] on the same line")?;
        let mut items = Vec::new();
        let (mut depth, mut start) = (0usize, 0usize);
        for (i, c) in inner.char_indices() {
            match c {
                '[' => depth += 1,
                ']' => depth = depth.checked_sub(1).ok_or("unbalanced ]")?,
                ',' if depth == 0 => {
                    items.push(&inner[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
        }
        if depth != 0 {
            return Err("unbalanced [".into());
        }
        items.push(&inner[start..]);
        return items
            .into_iter()
            .filter(|t| !t.trim().is_empty())
            .map(|t| parse_value(t.trim()))
            .collect::<Result<_, _>>()
            .map(Value::List);
    }
    match s {
        "true" => return Ok(Value::Bool(true)),
        "false" => return Ok(Value::Bool(false)),
        _ => {}
    }
    s.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .map(Value::Num)
        .ok_or_else(|| format!("`{s}` is not a number, true, false, a quoted string or a list"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
# the first two views
version = 1

[[place]]
name = "lake-mead"   # beach
world = "WastelandNV"
x = 53799.8
y = 38375.3
z = 2917.4
heading = 60
pitch = 0

[[place]]
name = "colorado-river"
world = "WastelandNV"
x = 62699
y = -97604
z = 20.0
heading = 90
path = [[0, 0, 0], [10.5, 0, 0]]
"#;

    #[test]
    fn reads_places_in_order_with_defaults() {
        let f = PlaceFile::parse(GOOD).unwrap();
        assert_eq!(f.version, 1);
        assert_eq!(f.places.len(), 2);
        assert_eq!(f.places[0].name, "lake-mead");
        assert_eq!(f.places[0].pos, [53799.8, 38375.3, 2917.4]);
        assert_eq!(f.places[0].heading, 60.0);
        assert_eq!(f.places[1].pitch, 0.0);
        assert_eq!(f.places[1].path, vec![[0.0; 3], [10.5, 0.0, 0.0]]);
    }

    #[test]
    fn the_cell_is_the_floor_of_the_position_over_4096() {
        let f = PlaceFile::parse(GOOD).unwrap();
        assert_eq!(f.places[0].cell(), (13, 9));
        assert_eq!(f.places[1].cell(), (15, -24));
    }

    #[test]
    fn select_keeps_the_asked_order_and_names_unknown_places() {
        let f = PlaceFile::parse(GOOD).unwrap();
        let s = f
            .select(&["colorado-river".into(), "lake-mead".into()])
            .unwrap();
        assert_eq!(s[0].name, "colorado-river");
        assert_eq!(f.select(&[]).unwrap().len(), 2);
        let e = f.select(&["nowhere".into()]).unwrap_err();
        assert!(e.contains("nowhere") && e.contains("lake-mead"), "{e}");
    }

    #[test]
    fn errors_name_the_line() {
        let cases = [
            ("[[place]]\nname = \"a\"", "no `version"),
            ("version = 2\n[[place]]\n", "newer"),
            ("version = 1\n", "no [[place]]"),
            (
                "version = 1\n[[place]]\nname = \"a b\"\nworld = \"W\"\nx=1\ny=1\nz=1",
                "letters, digits",
            ),
            (
                "version = 1\n[[place]]\nname = \"a\"\nworld = \"W\"\nx=1\ny=1",
                "no `z`",
            ),
            (
                "version = 1\n[[place]]\nname = \"a\"\nbogus = 1",
                "unknown key",
            ),
            ("version = 1\nx = 1", "line 2"),
            ("version = 1\n[[other]]", "line 2"),
            (
                "version = 1\n[[place]]\nname = \"a\"\nname = \"b\"",
                "twice",
            ),
            (
                "version = 1\n[[place]]\nname = \"a\"\nworld = \"W\"\nx=1\ny=1\nz=1\npitch = 120",
                "pitch",
            ),
        ];
        for (text, want) in cases {
            let e = PlaceFile::parse(text).unwrap_err();
            assert!(e.contains(want), "{text:?} gave {e}");
        }
    }

    #[test]
    fn a_place_by_reference_has_an_offset_and_something_to_look_at() {
        let t = "version = 1\n[[place]]\nname = \"a\"\nref = \"NovacMapMarker\"\ndy = -9000\ndz = 400\nlook_at = \"VHeliosONEMapMarkerREF\"\n";
        let p = &PlaceFile::parse(t).unwrap().places[0];
        assert_eq!(p.reference.as_deref(), Some("NovacMapMarker"));
        assert_eq!(p.offset, [0.0, -9000.0, 400.0]);
        assert_eq!(p.look_at.as_deref(), Some("VHeliosONEMapMarkerREF"));
        assert!(p.world.is_empty() && !p.heading_given && !p.pitch_given);
        let with_heading = format!("{t}heading = 90\n");
        assert!(PlaceFile::parse(&with_heading).unwrap().places[0].heading_given);
    }

    #[test]
    fn an_interior_is_a_cell_by_editor_id_alone_with_an_offset_or_with_coordinates() {
        let head = "version = 1\n[[place]]\nname = \"a\"\ncell = \"GSDocMitchellHouse\"\n";
        let p = &PlaceFile::parse(head).unwrap().places[0];
        assert_eq!(p.cell, "GSDocMitchellHouse");
        assert!(p.at_start && p.world.is_empty());
        let p = &PlaceFile::parse(&format!("{head}dz = 50\n"))
            .unwrap()
            .places[0];
        assert!(p.at_start);
        assert_eq!(p.offset, [0.0, 0.0, 50.0]);
        let p = &PlaceFile::parse(&format!("{head}x = 1\ny = 2\nz = 3\n"))
            .unwrap()
            .places[0];
        assert!(!p.at_start);
        assert_eq!(p.pos, [1.0, 2.0, 3.0]);
        let p = &PlaceFile::parse(&format!("{head}ref = \"Marker\"\ndy = 5\n"))
            .unwrap()
            .places[0];
        assert!(!p.at_start && p.reference.as_deref() == Some("Marker"));
        for (extra, want) in [
            ("world = \"W\"\n", "not both"),
            ("x = 1\ny = 2\nz = 3\ndz = 4\n", "need a `ref`"),
            ("ref = \"M\"\nx = 1\ny = 2\nz = 3\n", "takes dx"),
            ("x = 1\n", "no `y`"),
        ] {
            let e = PlaceFile::parse(&format!("{head}{extra}")).unwrap_err();
            assert!(e.contains(want), "{extra:?} gave {e}");
        }
    }

    #[test]
    fn the_move_is_the_games_placement_or_the_consoles_setpos() {
        let base = "version = 1\n[[place]]\nname = \"a\"\nworld = \"W\"\nx = 1\ny = 1\nz = 1\n";
        assert!(!PlaceFile::parse(base).unwrap().places[0].setpos);
        assert!(
            PlaceFile::parse(&format!("{base}move = \"setpos\"\n"))
                .unwrap()
                .places[0]
                .setpos
        );
        assert!(PlaceFile::parse(&format!("{base}move = \"teleport\"\n"))
            .unwrap_err()
            .contains("setpos"));
    }

    #[test]
    fn coordinates_and_references_do_not_mix() {
        let both = "version = 1\n[[place]]\nname = \"a\"\nref = \"R\"\nx = 1\ny = 1\nz = 1\n";
        assert!(PlaceFile::parse(both)
            .unwrap_err()
            .contains("takes dx, dy, dz"));
        let stray =
            "version = 1\n[[place]]\nname = \"a\"\nworld = \"W\"\nx = 1\ny = 1\nz = 1\ndx = 5\n";
        assert!(PlaceFile::parse(stray)
            .unwrap_err()
            .contains("need a `ref`"));
    }

    #[test]
    fn aim_gives_compass_heading_and_a_pitch_that_looks_down_for_a_lower_target() {
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        let (h, p) = aim([0.0; 3], [0.0, 100.0, 0.0]);
        assert!(close(h, 0.0) && close(p, 0.0));
        let (h, _) = aim([0.0; 3], [100.0, 0.0, 0.0]);
        assert!(close(h, 90.0));
        let (h, _) = aim([0.0; 3], [-100.0, 0.0, 0.0]);
        assert!(close(h, 270.0));
        let (_, p) = aim([0.0; 3], [100.0, 0.0, -100.0]);
        assert!(close(p, 45.0), "a target below is looked down at: {p}");
        let (_, p) = aim([0.0; 3], [0.0, 100.0, 100.0]);
        assert!(close(p, -45.0));
    }

    #[test]
    fn duplicate_names_are_refused() {
        let one = "[[place]]\nname = \"a\"\nworld = \"W\"\nx = 1\ny = 1\nz = 1\n";
        let e = PlaceFile::parse(&format!("version = 1\n{one}{one}")).unwrap_err();
        assert!(e.contains("used twice"), "{e}");
    }

    #[test]
    fn a_hash_inside_a_string_is_not_a_comment() {
        let t = "version = 1\n[[place]]\nname = \"a\"\nworld = \"W#1\"\nx=1\ny=2\nz=3";
        assert_eq!(PlaceFile::parse(t).unwrap().places[0].world, "W#1");
    }

    #[test]
    fn a_camera_is_a_world_position_or_an_offset_and_may_look_at_a_point() {
        let base = "version = 1\n[[place]]\nname = \"a\"\nworld = \"W\"\nx = 1\ny = 2\nz = 3\n";
        let none = PlaceFile::parse(base).unwrap();
        assert_eq!(none.places[0].camera, None);
        let up = PlaceFile::parse(&format!("{base}camera_dz = 6000\ncamera_dx = -10\n")).unwrap();
        assert_eq!(
            up.places[0].camera,
            Some(Camera::Above([-10.0, 0.0, 6000.0]))
        );
        let at =
            PlaceFile::parse(&format!("{base}camera = [5, 6, 7]\nlook = [1, 1, 1]\n")).unwrap();
        assert_eq!(at.places[0].camera, Some(Camera::At([5.0, 6.0, 7.0])));
        assert_eq!(at.places[0].look, Some([1.0; 3]));
        assert!(
            PlaceFile::parse(&format!("{base}camera = [5, 6, 7]\ncamera_dz = 1\n"))
                .unwrap_err()
                .contains("two ways")
        );
        assert!(PlaceFile::parse(&format!("{base}look = [1, 1, 1]\n"))
            .unwrap_err()
            .contains("needs a camera"));
        assert!(PlaceFile::parse(&format!("{base}camera = [5, 6]\n"))
            .unwrap_err()
            .contains("three numbers"));
    }

    #[test]
    fn a_path_is_walked_by_distance_and_loops() {
        let p = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0]];
        assert_eq!(path_point(&p, 0.0), Some([0.0, 0.0, 0.0]));
        assert_eq!(path_point(&p, 5.0), Some([5.0, 0.0, 0.0]));
        assert_eq!(path_point(&p, 15.0), Some([10.0, 5.0, 0.0]));
        // The loop back to the first point: 20 along the legs, 14.14 back; 25 is 5 into the way home.
        let back = path_point(&p, 25.0).unwrap();
        assert!(
            (back[0] - 10.0 + 5.0 / 2f64.sqrt()).abs() < 1e-9,
            "{back:?}"
        );
        let total = 20.0 + 200f64.sqrt();
        assert_eq!(path_point(&p, total + 5.0), path_point(&p, 5.0));
        assert_eq!(path_point(&[[1.0, 2.0, 3.0]], 99.0), Some([1.0, 2.0, 3.0]));
        assert_eq!(path_point(&[], 1.0), None);
    }

    #[test]
    fn a_place_may_walk_a_path_at_a_speed() {
        let t = "version = 1\n[[place]]\nname = \"a\"\nworld = \"W\"\nx = 1\ny = 2\nz = 3\npath = [[0, 0, 0], [10, 0, 0]]\npath_speed = 4\n";
        let p = &PlaceFile::parse(t).unwrap().places[0];
        assert_eq!((p.path.len(), p.path_speed), (2, 4.0));
        assert_eq!(
            PlaceFile::parse(&t.replace("path_speed = 4\n", ""))
                .unwrap()
                .places[0]
                .path_speed,
            10.0
        );
    }

    #[test]
    fn the_move_may_be_a_queued_request_like_the_games_own() {
        let t = "version = 1\n[[place]]\nname = \"a\"\nworld = \"W\"\nx = 1\ny = 2\nz = 3\nmove = \"request\"\n";
        let p = &PlaceFile::parse(t).unwrap().places[0];
        assert!(p.request && !p.setpos);
        assert!(PlaceFile::parse(&t.replace("request", "teleport"))
            .unwrap_err()
            .contains("request"));
    }
}
