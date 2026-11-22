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
//! Supported: `#` comments, `key = value` with a quoted string, a number, or an array of
//! numbers or of arrays of numbers on one line; `[[place]]` tables; the top-level `version`.

/// The newest format version this build reads.
pub const VERSION: u32 = 1;

/// One place: where the player stands and which way the camera looks.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub name: String,
    /// The world space's editor id (`WastelandNV`).
    pub world: String,
    /// Position in game units. `z` is the standing height: the engine checks the player stands.
    pub pos: [f64; 3],
    /// Degrees: 0 north, 90 east.
    pub heading: f64,
    /// Degrees: positive looks down.
    pub pitch: f64,
    /// Points to walk through (a bench); empty for a still picture.
    pub path: Vec<[f64; 3]>,
}

impl Place {
    /// The exterior grid cell holding the position (a cell is 4096 units).
    pub fn cell(&self) -> (i32, i32) {
        (
            (self.pos[0] / 4096.0).floor() as i32,
            (self.pos[1] / 4096.0).floor() as i32,
        )
    }
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
        if !["name", "world", "x", "y", "z", "heading", "pitch", "path"].contains(&k.as_str()) {
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
    let heading = num("heading", Some(0.0))?;
    let pitch = num("pitch", Some(0.0))?;
    if !(-360.0..=360.0).contains(&heading) || !(-89.0..=89.0).contains(&pitch) {
        return Err(format!(
            "line {n}: heading is within +-360 degrees, pitch within +-89"
        ));
    }
    Ok(Place {
        name,
        world: string("world")?,
        pos: [num("x", None)?, num("y", None)?, num("z", None)?],
        heading,
        pitch,
        path,
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
    s.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .map(Value::Num)
        .ok_or_else(|| format!("`{s}` is not a number, a quoted string or a list"))
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
}
