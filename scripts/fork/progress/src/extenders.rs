// SPDX-License-Identifier: GPL-3.0-only
//! Script-extender commands: the reference list each extender registers (read from its own
//! source at the pinned commit, [`crate::pins`]), the commands the port builds (the real command
//! tables, as `.fork/capabilities.json` lists them; `scripts/fork/release-files` writes that file
//! and its test fails when it is stale) and whether a test of the crate that builds them runs
//! each one.
//!
//! "Tested" is read from the test code, not from a run: a test file of the crate (a
//! `*_tests.rs`, `tests.rs` or `tests/` file, or the `#[cfg(test)]` part of any other file, with
//! at least one `#[test]`) names the command, by its exact name as a word or by its opcode as a
//! hexadecimal literal. A file that names more than [`BULK`] of one extender's commands is a table
//! check (a list of every name), not a run of each, and counts for none. The tests themselves
//! must pass for a pull request to merge, which this tool does not repeat.

use crate::cpp::{self, Line};
use crate::json::Value;
use crate::pins;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// A test file naming more of one extender's commands than this is a list, not a run (the largest
/// real test files name about 60).
pub const BULK: usize = 200;

/// One command slot of an extender's reference list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefCmd {
    pub name: String,
    /// Its opcode, where the source makes it certain.
    pub opcode: Option<u32>,
}

fn cmd(name: impl Into<String>, opcode: Option<u32>) -> RefCmd {
    RefCmd {
        name: name.into(),
        opcode,
    }
}

/// xNVSE: the calls of `CommandTable::AddCommandsV*` (what `CommandTable::Init` runs; its debug
/// commands are behind `_DEBUG`), one opcode each from `0x1400`. `ADD_CMD(X)` names the record
/// `kCommandInfo_X`, whose command name is the first field of its definition (`files`: the
/// sources that define them); `ImportConsoleCommand("X")` re-exports a console command as
/// `con_X`.
pub fn xnvse(table: &str, files: &[String]) -> Vec<RefCmd> {
    let mut alias = BTreeMap::new();
    for f in files {
        alias.extend(info_names(f));
    }
    let lines = cpp::read(table, &["RUNTIME"]);
    let mut out = Vec::new();
    for body in cpp::bodies(&lines, |c| {
        c.contains("void") && c.contains("CommandTable::AddCommandsV")
    }) {
        for l in body {
            for c in cpp::calls(&l.code) {
                let name = match c.name.as_str() {
                    "ADD_CMD" | "ADD_CMD_RET" if !c.quoted => {
                        alias.get(&c.first).cloned().unwrap_or(c.first)
                    }
                    "ImportConsoleCommand" if c.quoted => format!("con_{}", c.first),
                    _ => continue,
                };
                let opcode = 0x1400 + out.len() as u32;
                out.push(cmd(name, Some(opcode)));
            }
        }
    }
    out
}

/// `kCommandInfo_X = { "Name"` (the name a `CommandInfo` record is defined with), by `X`.
fn info_names(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut rest = text;
    while let Some(k) = rest.find("kCommandInfo_") {
        rest = &rest[k + "kCommandInfo_".len()..];
        let id: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let after = rest[id.len()..].trim_start();
        let Some(after) = after.strip_prefix('=') else {
            continue;
        };
        let Some(after) = after.trim_start().strip_prefix('{') else {
            continue;
        };
        let Some(after) = after.trim_start().strip_prefix('"') else {
            continue;
        };
        if let Some(end) = after.find('"') {
            out.insert(id, after[..end].to_string());
        }
    }
    out
}

/// The `REG_*` macro calls of the lines, in order, with each line's opcode comment.
fn reg_calls(lines: &[Line]) -> Vec<(String, Option<u32>)> {
    let mut out = Vec::new();
    for l in lines {
        let t = l.code.trim_start();
        // `REG_EMPTY;` holds a slot without a command.
        if t.starts_with("REG_EMPTY") {
            out.push((String::new(), l.opcode));
            continue;
        }
        for c in cpp::calls(&l.code) {
            if c.name.starts_with("REG_") && !c.quoted && !c.first.is_empty() {
                out.push((c.first, l.opcode));
            }
        }
    }
    out
}

/// JIP LN: every `REG_*` call of `jip_nvse.cpp`, at the opcode its comment gives; an empty slot
/// (`REG_EMPTY`) is no command. `JIP_DEBUG` is off in a release.
pub fn jip(text: &str) -> Vec<RefCmd> {
    reg_calls(&cpp::read(text, &[]))
        .into_iter()
        .filter(|(n, _)| !n.is_empty())
        .map(|(n, op)| cmd(n, op))
        .collect()
}

/// JohnnyGuitar: the `REG_CMD` and `REG_TYPED_CMD` calls of `JohnnyCommands.cpp`, in order from
/// `JG_OPCODE_BASE` (`plugin_data`, `JohnnyPluginData.hpp`).
pub fn johnnyguitar(text: &str, plugin_data: &str) -> Result<Vec<RefCmd>, String> {
    let base = constant(plugin_data, "JG_OPCODE_BASE").ok_or("JohnnyGuitar: no JG_OPCODE_BASE")?;
    Ok(reg_calls(&cpp::read(text, &["GAME"]))
        .into_iter()
        .enumerate()
        .map(|(i, (n, _))| cmd(n, Some(base + i as u32)))
        .collect())
}

/// `NAME = 0x....` in a source.
fn constant(text: &str, name: &str) -> Option<u32> {
    let k = text.find(&format!("{name} ="))?;
    let rest = text[k + name.len() + 2..].trim_start();
    let hex: String = rest
        .strip_prefix("0x")?
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();
    u32::from_str_radix(&hex, 16).ok()
}

/// ShowOff: the `REG_*` calls of `ShowOffNVSE.cpp` that carry an opcode comment ("only commands
/// with listed opcodes should be used in scripts", the source says; the rest are behind `#if 0`
/// or not released).
pub fn showoff(text: &str) -> Vec<RefCmd> {
    reg_calls(&cpp::read(text, &[]))
        .into_iter()
        .filter(|(n, op)| !n.is_empty() && op.is_some())
        .map(|(n, op)| cmd(n, op))
        .collect()
}

/// kNVSE: the `REG_CMD` calls of `main.cpp` and the commands `CreateCommands` makes with
/// `builder.Create("Name", ...)` (`commands_animation.cpp`), without the `_DEBUG` ones. Their
/// opcodes are left open (the builder hands them out).
pub fn knvse(main: &str, animation: &str) -> Vec<RefCmd> {
    let mut out: Vec<RefCmd> = reg_calls(&cpp::read(main, &[]))
        .into_iter()
        .filter(|(n, _)| !n.is_empty())
        .map(|(n, _)| cmd(n, None))
        .collect();
    for l in cpp::read(animation, &[]) {
        for c in cpp::calls(&l.code) {
            if c.name == "Create" && c.before == '.' && c.quoted {
                out.push(cmd(c.first, None));
            }
        }
    }
    out
}

/// An extender as the summary and the picture show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extender {
    pub id: String,
    pub name: String,
    /// The pinned source the reference comes from, or what stands in for it.
    pub reference: String,
    /// The crate that builds it ("" for none).
    pub crate_dir: String,
    pub total: usize,
    pub tested: Vec<String>,
    pub untested: Vec<String>,
    pub missing: Vec<String>,
}

/// The extenders shown, in order: (id in capabilities.json, display name, pin).
pub const SHOWN: &[(&str, &str, Option<&str>)] = &[
    ("xnvse", "xNVSE", Some("xnvse")),
    ("jip-ln", "JIP LN NVSE", Some("jip-ln")),
    ("johnnyguitar", "JohnnyGuitar NVSE", Some("johnnyguitar")),
    ("showoff", "ShowOff xNVSE", Some("showoff")),
    ("knvse", "kNVSE", Some("knvse")),
    ("stewie-tweaks", "lStewieAl's Tweaks", None),
    ("ttw-nvse", "TTW NVSE", None),
    ("anh-nvse", "AnhNVSE", None),
];

/// The reference list of a pinned extender, read from `sources`.
pub fn reference(sources: &Path, id: &str) -> Result<Vec<RefCmd>, String> {
    let r = |p: &str| pins::read(sources, id, p);
    Ok(match id {
        "xnvse" => {
            let files: Vec<String> = pins::files(sources, id, "nvse/nvse", &["cpp", "h"])
                .iter()
                .map(|p| {
                    String::from_utf8_lossy(&std::fs::read(p).unwrap_or_default()).into_owned()
                })
                .collect();
            xnvse(&r("nvse/nvse/CommandTable.cpp")?, &files)
        }
        "jip-ln" => jip(&r("jip_nvse.cpp")?),
        "johnnyguitar" => johnnyguitar(
            &r("JG/internal/JG/JohnnyCommands.cpp")?,
            &r("JG/internal/JG/JohnnyPluginData.hpp")?,
        )?,
        "showoff" => showoff(&r("SHOWOFF-NVSE/ShowOffNVSE.cpp")?),
        "knvse" => knvse(
            &r("nvse_plugin_example/main.cpp")?,
            &r("nvse_plugin_example/commands_animation.cpp")?,
        ),
        other => return Err(format!("no reference reader for {other}")),
    })
}

/// The test text of a crate: (file, the part that is test code), for the files with a `#[test]`.
pub fn test_texts(root: &Path, crate_dir: &str) -> Vec<(String, String)> {
    let mut files = Vec::new();
    rust_files(&root.join(crate_dir), &mut files);
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        let rel = f
            .strip_prefix(root)
            .unwrap_or(&f)
            .to_string_lossy()
            .replace('\\', "/");
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let whole = name.ends_with("_tests.rs")
            || name == "tests.rs"
            || rel.split('/').any(|c| c == "tests");
        let part = if whole {
            text.as_str()
        } else if let Some(k) = inline_tests_at(&text) {
            &text[k..]
        } else {
            continue;
        };
        if part.contains("#[test]") {
            out.push((rel, strip_comments(part)));
        }
    }
    out
}

/// Where a file's inline test module starts (`#[cfg(test)]` then `mod name {`); a
/// `#[cfg(test)] mod name;` that names another file is not one.
pub fn inline_tests_at(text: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(k) = text[from..].find("#[cfg(test)]") {
        let at = from + k;
        if let Some(m) = text[at + 12..].trim_start().strip_prefix("mod ") {
            let rest = m.trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '_');
            if rest.trim_start().starts_with('{') {
                return Some(at);
            }
        }
        from = at + 12;
    }
    None
}

/// Rust source without its comments (`//` to the end of the line, `/* */`), string and character
/// literals kept: a name in a comment ("Tests of `X`") is no call of `X`.
pub fn strip_comments(text: &str) -> String {
    let b: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == '/' && b.get(i + 1) == Some(&'/') {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && b.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < b.len() && !(b[i] == '*' && b[i + 1] == '/') {
                if b[i] == '\n' {
                    out.push('\n');
                }
                i += 1;
            }
            i += 2;
            continue;
        }
        if c == '"' {
            out.push(c);
            i += 1;
            while i < b.len() && b[i] != '"' {
                if b[i] == '\\' && i + 1 < b.len() {
                    out.push(b[i]);
                    i += 1;
                }
                out.push(b[i]);
                i += 1;
            }
            if i < b.len() {
                out.push('"');
                i += 1;
            }
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

pub fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The words (identifier characters) of a text, and the values of its hexadecimal literals.
fn words_and_hex(text: &str) -> (BTreeSet<&str>, BTreeSet<u32>) {
    let mut words = BTreeSet::new();
    let mut hex = BTreeSet::new();
    for w in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
        if w.is_empty() {
            continue;
        }
        if let Some(h) = w.strip_prefix("0x").or_else(|| w.strip_prefix("0X")) {
            let digits: String = h.chars().filter(|c| *c != '_').collect();
            // A literal's type suffix (`0x2200u32`) is not part of its value.
            let digits = digits
                .trim_end_matches(|c: char| !c.is_ascii_hexdigit())
                .split(['u', 'i'])
                .next()
                .unwrap_or("")
                .to_string();
            if let Ok(v) = u32::from_str_radix(&digits, 16) {
                hex.insert(v);
            }
        }
        words.insert(w);
    }
    (words, hex)
}

/// The reference slots a test text names (indices into `refs`): `Some` list, or `None` when the
/// file names more than [`BULK`] of them (a table check).
fn named(refs: &[RefCmd], text: &str) -> Option<BTreeSet<usize>> {
    let (words, hex) = words_and_hex(text);
    let hits: BTreeSet<usize> = refs
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            words.contains(r.name.as_str()) || r.opcode.is_some_and(|op| hex.contains(&op))
        })
        .map(|(i, _)| i)
        .collect();
    (hits.len() <= BULK).then_some(hits)
}

/// Each reference slot as tested, built without a test, or missing. `implemented` are the built
/// names (any case: script commands are matched without case, as the game does).
pub fn classify(
    refs: &[RefCmd],
    implemented: &[String],
    tests: &[(String, String)],
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let built: BTreeSet<String> = implemented.iter().map(|n| n.to_lowercase()).collect();
    let mut covered = BTreeSet::new();
    for (_, text) in tests {
        if let Some(hits) = named(refs, text) {
            covered.extend(hits);
        }
    }
    let (mut tested, mut untested, mut missing) = (Vec::new(), Vec::new(), Vec::new());
    for (i, r) in refs.iter().enumerate() {
        if !built.contains(&r.name.to_lowercase()) {
            missing.push(r.name.clone());
        } else if covered.contains(&i) {
            tested.push(r.name.clone());
        } else {
            untested.push(r.name.clone());
        }
    }
    (tested, untested, missing)
}

/// The capabilities entry of an extender (`.fork/capabilities.json`), if it has one.
fn entry<'a>(caps: &'a Value, id: &str) -> Option<&'a Value> {
    caps.get("extenders")?
        .as_arr()
        .iter()
        .find(|e| e.get("id").and_then(Value::as_str) == Some(id))
}

/// Every shown extender.
pub fn all(root: &Path, sources: &Path, caps: &Value) -> Result<Vec<Extender>, String> {
    let mut out = Vec::new();
    for (id, display, pin) in SHOWN {
        let e = entry(caps, id);
        let implemented: Vec<String> = e
            .and_then(|e| e.get("commands"))
            .and_then(|c| c.get("implemented_names"))
            .map(|n| {
                n.as_arr()
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let crate_dir = e
            .and_then(|e| e.get("provided_by_source"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if !crate_dir.is_empty()
            && (!crate_dir.starts_with("crates/")
                || crate_dir.contains("..")
                || crate_dir.contains('\\'))
        {
            return Err(format!(
                "{display}: provided_by_source {crate_dir:?} is not a crate under crates/"
            ));
        }
        let tests = if crate_dir.is_empty() {
            Vec::new()
        } else {
            test_texts(root, &crate_dir)
        };
        let (refs, reference) = match pin {
            Some(p) => {
                let pin = pins::pin(p);
                let refs = reference(sources, p)?;
                if refs.is_empty() {
                    return Err(format!("{display}: the reference list read empty"));
                }
                (
                    refs,
                    format!("{} {} ({})", pin.repo, pin.version, &pin.commit[..7]),
                )
            }
            // No public source: the slots the plugin registers, as the built-in table counts them
            // (`.fork/capabilities.json`'s total), named where the port builds them.
            None => {
                let total = e
                    .and_then(|e| e.get("commands"))
                    .and_then(|c| c.get("total"))
                    .and_then(Value::as_u64)
                    .ok_or_else(|| format!("{display}: no total in .fork/capabilities.json"))?;
                let mut refs: Vec<RefCmd> =
                    implemented.iter().map(|n| cmd(n.clone(), None)).collect();
                for k in refs.len()..total as usize {
                    refs.push(cmd(format!("(unnamed slot {})", k + 1), None));
                }
                (
                    refs,
                    "no public source: the plugin's slot count from .fork/capabilities.json"
                        .to_string(),
                )
            }
        };
        let (tested, untested, missing) = classify(&refs, &implemented, &tests);
        out.push(Extender {
            id: id.to_string(),
            name: display.to_string(),
            reference,
            crate_dir,
            total: refs.len(),
            tested,
            untested,
            missing,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xnvse_reads_the_add_functions_only_with_record_names() {
        let table = "\
void CommandTable::AddDebugCommands()
{
\tADD_CMD(tcmd);
}
void CommandTable::AddCommandsV1()
{
\tRecordReleaseVersion();
\tADD_CMD(GetNVSEVersion);
\tADD_CMD_RET(While, kRetnType_Default);
\tImportConsoleCommand(\"TFC\"); // changed from ToggleFreeCamera
#if _DEBUG
\tADD_CMD(DebugThing);
#endif
}
";
        let defs = "CommandInfo kCommandInfo_While =\n{\n\t\"while\",\n";
        let refs = xnvse(table, &[defs.to_string()]);
        assert_eq!(
            refs,
            [
                cmd("GetNVSEVersion", Some(0x1400)),
                cmd("while", Some(0x1401)),
                cmd("con_TFC", Some(0x1402)),
            ]
        );
    }

    #[test]
    fn jip_and_showoff_take_the_opcode_comments() {
        let src = "\t/*2200*/REG_CMD(GetLNVersion);\n\t/*2201*/REG_EMPTY;\n\t/*2202*/REG_CMD_STR(GetTimeStamp);\n#if JIP_DEBUG\n\tREG_CMD(Debug);\n#endif\n";
        assert_eq!(
            jip(src),
            [
                cmd("GetLNVersion", Some(0x2200)),
                cmd("GetTimeStamp", Some(0x2202))
            ]
        );
        let so = "#define REG_CMD(name) x\n\t\t/*3C93*/ REG_CMD(ModNumericGameSetting)\n\t\tREG_CMD(IsGameSetting)\n#if 0\n\t\t/*3C94*/ REG_CMD(Never)\n#endif\n";
        assert_eq!(showoff(so), [cmd("ModNumericGameSetting", Some(0x3c93))]);
    }

    #[test]
    fn johnnyguitar_counts_from_its_base() {
        let data = "\tconstexpr uint32_t JG_OPCODE_BASE = 0x3100;\n";
        let src = "#define REG_CMD(name) x\n\tREG_CMD(A);\n\tREG_TYPED_CMD(B, Form);\n";
        assert_eq!(
            johnnyguitar(src, data).unwrap(),
            [cmd("A", Some(0x3100)), cmd("B", Some(0x3101))]
        );
        assert!(johnnyguitar(src, "").is_err());
    }

    #[test]
    fn knvse_adds_the_builders_commands() {
        let main = "\tREG_CMD(ForcePlayIdle);\n#if _DEBUG\n\tREG_CMD(kNVSETest);\n#endif\n";
        let anim = "\tbuilder.Create(\"GetAnim\", kRetnType_Array, {}, true, [](COMMAND_ARGS) {});\n\tother.Create(x);\n";
        assert_eq!(
            knvse(main, anim),
            [cmd("ForcePlayIdle", None), cmd("GetAnim", None)]
        );
    }

    #[test]
    fn tested_needs_a_name_or_an_opcode_in_a_test_and_no_list() {
        let refs = vec![
            cmd("Alpha", Some(0x2200)),
            cmd("Beta", Some(0x2201)),
            cmd("Gamma", Some(0x2202)),
            cmd("Delta", Some(0x2203)),
        ];
        let built = vec!["alpha".to_string(), "BETA".into(), "Gamma".into()];
        let tests = vec![
            (
                "a_tests.rs".to_string(),
                "const OP: u32 = 0x22_00;\n#[test] fn t() {}".to_string(),
            ),
            (
                "b_tests.rs".to_string(),
                "// Tests of `Beta`\n#[test] fn t() {}".to_string(),
            ),
        ];
        let (t, u, m) = classify(&refs, &built, &tests);
        assert_eq!(t, ["Alpha", "Beta"]);
        assert_eq!(u, ["Gamma"]);
        assert_eq!(m, ["Delta"]);

        // A file listing every name is a table check: it tests none of them.
        let many: Vec<RefCmd> = (0..=BULK).map(|i| cmd(format!("C{i}"), None)).collect();
        let list: String = many.iter().map(|c| format!("\"{}\",", c.name)).collect();
        let names: Vec<String> = many.iter().map(|c| c.name.clone()).collect();
        let (t, u, _) = classify(&many, &names, &[("list_tests.rs".into(), list)]);
        assert!(t.is_empty());
        assert_eq!(u.len(), BULK + 1);
    }

    #[test]
    fn comments_name_nothing() {
        let t = strip_comments(
            "// Tests of `Alpha`\nlet x = \"// Beta\"; /* Gamma\n */ call(0x2200);\n",
        );
        assert!(!t.contains("Alpha") && t.contains("// Beta") && !t.contains("Gamma"));
        assert_eq!(t.lines().count(), 3, "lines kept");
    }

    #[test]
    fn hex_literals_with_suffixes_and_separators() {
        let (_, hex) = words_and_hex("0x2200u32 0x0040_1000 0X1f");
        assert!(hex.contains(&0x2200) && hex.contains(&0x401000) && hex.contains(&0x1f));
    }
}
