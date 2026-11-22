// SPDX-License-Identifier: GPL-3.0-only
//! A TOML file kept exactly as it was written, with its keys found by their full path.
//!
//! The file is a list of pieces, one per line, or more lines for a value that goes on over lines.
//! Each piece keeps its text byte for byte, so a file is written back unchanged except where a
//! value was set: comments, the order, spaces, keys this program does not know and lines it cannot
//! read all stay. Setting a value replaces only the value's text on its line, or adds a line in the
//! right table, or a new table at the end.
//!
//! The subset read: `[table]` headers with bare or quoted key parts, `key = value` lines with dotted
//! keys, `#` comments. `[[array]]` tables are kept but their keys are not read. A line that is not
//! TOML is kept and reported.

use crate::value::{self, Value};
use std::ops::Range;

/// One piece of the file.
#[derive(Clone, Debug, PartialEq)]
struct Piece {
    /// The text with its line ending(s).
    text: String,
    /// The first line's number, from 1.
    line: usize,
    kind: Kind,
}

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    /// A blank line, a comment, or a line kept as it is.
    Other,
    /// `[a.b]`.
    Table(Vec<String>),
    /// `[[a.b]]`: kept; keys under it are not read.
    ArrayTable,
    /// `key = value`: the full path (table and key), the value when it is one of this file's kinds,
    /// and where the value's text is in [`Piece::text`].
    Entry {
        path: Vec<String>,
        value: Option<Value>,
        span: Range<usize>,
    },
}

/// Something in the file that was not read, with its line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub line: usize,
    pub text: String,
}

impl std::fmt::Display for Note {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.text)
    }
}

/// A TOML file kept as written.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Doc {
    pieces: Vec<Piece>,
}

/// How two path parts are compared: exactly, or without case (the game's names).
pub type PartEq = fn(usize, &str, &str) -> bool;

/// Exact comparison of every part.
pub fn exact(_: usize, a: &str, b: &str) -> bool {
    a == b
}

impl Doc {
    /// Reads `text`. Never fails: what cannot be read is kept and named in the notes.
    pub fn parse(text: &str) -> (Doc, Vec<Note>) {
        let mut doc = Doc::default();
        let mut notes = Vec::new();
        let mut table: Option<Vec<String>> = Some(Vec::new());
        let mut rest = text;
        let mut line = 1;
        while !rest.is_empty() {
            let (kind, len) = match read_piece(rest, &table) {
                Ok((kind, len)) => (kind, len),
                Err(why) => {
                    notes.push(Note {
                        line,
                        text: format!("not read, kept as it is: {why}"),
                    });
                    (Kind::Other, 0)
                }
            };
            // A piece always ends at a line's end.
            let len = rest[len..].find('\n').map_or(rest.len(), |i| len + i + 1);
            match &kind {
                Kind::Table(path) => table = Some(path.clone()),
                Kind::ArrayTable => table = None,
                _ => {}
            }
            let piece_text = &rest[..len];
            doc.pieces.push(Piece {
                text: piece_text.to_string(),
                line,
                kind,
            });
            line += piece_text.matches('\n').count();
            rest = &rest[len..];
        }
        let mut seen: Vec<&Vec<String>> = Vec::new();
        for p in &doc.pieces {
            if let Kind::Entry { path, .. } = &p.kind {
                if seen.contains(&path) {
                    notes.push(Note {
                        line: p.line,
                        text: format!("`{}` is set twice; the first one counts", dotted(path)),
                    });
                } else {
                    seen.push(path);
                }
            }
        }
        (doc, notes)
    }

    /// The file's text: what was read, with the changes made since.
    pub fn to_text(&self) -> String {
        self.pieces.iter().map(|p| p.text.as_str()).collect()
    }

    /// Every key with its full path, its value (`None`: not one of this file's kinds) and its line,
    /// in the file's order. A key set twice is listed once, at its first line.
    pub fn entries(&self) -> Vec<(&[String], Option<&Value>, usize)> {
        let mut out: Vec<(&[String], Option<&Value>, usize)> = Vec::new();
        for p in &self.pieces {
            if let Kind::Entry { path, value, .. } = &p.kind {
                if !out.iter().any(|(q, ..)| *q == path.as_slice()) {
                    out.push((path, value.as_ref(), p.line));
                }
            }
        }
        out
    }

    /// The value at `path` (the first one, when set twice).
    pub fn get(&self, path: &[&str], eq: PartEq) -> Option<&Value> {
        self.pieces.iter().find_map(|p| match &p.kind {
            Kind::Entry { path: q, value, .. } if same(q, path, eq) => Some(value.as_ref()),
            _ => None,
        })?
    }

    /// Sets the value at `path`, whose last part is the key and the rest its table. An existing key
    /// keeps its line, spaces and comment, only its value changes; a new one goes after the last key
    /// of its table, or into a new table at the end of the file.
    pub fn set(&mut self, path: &[&str], v: &Value, eq: PartEq) {
        assert!(!path.is_empty(), "a key has a name");
        let new = v.to_toml();
        if let Some(p) = self
            .pieces
            .iter_mut()
            .find(|p| matches!(&p.kind, Kind::Entry { path: q, .. } if same(q, path, eq)))
        {
            if let Kind::Entry { value, span, .. } = &mut p.kind {
                p.text.replace_range(span.clone(), &new);
                *span = span.start..span.start + new.len();
                *value = Some(v.clone());
            }
            return;
        }
        let newline = self.newline();
        let (table, key) = path.split_at(path.len() - 1);
        let key_text = key_part(key[0]);
        // The table's header, then the last key under it.
        let header = self
            .pieces
            .iter()
            .position(|p| matches!(&p.kind, Kind::Table(t) if same(t, table, eq)));
        let at = match header {
            Some(h) => {
                let mut last = h;
                for (i, p) in self.pieces.iter().enumerate().skip(h + 1) {
                    match &p.kind {
                        Kind::Table(_) | Kind::ArrayTable => break,
                        Kind::Entry { .. } => last = i,
                        Kind::Other => {}
                    }
                }
                Some(last + 1)
            }
            None if table.is_empty() => {
                // A top-level key: after the last top-level key; without one, before the first table
                // (with a blank line between), or at the end of a file without tables.
                let first_table = self
                    .pieces
                    .iter()
                    .position(|p| matches!(p.kind, Kind::Table(_) | Kind::ArrayTable));
                let end = first_table.unwrap_or(self.pieces.len());
                match self.pieces[..end]
                    .iter()
                    .rposition(|p| matches!(p.kind, Kind::Entry { .. }))
                {
                    Some(i) => Some(i + 1),
                    None => {
                        if let Some(t) = first_table {
                            self.pieces.insert(
                                t,
                                Piece {
                                    text: newline.to_string(),
                                    line: 0,
                                    kind: Kind::Other,
                                },
                            );
                        }
                        Some(end)
                    }
                }
            }
            None => None,
        };
        let entry_text = format!("{key_text} = {new}{newline}");
        let start = key_text.len() + 3;
        let entry = |line| Piece {
            kind: Kind::Entry {
                path: path.iter().map(|s| s.to_string()).collect(),
                value: Some(v.clone()),
                span: start..start + new.len(),
            },
            text: entry_text.clone(),
            line,
        };
        match at {
            Some(i) => {
                self.end_last_line_before(i, newline);
                self.pieces.insert(i, entry(0));
            }
            None => {
                let end = self.pieces.len();
                self.end_last_line_before(end, newline);
                if !self.pieces.is_empty() {
                    self.pieces.push(Piece {
                        text: newline.to_string(),
                        line: 0,
                        kind: Kind::Other,
                    });
                }
                let header_text = format!(
                    "[{}]{newline}",
                    table
                        .iter()
                        .map(|s| key_part(s))
                        .collect::<Vec<_>>()
                        .join(".")
                );
                self.pieces.push(Piece {
                    text: header_text,
                    line: 0,
                    kind: Kind::Table(table.iter().map(|s| s.to_string()).collect()),
                });
                self.pieces.push(entry(0));
            }
        }
    }

    /// Takes the key at `path` out (every line that sets it). Whether there was one.
    pub fn remove(&mut self, path: &[&str], eq: PartEq) -> bool {
        let before = self.pieces.len();
        self.pieces
            .retain(|p| !matches!(&p.kind, Kind::Entry { path: q, .. } if same(q, path, eq)));
        before != self.pieces.len()
    }

    /// The file's line ending: CRLF when its first line has one.
    fn newline(&self) -> &'static str {
        match self.pieces.first() {
            Some(p) if p.text.ends_with("\r\n") => "\r\n",
            _ => "\n",
        }
    }

    /// Makes sure the piece before `i` ends with a line ending, so a piece put at `i` starts a line.
    fn end_last_line_before(&mut self, i: usize, newline: &str) {
        if i > 0 {
            let p = &mut self.pieces[i - 1];
            if !p.text.ends_with('\n') {
                p.text.push_str(newline);
            }
        }
    }
}

fn same(stored: &[String], asked: &[&str], eq: PartEq) -> bool {
    stored.len() == asked.len()
        && stored
            .iter()
            .zip(asked)
            .enumerate()
            .all(|(i, (a, b))| eq(i, a, b))
}

/// A path as `a.b."c d"`, for messages.
pub fn dotted(path: &[String]) -> String {
    path.iter()
        .map(|s| key_part(s))
        .collect::<Vec<_>>()
        .join(".")
}

/// A key part as TOML text: bare when it can be, else a basic string.
pub fn key_part(part: &str) -> String {
    if !part.is_empty()
        && part
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        part.to_string()
    } else {
        value::quote(part)
    }
}

/// Reads one piece at the start of `text`: its kind and the length of what it read (the caller
/// extends it to the line's end). `table` is the current table (`None` under an array table).
fn read_piece(text: &str, table: &Option<Vec<String>>) -> Result<(Kind, usize), String> {
    let line_end = text.find('\n').unwrap_or(text.len());
    let line = &text[..line_end];
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    if trimmed.trim_end().is_empty() || trimmed.starts_with('#') {
        return Ok((Kind::Other, 0));
    }
    if trimmed.starts_with("[[") {
        let close = trimmed.find("]]").ok_or("a [[table]] header without ]]")?;
        key_path(&trimmed[2..close])?;
        end_of_line(&trimmed[close + 2..])?;
        return Ok((Kind::ArrayTable, 0));
    }
    if let Some(inner) = trimmed.strip_prefix('[') {
        let close = find_close(inner)?;
        let path = key_path(&inner[..close])?;
        end_of_line(&inner[close + 1..])?;
        return Ok((Kind::Table(path), 0));
    }
    let eq = find_equals(trimmed)?;
    let key = key_path(&trimmed[..eq])?;
    let after = &text[indent + eq + 1..];
    let value_start = indent + eq + 1 + (after.len() - after.trim_start_matches([' ', '\t']).len());
    let (value, len) = value::parse(&text[value_start..])?;
    let span = value_start..value_start + len;
    end_of_line(text[span.end..].split('\n').next().unwrap_or(""))?;
    let Some(table) = table else {
        return Ok((Kind::Other, span.end));
    };
    let mut path = table.clone();
    path.extend(key);
    Ok((
        Kind::Entry {
            path,
            value,
            span: span.clone(),
        },
        span.end,
    ))
}

/// What may follow a header or a value on its line: spaces and a comment.
fn end_of_line(rest: &str) -> Result<(), String> {
    let rest = rest.trim_start_matches([' ', '\t']);
    if rest.is_empty() || rest == "\r" || rest.starts_with('#') {
        Ok(())
    } else {
        Err(format!("unexpected `{}` after the value", rest.trim_end()))
    }
}

/// The `]` that closes a header, outside quotes.
fn find_close(inner: &str) -> Result<usize, String> {
    let mut quote = None;
    for (i, c) in inner.char_indices() {
        match (quote, c) {
            (None, ']') => return Ok(i),
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            _ => {}
        }
    }
    Err("a [table] header without ]".to_string())
}

/// The `=` of a key line, outside quotes.
fn find_equals(line: &str) -> Result<usize, String> {
    let mut quote = None;
    for (i, c) in line.char_indices() {
        match (quote, c) {
            (None, '=') => return Ok(i),
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            _ => {}
        }
    }
    Err("neither a table header, a key = value line nor a comment".to_string())
}

/// A dotted key: `a.b`, `"c d".e`, `'f'`.
fn key_path(text: &str) -> Result<Vec<String>, String> {
    let mut parts = Vec::new();
    let mut rest = text.trim();
    loop {
        if rest.is_empty() {
            return Err("an empty key".to_string());
        }
        let (part, len) = if rest.starts_with('"') || rest.starts_with('\'') {
            match value::parse(rest)? {
                (Some(Value::Text(s)), len) => (s, len),
                _ => return Err(format!("`{rest}` is not a key")),
            }
        } else {
            let len = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
                .unwrap_or(rest.len());
            if len == 0 {
                return Err(format!("`{rest}` is not a key"));
            }
            (rest[..len].to_string(), len)
        };
        parts.push(part);
        rest = rest[len..].trim_start();
        if rest.is_empty() {
            return Ok(parts);
        }
        rest = rest
            .strip_prefix('.')
            .ok_or_else(|| format!("`{rest}` in a key"))?
            .trim_start();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# My settings
version = 1 # the format
preset = \"modern\"

[global.ao]
quality = \"high\"   # nice
strength = 0.8
future_key = [1, 2,
  3]

[worldspace.\"Wasteland NV\".shadows]
enabled = true
notes = '''
two
lines'''

[[shader]]
name = \"x\"

[other]
a.b = 1
when = 1979-05-27
";

    fn path(p: &[String]) -> Vec<&str> {
        p.iter().map(String::as_str).collect()
    }

    #[test]
    fn an_unchanged_file_is_written_back_byte_for_byte() {
        let (doc, notes) = Doc::parse(SAMPLE);
        assert_eq!(notes, vec![]);
        assert_eq!(doc.to_text(), SAMPLE);
        let crlf = SAMPLE.replace('\n', "\r\n");
        assert_eq!(Doc::parse(&crlf).0.to_text(), crlf);
        let no_end = "a = 1\n[t]\nb = 2";
        assert_eq!(Doc::parse(no_end).0.to_text(), no_end);
    }

    #[test]
    fn finds_keys_by_their_full_path() {
        let (doc, _) = Doc::parse(SAMPLE);
        let keys: Vec<(Vec<&str>, usize)> = doc
            .entries()
            .iter()
            .map(|(p, _, l)| (path(p), *l))
            .collect();
        assert_eq!(
            keys,
            vec![
                (vec!["version"], 2),
                (vec!["preset"], 3),
                (vec!["global", "ao", "quality"], 6),
                (vec!["global", "ao", "strength"], 7),
                (vec!["global", "ao", "future_key"], 8),
                (vec!["worldspace", "Wasteland NV", "shadows", "enabled"], 12),
                (vec!["worldspace", "Wasteland NV", "shadows", "notes"], 13),
                (vec!["other", "a", "b"], 21),
                (vec!["other", "when"], 22),
            ]
        );
        assert_eq!(
            doc.get(&["global", "ao", "strength"], exact),
            Some(&Value::Number(0.8))
        );
        assert_eq!(doc.get(&["global", "ao", "future_key"], exact), None);
        assert_eq!(
            doc.get(&["worldspace", "Wasteland NV", "shadows", "notes"], exact),
            Some(&Value::Text("two\nlines".into()))
        );
        // Keys under [[shader]] are not read.
        assert_eq!(doc.get(&["shader", "name"], exact), None);
    }

    #[test]
    fn setting_a_key_changes_only_its_value() {
        let (mut doc, _) = Doc::parse(SAMPLE);
        doc.set(
            &["global", "ao", "quality"],
            &Value::Text("low".into()),
            exact,
        );
        doc.set(&["preset"], &Value::Text("original".into()), exact);
        let want = SAMPLE
            .replace("quality = \"high\"   # nice", "quality = \"low\"   # nice")
            .replace("preset = \"modern\"", "preset = \"original\"");
        assert_eq!(doc.to_text(), want);
        // Set again: the span moved with the shorter text.
        doc.set(
            &["global", "ao", "quality"],
            &Value::Text("medium".into()),
            exact,
        );
        assert!(doc.to_text().contains("quality = \"medium\"   # nice\n"));
    }

    #[test]
    fn a_new_key_goes_into_its_table_or_a_new_one() {
        let (mut doc, _) = Doc::parse(SAMPLE);
        doc.set(&["global", "ao", "radius"], &Value::Number(2.0), exact);
        doc.set(
            &["cell", "GSDocMitchellHouse", "ao", "enabled"],
            &Value::Bool(false),
            exact,
        );
        doc.set(&["top"], &Value::Bool(true), exact);
        let text = doc.to_text();
        assert!(
            text.contains("future_key = [1, 2,\n  3]\nradius = 2\n\n[worldspace"),
            "{text}"
        );
        assert!(
            text.ends_with("when = 1979-05-27\n\n[cell.GSDocMitchellHouse.ao]\nenabled = false\n"),
            "{text}"
        );
        assert!(text.contains("preset = \"modern\"\ntop = true\n"), "{text}");
        // What was written reads back the same.
        let (again, notes) = Doc::parse(&text);
        assert_eq!(notes, vec![]);
        assert_eq!(
            again.get(&["global", "ao", "radius"], exact),
            Some(&Value::Number(2.0))
        );
        assert_eq!(
            again.get(&["cell", "GSDocMitchellHouse", "ao", "enabled"], exact),
            Some(&Value::Bool(false))
        );
        assert_eq!(again.to_text(), text);
    }

    #[test]
    fn an_empty_file_gets_a_table() {
        let mut doc = Doc::default();
        doc.set(&["version"], &Value::Number(1.0), exact);
        doc.set(
            &["global", "ao", "quality"],
            &Value::Text("high".into()),
            exact,
        );
        doc.set(
            &["worldspace", "The Strip", "ao", "quality"],
            &Value::Text("low".into()),
            exact,
        );
        assert_eq!(
            doc.to_text(),
            "version = 1\n\n[global.ao]\nquality = \"high\"\n\n[worldspace.\"The Strip\".ao]\nquality = \"low\"\n"
        );
        let crlf = Doc::parse("a = 1\r\n").0;
        let mut crlf = crlf;
        crlf.set(&["t", "b"], &Value::Bool(true), exact);
        assert_eq!(crlf.to_text(), "a = 1\r\n\r\n[t]\r\nb = true\r\n");
    }

    #[test]
    fn a_file_without_a_last_line_ending_gets_one_before_a_new_line() {
        let (mut doc, _) = Doc::parse("[t]\na = 1");
        doc.set(&["t", "b"], &Value::Number(2.0), exact);
        assert_eq!(doc.to_text(), "[t]\na = 1\nb = 2\n");
    }

    #[test]
    fn lines_that_are_not_toml_are_kept_and_named() {
        let text = "a = 1\nthis is not toml\nb = \"open\n[bad\nc = 2 junk\nd = 4\na = 5\n";
        let (doc, notes) = Doc::parse(text);
        assert_eq!(doc.to_text(), text);
        let lines: Vec<usize> = notes.iter().map(|n| n.line).collect();
        assert_eq!(lines, vec![2, 3, 4, 5, 7], "{notes:?}");
        assert!(notes[4].text.contains("set twice"));
        assert_eq!(doc.get(&["d"], exact), Some(&Value::Number(4.0)));
        assert_eq!(doc.get(&["a"], exact), Some(&Value::Number(1.0)));
    }

    #[test]
    fn removing_a_key_takes_its_line_out() {
        let (mut doc, _) = Doc::parse("[t]\na = 1\nb = 2\n");
        assert!(doc.remove(&["t", "a"], exact));
        assert!(!doc.remove(&["t", "a"], exact));
        assert_eq!(doc.to_text(), "[t]\nb = 2\n");
    }

    #[test]
    fn parts_can_be_compared_without_case() {
        fn place(i: usize, a: &str, b: &str) -> bool {
            if i == 1 {
                a.eq_ignore_ascii_case(b)
            } else {
                a == b
            }
        }
        let (mut doc, _) = Doc::parse("[worldspace.WastelandNV.ao]\nquality = \"high\"\n");
        assert_eq!(
            doc.get(&["worldspace", "wastelandnv", "ao", "quality"], place),
            Some(&Value::Text("high".into()))
        );
        doc.set(
            &["worldspace", "WASTELANDNV", "ao", "strength"],
            &Value::Number(0.5),
            place,
        );
        assert_eq!(
            doc.to_text(),
            "[worldspace.WastelandNV.ao]\nquality = \"high\"\nstrength = 0.5\n"
        );
    }
}
