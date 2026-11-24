// SPDX-License-Identifier: GPL-3.0-only
//! The coverage catalogue's generated files (`.fork/catalogue`, docs/fork/CATALOGUE.md): the
//! reference lists in `ref/` and their pins in `sources.toml`, read from the same pinned sources
//! and repository files as the progress pictures, and the implementation evidence of the groups
//! that have no importer of their own in tally (Direct3D 9 and the graphics plan), written for
//! `tally check` to compare with what the catalogue declares.
//!
//! The group files themselves (`.fork/catalogue/<area>/<group>.toml`) are kept by hand: they say
//! what is implemented; these lists say what exists.

use crate::{extenders, features, pins, sha256, Progress};
use std::collections::BTreeMap;
use std::path::Path;

/// What tally makes of a name as an id segment (tally's `segment`, the capabilities importer's
/// rule): lower case, every other character `_`, `x` in front when it does not start with a
/// letter, `_` after a Rust keyword.
pub fn segment(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if !s.starts_with(|c: char| c.is_ascii_lowercase()) {
        s.insert(0, 'x');
    }
    if KEYWORDS.contains(&s.as_str()) {
        s.push('_');
    }
    s
}

/// Words that cannot name a Rust module or constant (tally's list).
const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut",
    "pub", "ref", "return", "self", "static", "struct", "super", "trait", "true", "type", "unsafe",
    "use", "where", "while", "abstract", "become", "box", "do", "final", "macro", "override",
    "priv", "try", "typeof", "unsized", "virtual", "yield", "union",
];

/// One row of a reference list: the item's local id, the name as the source writes it, and one
/// more column (an opcode, a slot, a value or a group).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub key: String,
    pub title: String,
    pub extra: String,
}

/// A reference list as tally reads it: `key<TAB>title<TAB><extra>`, in the source's order, from
/// (key, title, extra) rows. A key listed again with the same title is one row (JIP LN registers
/// `SetLinkedReference` at two opcodes); two different titles that give the same key are an error.
pub fn list(
    what: &str,
    extra: &str,
    rows: impl IntoIterator<Item = (String, String, String)>,
) -> Result<String, String> {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    let mut out = format!("key\ttitle\t{extra}\n");
    for (key, title, more) in rows {
        match seen.get(&key) {
            Some(t) if *t == title => continue,
            Some(t) => {
                return Err(format!(
                    "{what}: `{t}` and `{title}` both give the id `{key}`"
                ))
            }
            None => {}
        }
        seen.insert(key.clone(), title.clone());
        out.push_str(&format!("{key}\t{title}\t{more}\n"));
    }
    if seen.is_empty() {
        return Err(format!("{what}: the list reads empty"));
    }
    Ok(out)
}

/// Rows keyed by the name's id segment.
fn named(
    rows: impl IntoIterator<Item = (String, String)>,
) -> impl Iterator<Item = (String, String, String)> {
    rows.into_iter().map(|(n, e)| (segment(&n), n, e))
}

/// One generated reference list: its file under `ref/`, the pin it is read from (`None`: the
/// repository itself) and the files of that pin it is read from.
pub struct RefList {
    pub file: String,
    pub pin: Option<&'static str>,
    pub upstream: Vec<String>,
    pub text: String,
}

/// The extender pins, by catalogue file name (the extender id as an id segment).
const EXTENDERS: &[(&str, &str)] = &[
    ("xnvse", "xnvse"),
    ("jip-ln", "jip_ln"),
    ("johnnyguitar", "johnnyguitar"),
    ("showoff", "showoff"),
    ("knvse", "knvse"),
];

fn hex(op: Option<u32>) -> String {
    op.map(|o| format!("0x{o:04x}")).unwrap_or_default()
}

/// Every reference list: the extenders' commands, Direct3D 9's device methods and render states,
/// the graphics plan and the engine's functions.
pub fn ref_lists(
    root: &Path,
    sources: &Path,
    p: &Progress,
    ledger_tsv: &str,
) -> Result<Vec<RefList>, String> {
    let mut out = Vec::new();
    for (pin, name) in EXTENDERS {
        let cmds = extenders::reference(sources, pin)?;
        let rows = cmds
            .iter()
            .filter(|c| !c.name.is_empty())
            .map(|c| (c.name.clone(), hex(c.opcode)));
        out.push(RefList {
            file: format!("ref/{name}.tsv"),
            pin: Some(pin),
            upstream: upstream_files(sources, pin)?,
            text: list(name, "opcode", named(rows))?,
        });
    }
    let d3d9_files = upstream_files(sources, "d3d9-headers")?;
    let methods = p
        .d3d9
        .methods
        .iter()
        .enumerate()
        .map(|(i, m)| (m.clone(), i.to_string()));
    out.push(RefList {
        file: "ref/d3d9_methods.tsv".into(),
        pin: Some("d3d9-headers"),
        upstream: d3d9_files
            .iter()
            .filter(|f| f.ends_with("/d3d9.h"))
            .cloned()
            .collect(),
        text: list("d3d9 methods", "slot", named(methods))?,
    });
    let states = crate::d3d9::render_states(&pins::read(
        sources,
        "d3d9-headers",
        "mingw-w64-headers/include/d3d9types.h",
    )?)?;
    out.push(RefList {
        file: "ref/d3d9_states.tsv".into(),
        pin: Some("d3d9-headers"),
        upstream: d3d9_files
            .iter()
            .filter(|f| f.ends_with("/d3d9types.h"))
            .cloned()
            .collect(),
        text: list(
            "d3d9 render states",
            "value",
            states
                .iter()
                .map(|(n, v)| (segment(n), format!("D3DRS_{n}"), v.to_string())),
        )?,
    });
    let plan = features::read_list(
        &std::fs::read_to_string(root.join("docs/fork/graphics-features.tsv"))
            .map_err(|e| format!("docs/fork/graphics-features.tsv: {e}"))?,
    )?;
    out.push(RefList {
        file: "ref/graphics_features.tsv".into(),
        pin: None,
        upstream: vec!["docs/fork/graphics-features.tsv".into()],
        text: list(
            "graphics plan",
            "group",
            plan.iter()
                .map(|c| (segment(&c.id), c.name.clone(), c.group.clone())),
        )?,
    });
    out.push(RefList {
        file: "ref/engine_functions.tsv".into(),
        pin: None,
        upstream: vec!["scripts/ledger (--tsv)".into()],
        text: engine_functions(ledger_tsv)?,
    });
    Ok(out)
}

/// The engine's functions in the ledger's order: key `f` and the address as eight lower-case hex
/// digits (tally's ledger importer), title the function's name or, without one, its address, and
/// the ledger's status (platform and library code is not game code: n/a in the catalogue).
pub fn engine_functions(ledger_tsv: &str) -> Result<String, String> {
    let mut lines = ledger_tsv.lines();
    let head: Vec<&str> = lines.next().unwrap_or("").split('\t').collect();
    let col = |n: &str| {
        head.iter()
            .position(|c| *c == n)
            .ok_or(format!("ledger TSV: no column {n}"))
    };
    let (addr, name, status) = (col("address")?, col("name")?, col("status")?);
    let mut out = String::from("key\ttitle\tstatus\n");
    let mut n = 0;
    for l in lines.filter(|l| !l.is_empty()) {
        let f: Vec<&str> = l.split('\t').collect();
        let get = |k: usize| f.get(k).copied().unwrap_or("").trim();
        let a = get(addr).trim_start_matches("0x").to_ascii_lowercase();
        if a.len() != 8 || !a.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("ledger TSV: address {:?}", get(addr)));
        }
        let title = if get(name).is_empty() {
            format!("0x{a}")
        } else {
            get(name).replace('\t', " ")
        };
        out.push_str(&format!("f{a}\t{title}\t{}\n", get(status)));
        n += 1;
    }
    if n == 0 {
        return Err("ledger TSV: no functions".into());
    }
    Ok(out)
}

/// The files of a pin a reference list is read from, relative to the pin's checkout, sorted
/// (xNVSE: every source of `nvse/nvse/`, where its command names are defined).
fn upstream_files(sources: &Path, pin: &str) -> Result<Vec<String>, String> {
    let p = pins::pin(pin);
    let mut out = Vec::new();
    for path in p.paths {
        let rel = path.trim_start_matches('/');
        if let Some(dir) = rel.strip_suffix('/') {
            for f in pins::files(sources, pin, dir, &["cpp", "h"]) {
                let name = f
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                out.push(format!("{dir}/{name}"));
            }
        } else {
            out.push(rel.to_string());
        }
    }
    if out.is_empty() {
        return Err(format!("{pin}: no files read"));
    }
    out.sort();
    Ok(out)
}

/// `sources.toml`: one source per pinned list, with the SHA-256 of the list and of every upstream
/// file it is read from.
pub fn sources_toml(sources: &Path, lists: &[RefList]) -> Result<String, String> {
    let mut out = String::from(
        "# SPDX-License-Identifier: GPL-3.0-only\n# Generated by scripts/fork/progress (`progress catalogue`) from the pins in scripts/fork/progress/src/pins.rs;\n# do not edit. Move a pin there and run `task catalogue` (docs/fork/CATALOGUE.md).\n",
    );
    for l in lists {
        let Some(pin) = l.pin else { continue };
        let p = pins::pin(pin);
        let id = l
            .file
            .trim_start_matches("ref/")
            .trim_end_matches(".tsv")
            .replace('_', "-");
        out.push_str(&format!(
            "\n[[source]]\nid = \"{id}\"\nname = \"{}\"\nrepo = \"https://github.com/{}\"\ntag = \"{}\"\ncommit = \"{}\"\nlicence = \"{}\"\nread = \"{}\"\nextractor = \"manual\"\nlist = \"{}\"\nsha256 = \"{}\"\n\n[source.upstream]\n",
            display(pin),
            p.repo,
            p.version,
            p.commit,
            licence(pin),
            p.read,
            l.file,
            sha256::hex(l.text.as_bytes())
        ));
        for f in &l.upstream {
            let bytes = std::fs::read(pins::dir(sources, pin).join(f))
                .map_err(|e| format!("{pin} {f}: {e}"))?;
            out.push_str(&format!("\"{f}\" = \"{}\"\n", sha256::hex(&bytes)));
        }
    }
    Ok(out)
}

fn display(pin: &str) -> &'static str {
    match pin {
        "xnvse" => "xNVSE",
        "jip-ln" => "JIP LN NVSE",
        "johnnyguitar" => "JohnnyGuitar NVSE",
        "showoff" => "ShowOff xNVSE",
        "knvse" => "kNVSE",
        "d3d9-headers" => "mingw-w64 Direct3D 9 headers",
        _ => "?",
    }
}

/// The licence each source states (GitHub's reading of its licence file); only names and counts
/// are taken from them.
fn licence(pin: &str) -> &'static str {
    match pin {
        "jip-ln" => "GPL-3.0",
        "johnnyguitar" => "LGPL-2.1",
        "d3d9-headers" => "NOASSERTION (mingw-w64-headers: public domain notice)",
        _ => "none stated",
    }
}

/// The implementation evidence of the groups tally has no importer for: `id<TAB>state` rows with
/// the full item id, `implemented`, `partial` or `missing`, from the same reading of the code as
/// the pictures (a device method carried to wgpu, or only answered; a render state mapped; a
/// graphics capability a built-in setting declares).
pub fn evidence(p: &Progress) -> String {
    let mut out = String::from("id\tstate\n");
    let mut row = |id: String, state: &str| out.push_str(&format!("{id}\t{state}\n"));
    for m in &p.d3d9.carried {
        row(format!("d3d9.methods.{}", segment(m)), "implemented");
    }
    for m in &p.d3d9.answered_only {
        row(format!("d3d9.methods.{}", segment(m)), "partial");
    }
    for m in &p.d3d9.not_answered {
        row(format!("d3d9.methods.{}", segment(m)), "missing");
    }
    for s in &p.d3d9.mapped {
        row(format!("d3d9.states.{}", segment(s)), "implemented");
    }
    for s in p.d3d9.warned.iter().chain(&p.d3d9.ignored) {
        row(format!("d3d9.states.{}", segment(s)), "missing");
    }
    for (id, _) in p.features.done.iter().chain(&p.features.declared) {
        row(format!("gfx.features.{}", segment(id)), "implemented");
    }
    for id in &p.features.open {
        row(format!("gfx.features.{}", segment(id)), "missing");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_become_tallys_id_segments() {
        assert_eq!(segment("GetLNVersion"), "getlnversion");
        assert_eq!(segment("con_SetGameSetting"), "con_setgamesetting");
        assert_eq!(segment("Return"), "return_");
        assert_eq!(segment("2DTexture"), "x2dtexture");
        assert_eq!(segment("hdr-tonemap"), "hdr_tonemap");
        assert_eq!(segment("ZENABLE"), "zenable");
    }

    #[test]
    fn a_name_listed_twice_is_one_row_and_a_clash_is_an_error() {
        let rows = |v: &[&str]| {
            named(v.iter().map(|n| (n.to_string(), String::new()))).collect::<Vec<_>>()
        };
        let t = list(
            "jip",
            "opcode",
            rows(&["A", "SetLinkedReference", "B", "SetLinkedReference"]),
        )
        .unwrap();
        assert_eq!(
            t,
            "key\ttitle\topcode\na\tA\t\nsetlinkedreference\tSetLinkedReference\t\nb\tB\t\n"
        );
        assert!(list("x", "opcode", rows(&["GetX", "getx"])).is_err());
        assert!(list("x", "opcode", rows(&[])).is_err());
    }

    #[test]
    fn engine_functions_are_keyed_by_address() {
        let ledger = "address\tsize\tname\tsubsystem\tunit\tstatus\trust\ttyped\n00401000\t21\t\ta\tb\ttranslated\tx\t\n00ECD291\t4\tfree\ta\tb\tplatform\t\t\n";
        assert_eq!(
            engine_functions(ledger).unwrap(),
            "key\ttitle\tstatus\nf00401000\t0x00401000\ttranslated\nf00ecd291\tfree\tplatform\n"
        );
        assert!(engine_functions("address\tname\tstatus\n401000\tx\ttranslated\n").is_err());
        assert!(engine_functions("address\tname\tstatus\n").is_err());
        assert!(engine_functions("name\tstatus\n").is_err());
    }
}
