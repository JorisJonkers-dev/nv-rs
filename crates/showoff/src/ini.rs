// SPDX-License-Identifier: GPL-3.0-only
//! ShowOff xNVSE's cached INI commands (group 4 of the profile's missing commands: 882 calls in
//! 10 plugins, 742 of them `GetINIFloatOrDefault_Cached` and its short name). Written from the
//! commands' GECK Wiki pages (see `NOTICE`, `provenance/02-cached-ini.md`); no plugin code was
//! read.
//!
//! Files are read from and written to `Data\Config\` (the page: "as in MCM's INI functions"),
//! through the game's own file calls, so the engine's file layer applies. A file is read once
//! into a cache that lives in the plugin's state until `ClearFileCacheShowOff` drops it; the
//! `Create` and `Set` commands change the cache only, and `SaveCachedIniFile` writes it back,
//! recreating the file.
//!
//! The cache keeps a file's lines, so what was not touched is written back as it was read
//! (comments, blank lines, the order of sections and keys). Section and key names match without
//! case. What the pages leave open is in `provenance/02-cached-ini.md`.

use crate::commands::Cmd;
use crate::identity::OPCODE_BASE;
use crate::State;
use engine_core::prelude::*;
use nvse::commands::string::assign_to_string_var;
use nvse::expr::{self, Eval, Tok, Ty};
use nvse::table::{Param, Returns};
use nvse::{handled, CommandArgs};

const NV_NUMBER: u32 = 0x8001;
const NV_STRING: u32 = 0x8004;
const NV_STRING_OR_NUMBER: u32 = 0x8005;

const fn p(name: &'static str, kind: u32, optional: bool) -> Param {
    Param {
        name,
        kind,
        optional,
    }
}

const GET_PARAMS: &[Param] = &[
    p("sectionAndKey", NV_STRING, false),
    p("iniPath", NV_STRING, true),
    p("defaultValue", NV_NUMBER, true),
];
const CREATE_PARAMS: &[Param] = &[
    p("sectionAndKey", NV_STRING, false),
    p("iniPath", NV_STRING, true),
    p("createValue", NV_NUMBER, true),
    p("comment", NV_STRING, true),
];
const STRING_PARAMS: &[Param] = &[
    p("sectionAndKey", NV_STRING, false),
    p("iniPath", NV_STRING, true),
    p("defaultValue", NV_STRING, true),
];
const CLEAR_PARAMS: &[Param] = &[
    p("filePath", NV_STRING, false),
    p("toClearType", NV_NUMBER, false),
];
const SET_PARAMS: &[Param] = &[
    p("sectionAndKey", NV_STRING, false),
    p("newValue", NV_STRING_OR_NUMBER, false),
    p("iniPath", NV_STRING, true),
    p("comment", NV_STRING, true),
];
const SAVE_PARAMS: &[Param] = &[p("iniPath", NV_STRING, true)];

const fn cmd(
    offset: u32,
    name: &'static str,
    short: &'static str,
    params: &'static [Param],
    execute: AbiFn,
    returns: Returns,
) -> Cmd {
    Cmd {
        opcode: OPCODE_BASE + offset,
        name,
        short,
        params,
        execute,
        eval: None,
        needs_ref: false,
        expression: true,
        returns,
    }
}

/// The commands, in the order of their opcodes.
pub static COMMANDS: [Cmd; 6] = [
    cmd(
        0x93,
        "SetINIValue_Cached",
        "SetINIValC",
        SET_PARAMS,
        set_ini_value,
        Returns::Default,
    ),
    cmd(
        0x94,
        "GetINIFloatOrCreate_Cached",
        "GetINIFltOrCreateC",
        CREATE_PARAMS,
        get_float_or_create,
        Returns::Default,
    ),
    cmd(
        0x96,
        "GetINIFloatOrDefault_Cached",
        "GetINIFloat_Cached",
        GET_PARAMS,
        get_float_or_default,
        Returns::Default,
    ),
    cmd(
        0x97,
        "GetINIStringOrDefault_Cached",
        "GetINIString_Cached",
        STRING_PARAMS,
        get_string_or_default,
        Returns::String,
    ),
    cmd(
        0x99,
        "ClearFileCacheShowOff",
        "CloseFileSO",
        CLEAR_PARAMS,
        clear_file_cache,
        Returns::Default,
    ),
    cmd(
        0x9a,
        "SaveCachedIniFile",
        "SaveCachedIni",
        SAVE_PARAMS,
        save_cached_ini,
        Returns::Default,
    ),
];

// ---- the cache ----

/// One line of a cached file.
#[derive(Clone, Debug, PartialEq)]
enum Line {
    /// A comment, a blank line, or anything that is neither a section nor a `key=value` line.
    Other(Vec<u8>),
    Section(Vec<u8>),
    Entry {
        key: Vec<u8>,
        value: Vec<u8>,
    },
}

/// A cached INI file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IniFile {
    lines: Vec<Line>,
}

fn trim(mut s: &[u8]) -> &[u8] {
    while let [b' ' | b'\t', rest @ ..] = s {
        s = rest;
    }
    while let [rest @ .., b' ' | b'\t'] = s {
        s = rest;
    }
    s
}

fn same(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

impl IniFile {
    pub fn parse(text: &[u8]) -> IniFile {
        let mut lines = Vec::new();
        let text = text.strip_prefix(b"\xef\xbb\xbf").unwrap_or(text);
        for raw in text.split(|&b| b == b'\n') {
            let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
            let line = trim(raw);
            if line.first() == Some(&b'[') {
                if let Some(end) = line.iter().position(|&b| b == b']') {
                    lines.push(Line::Section(trim(&line[1..end]).to_vec()));
                    continue;
                }
            }
            match line.iter().position(|&b| b == b'=') {
                Some(eq) if !matches!(line.first(), Some(b';' | b'#')) && eq > 0 => {
                    lines.push(Line::Entry {
                        key: trim(&line[..eq]).to_vec(),
                        value: trim(&line[eq + 1..]).to_vec(),
                    });
                }
                _ => lines.push(Line::Other(raw.to_vec())),
            }
        }
        // The split leaves one empty piece after a final newline.
        if lines.last() == Some(&Line::Other(Vec::new())) {
            lines.pop();
        }
        IniFile { lines }
    }

    /// The file's text, lines ended by `\r\n`.
    pub fn text(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for line in &self.lines {
            match line {
                Line::Other(t) => out.extend_from_slice(t),
                Line::Section(s) => {
                    out.push(b'[');
                    out.extend_from_slice(s);
                    out.push(b']');
                }
                Line::Entry { key, value } => {
                    out.extend_from_slice(key);
                    out.extend_from_slice(b" = ");
                    out.extend_from_slice(value);
                }
            }
            out.extend_from_slice(b"\r\n");
        }
        out
    }

    /// The index of the entry `key` of section `section`: the first of them.
    fn find(&self, section: &[u8], key: &[u8]) -> Option<usize> {
        let mut inside = false;
        for (i, line) in self.lines.iter().enumerate() {
            match line {
                Line::Section(s) => inside = same(s, section),
                Line::Entry { key: k, .. } if inside && same(k, key) => return Some(i),
                _ => {}
            }
        }
        None
    }

    pub fn get(&self, section: &[u8], key: &[u8]) -> Option<&[u8]> {
        match self.lines.get(self.find(section, key)?)? {
            Line::Entry { value, .. } => Some(value),
            _ => None,
        }
    }

    /// Sets the value: true when the key was there (updated), false when it was inserted at the
    /// end of its section (the section made at the end of the file when it is not there), with
    /// `comment` on its own line above it.
    pub fn set(&mut self, section: &[u8], key: &[u8], value: &[u8], comment: &[u8]) -> bool {
        if let Some(i) = self.find(section, key) {
            // The key keeps the spelling it was written with.
            if let Line::Entry { value: v, .. } = &mut self.lines[i] {
                *v = value.to_vec();
            }
            return true;
        }
        let at = self.section_end(section).unwrap_or_else(|| {
            if self
                .lines
                .last()
                .is_some_and(|l| *l != Line::Other(Vec::new()))
            {
                self.lines.push(Line::Other(Vec::new()));
            }
            self.lines.push(Line::Section(section.to_vec()));
            self.lines.len()
        });
        let mut add = Vec::new();
        if !comment.is_empty() {
            let mut text = b"; ".to_vec();
            text.extend_from_slice(comment);
            add.push(Line::Other(text));
        }
        add.push(Line::Entry {
            key: key.to_vec(),
            value: value.to_vec(),
        });
        self.lines.splice(at..at, add);
        false
    }

    /// Where a new entry of the section goes: after its last entry (or its header).
    fn section_end(&self, section: &[u8]) -> Option<usize> {
        let mut inside = false;
        let mut end = None;
        for (i, line) in self.lines.iter().enumerate() {
            match line {
                Line::Section(s) => {
                    inside = same(s, section);
                    if inside {
                        end = Some(i + 1);
                    }
                }
                Line::Entry { .. } if inside => end = Some(i + 1),
                _ => {}
            }
        }
        end
    }
}

// ---- names and files ----

/// `Section:Key`, with `\` and `/` also the delimiter: the first delimiter splits. None for a
/// name without one, or with an empty section or key.
fn split_name(name: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let at = name.iter().position(|b| matches!(b, b':' | b'\\' | b'/'))?;
    let (section, key) = (trim(&name[..at]), trim(&name[at + 1..]));
    (!section.is_empty() && !key.is_empty()).then(|| (section.to_vec(), key.to_vec()))
}

/// The path under `Data\Config\` for the file name a script gave: `/` as `\`, `.ini` added when
/// the name has no extension of it; None for an empty name after the default, an absolute path
/// or one that leaves the folder (`..`, a drive).
fn config_path(given: &[u8], mod_name: &[u8]) -> Option<Vec<u8>> {
    let mut name: Vec<u8> = if given.is_empty() {
        let stem = mod_name
            .iter()
            .rposition(|&b| b == b'.')
            .map_or(mod_name, |d| &mod_name[..d]);
        stem.to_vec()
    } else {
        given
            .iter()
            .map(|&b| if b == b'/' { b'\\' } else { b })
            .collect()
    };
    if name.is_empty()
        || name[0] == b'\\'
        || name.contains(&b':')
        || name
            .split(|&b| b == b'\\')
            .any(|part| part == b".." || part.is_empty())
    {
        return None;
    }
    let ends_ini = name.len() >= 4 && same(&name[name.len() - 4..], b".ini");
    if !ends_ini {
        name.extend_from_slice(b".ini");
    }
    Some(name)
}

const CONFIG: &[u8] = b"Data\\Config\\";
const CREATE_FILE_A: u32 = 0x00fd_f088;
const WRITE_FILE: u32 = 0x00fd_f0dc;
const CLOSE_HANDLE: u32 = 0x00fd_f07c;
const CREATE_DIRECTORY: u32 = 0x00fd_f0b8;
const GENERIC_WRITE: u32 = 0x4000_0000;
const CREATE_ALWAYS: u32 = 2;
const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
const INVALID_HANDLE_VALUE: u32 = 0xffff_ffff;

fn full_path(relative: &[u8]) -> Vec<u8> {
    let mut p = CONFIG.to_vec();
    p.extend_from_slice(relative);
    p
}

/// The file's text from the game's file calls; an empty cache entry for a file that is not
/// there (or is empty).
fn load(e: &mut Engine, relative: &[u8]) -> IniFile {
    nvse::saveload::read_file(e, &full_path(relative))
        .map(|t| IniFile::parse(&t))
        .unwrap_or_default()
}

/// The file written anew, its folders made first.
fn write(e: &mut Engine, relative: &[u8], text: &[u8]) -> bool {
    let full = full_path(relative);
    let mut at = CONFIG.len() - 1;
    // `Data\Config`, then each folder of the path.
    let mut folders: Vec<usize> = vec![at];
    for (i, &b) in relative.iter().enumerate() {
        if b == b'\\' {
            at = CONFIG.len() + i;
            folders.push(at);
        }
    }
    for end in folders {
        nvse::events::with_guest_text(e, &full[..end], |e, p| {
            e.call(CREATE_DIRECTORY, &args![p, 0u32]);
        });
    }
    let handle = nvse::events::with_guest_text(e, &full, |e, p| {
        e.call(
            CREATE_FILE_A,
            &args![
                p,
                GENERIC_WRITE,
                0u32,
                0u32,
                CREATE_ALWAYS,
                FILE_ATTRIBUTE_NORMAL,
                0u32
            ],
        )
        .u32()
    });
    if handle == INVALID_HANDLE_VALUE {
        return false;
    }
    e.with_stack(0x1004, |e, frame| {
        let (buf, written) = (frame.addr(), frame.addr() + 0x1000);
        for piece in text.chunks(0x1000) {
            e.mem.write(buf, piece);
            e.call(
                WRITE_FILE,
                &args![handle, buf, piece.len() as u32, written, 0u32],
            );
        }
    });
    e.call(CLOSE_HANDLE, &args![handle]);
    true
}

/// The cache key of a path: without case.
fn cache_key(relative: &[u8]) -> Vec<u8> {
    relative.to_ascii_lowercase()
}

/// The cached file for `relative`, read when it is not cached yet.
fn cached<'a>(e: &'a mut Engine, relative: &[u8]) -> &'a mut IniFile {
    let key = cache_key(relative);
    if !e.ext::<State>().ini.contains_key(&key) {
        let file = load(e, relative);
        e.ext::<State>().ini.insert(key.clone(), file);
    }
    e.ext::<State>().ini.get_mut(&key).expect("just cached")
}

// ---- arguments ----

/// What a command's arguments are, read: the strings and numbers by position, `None` for an
/// argument not given.
enum Arg {
    Text(Vec<u8>),
    Number(f64),
}

fn read_args(e: &mut Engine, ev: &Eval, toks: &[Tok]) -> Vec<Option<Arg>> {
    toks.iter()
        .map(|t| {
            if matches!(t, Tok::OptionalEmpty) {
                None
            } else if ev.can_convert(e, t, Ty::Number) {
                Some(Arg::Number(ev.get_number(e, t)))
            } else if ev.can_convert(e, t, Ty::String) {
                Some(Arg::Text(ev.get_string(e, t)))
            } else {
                None
            }
        })
        .collect()
}

fn text_at(args: &[Option<Arg>], i: usize) -> Option<Vec<u8>> {
    match args.get(i) {
        Some(Some(Arg::Text(t))) => Some(t.clone()),
        _ => None,
    }
}

fn number_at(args: &[Option<Arg>], i: usize) -> Option<f64> {
    match args.get(i) {
        Some(Some(Arg::Number(n))) => Some(*n),
        _ => None,
    }
}

/// Runs `f` with the arguments; the result starts at 0.
fn run(
    e: &mut Engine,
    w: &[u32],
    f: impl FnOnce(&mut Engine, &CommandArgs, Vec<Option<Arg>>),
) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let mut ev = Eval::new(e, &a);
    expr::with_evaluator(e, |e| {
        if let Some(toks) = ev.extract_args(e) {
            let args = read_args(e, &ev, &toks);
            f(e, &a, args);
        }
    });
    expr::report(e, &ev);
    handled(true)
}

/// The path and name a command works on: the file's path under `Data\Config\` (the mod's own name
/// for none given) and the section and key.
fn target(
    e: &Engine,
    a: &CommandArgs,
    name: &[u8],
    path: Option<Vec<u8>>,
) -> Option<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    let (section, key) = split_name(name)?;
    let relative = config_path(
        &path.unwrap_or_default(),
        &nvse::errors::mod_name(e, a.script),
    )?;
    Some((relative, section, key))
}

/// The number a text reads as (`atof`: its leading number, 0 when it has none).
fn number_of(text: &[u8]) -> f64 {
    let s = String::from_utf8_lossy(trim(text));
    let mut end = 0;
    let (mut dot, mut exp) = (false, false);
    let b = s.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        let ok = match c {
            b'0'..=b'9' => true,
            b'+' | b'-' => i == 0 || matches!(b[i - 1], b'e' | b'E'),
            b'.' if !dot && !exp => {
                dot = true;
                true
            }
            b'e' | b'E' if !exp && i > 0 => {
                exp = true;
                true
            }
            _ => false,
        };
        if !ok {
            break;
        }
        end = i + 1;
    }
    // Back off to the longest prefix that parses.
    while end > 0 {
        if let Ok(v) = s[..end].parse::<f64>() {
            return v;
        }
        end -= 1;
    }
    0.0
}

fn number_text(v: f64) -> Vec<u8> {
    format!("{v}").into_bytes()
}

fn get_float_or_default(e: &mut Engine, w: &[u32]) -> Ret {
    run(e, w, |e, a, args| {
        let default = number_at(&args, 2).unwrap_or(0.0);
        a.set_result(e, default);
        let Some(name) = text_at(&args, 0) else {
            return;
        };
        let Some((rel, section, key)) = target(e, a, &name, text_at(&args, 1)) else {
            return;
        };
        let value = cached(e, &rel).get(&section, &key).map(<[u8]>::to_vec);
        if let Some(v) = value {
            a.set_result(e, number_of(&v));
        }
    })
}

fn get_float_or_create(e: &mut Engine, w: &[u32]) -> Ret {
    run(e, w, |e, a, args| {
        let create = number_at(&args, 2).unwrap_or(0.0);
        a.set_result(e, create);
        let Some(name) = text_at(&args, 0) else {
            return;
        };
        let Some((rel, section, key)) = target(e, a, &name, text_at(&args, 1)) else {
            return;
        };
        let comment = text_at(&args, 3).unwrap_or_default();
        let file = cached(e, &rel);
        match file.get(&section, &key).map(<[u8]>::to_vec) {
            Some(v) => a.set_result(e, number_of(&v)),
            None => {
                file.set(&section, &key, &number_text(create), &comment);
            }
        }
    })
}

fn get_string_or_default(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    let mut answer = Vec::new();
    let mut ev = Eval::new(e, &a);
    expr::with_evaluator(e, |e| {
        if let Some(toks) = ev.extract_args(e) {
            let args = read_args(e, &ev, &toks);
            answer = text_at(&args, 2).unwrap_or_default();
            if let Some(name) = text_at(&args, 0) {
                if let Some((rel, section, key)) = target(e, &a, &name, text_at(&args, 1)) {
                    if let Some(v) = cached(e, &rel).get(&section, &key) {
                        answer = v.to_vec();
                    }
                }
            }
        }
    });
    expr::report(e, &ev);
    a.set_result(e, 0.0);
    assign_to_string_var(e, &a, &answer);
    handled(true)
}

fn set_ini_value(e: &mut Engine, w: &[u32]) -> Ret {
    run(e, w, |e, a, args| {
        a.set_result(e, -1.0);
        let Some(name) = text_at(&args, 0) else {
            return;
        };
        let value = match args.get(1) {
            Some(Some(Arg::Text(t))) => t.clone(),
            Some(Some(Arg::Number(n))) => number_text(*n),
            _ => return,
        };
        let Some((rel, section, key)) = target(e, a, &name, text_at(&args, 2)) else {
            return;
        };
        let comment = text_at(&args, 3).unwrap_or_default();
        let updated = cached(e, &rel).set(&section, &key, &value, &comment);
        a.set_result(e, if updated { 1.0 } else { 2.0 });
    })
}

fn clear_file_cache(e: &mut Engine, w: &[u32]) -> Ret {
    run(e, w, |e, a, args| {
        let (Some(path), Some(kind)) = (text_at(&args, 0), number_at(&args, 1)) else {
            return;
        };
        // Type 0 is INI; type 1 (JSON) has no cache here yet.
        if kind as i32 != 0 {
            return;
        }
        let Some(rel) = config_path(&path, &nvse::errors::mod_name(e, a.script)) else {
            return;
        };
        let gone = e.ext::<State>().ini.remove(&cache_key(&rel)).is_some();
        a.set_result(e, f64::from(u8::from(gone)));
    })
}

fn save_cached_ini(e: &mut Engine, w: &[u32]) -> Ret {
    run(e, w, |e, a, args| {
        let Some(rel) = config_path(
            &text_at(&args, 0).unwrap_or_default(),
            &nvse::errors::mod_name(e, a.script),
        ) else {
            return;
        };
        let Some(text) = e
            .ext::<State>()
            .ini
            .get(&cache_key(&rel))
            .map(IniFile::text)
        else {
            return;
        };
        let done = write(e, &rel, &text);
        a.set_result(e, f64::from(u8::from(done)));
    })
}

#[cfg(test)]
#[path = "ini_tests.rs"]
mod tests;
