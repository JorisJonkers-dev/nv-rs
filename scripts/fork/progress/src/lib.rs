// SPDX-License-Identifier: GPL-3.0-only
//! Fork-only. The progress pictures (`docs/media/fork-progress-*.svg`) and their summary
//! (`docs/media/fork-progress.json`), every number derived from the code and from reference lists
//! read from pinned outside sources; nothing is kept by hand. docs/fork/PROGRESS.md says what each
//! number counts and how a count is proven.

pub mod cpp;
pub mod d3d9;
pub mod draw;
pub mod engine;
pub mod extenders;
pub mod features;
pub mod json;
pub mod pins;
pub mod svg;

use json::{obj, Value};
use std::collections::BTreeSet;
use std::path::Path;

/// The files this tool writes, relative to the repository root. The workflow that commits them
/// commits these and nothing else.
pub const OUTPUTS: &[&str] = &[
    "docs/media/fork-progress.json",
    "docs/media/fork-progress-engine.svg",
    "docs/media/fork-progress-native.svg",
    "docs/media/fork-progress-d3d9.svg",
    "docs/media/fork-progress-extenders.svg",
    "docs/media/fork-progress-features.svg",
];

/// What the tool reads.
pub struct Inputs<'a> {
    pub root: &'a Path,
    /// The pinned sources (`progress fetch`).
    pub sources: &'a Path,
    /// The ledger's per-function TSV (`scripts/ledger -- --tsv FILE`).
    pub ledger_tsv: &'a str,
}

/// Everything counted.
pub struct Progress {
    pub port: engine::Port,
    pub families: Vec<engine::Family>,
    pub d3d9: D3d9,
    pub extenders: Vec<extenders::Extender>,
    pub features: features::Features,
}

pub struct D3d9 {
    pub methods: Vec<String>,
    pub carried: Vec<String>,
    pub answered_only: Vec<String>,
    pub not_answered: Vec<String>,
    pub states: usize,
    pub mapped: Vec<String>,
    pub warned: Vec<String>,
    pub ignored: Vec<String>,
}

fn read(root: &Path, rel: &str) -> Result<String, String> {
    std::fs::read_to_string(root.join(rel)).map_err(|e| format!("{rel}: {e}"))
}

fn d3d9(root: &Path, sources: &Path) -> Result<D3d9, String> {
    let h = |p: &str| pins::read(sources, "d3d9-headers", p);
    let methods = d3d9::device_methods(&h("mingw-w64-headers/include/d3d9.h")?)?;
    let states = d3d9::render_states(&h("mingw-w64-headers/include/d3d9types.h")?)?;
    let slots = d3d9::answered_slots(&read(root, "crates/engine-platform/src/d3d9device.rs")?)?;
    let cmds = d3d9::stream_commands(&read(root, "crates/d3d9-stream/src/cmd.rs")?, &methods);
    if cmds.is_empty() {
        return Err("crates/d3d9-stream/src/cmd.rs: no command read".into());
    }
    let backend = read(root, "crates/d3d9-wgpu/src/lib.rs")?;
    let handled = d3d9::handled(d3d9::without_tests(&backend));
    let carried_lc: BTreeSet<String> = cmds
        .iter()
        .filter(|(c, _)| handled.contains(c))
        .flat_map(|(_, m)| m.iter().cloned())
        .collect();
    let (mut carried, mut answered_only, mut not_answered) = (Vec::new(), Vec::new(), Vec::new());
    for (slot, m) in methods.iter().enumerate() {
        if carried_lc.contains(&m.to_lowercase()) {
            carried.push(m.clone());
        } else if slots.contains(&(slot as u32)) {
            answered_only.push(m.clone());
        } else {
            not_answered.push(m.clone());
        }
    }
    let mut files = Vec::new();
    extenders::rust_files(&root.join("crates/d3d9-wgpu/src"), &mut files);
    files.sort();
    let texts: Vec<String> = files
        .iter()
        .filter(|f| !f.to_string_lossy().ends_with("_tests.rs"))
        .filter_map(|f| std::fs::read_to_string(f).ok())
        .map(|t| d3d9::without_tests(&t).to_string())
        .collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let (mut mapped, mut warned, mut ignored) = (Vec::new(), Vec::new(), Vec::new());
    for (name, _) in &states {
        match d3d9::state_use(&refs, name) {
            d3d9::StateUse::Mapped => mapped.push(name.clone()),
            d3d9::StateUse::Warned => warned.push(name.clone()),
            d3d9::StateUse::Ignored => ignored.push(name.clone()),
        }
    }
    Ok(D3d9 {
        methods,
        carried,
        answered_only,
        not_answered,
        states: states.len(),
        mapped,
        warned,
        ignored,
    })
}

/// Counts everything.
pub fn measure(inp: &Inputs) -> Result<Progress, String> {
    let port = engine::read_ledger(inp.ledger_tsv)?;
    let families = engine::families(inp.root);
    if families.is_empty() {
        return Err("no typed family declared under crates/".into());
    }
    let caps = json::parse(&read(inp.root, ".fork/capabilities.json")?)?;
    let extenders = extenders::all(inp.root, inp.sources, &caps)?;
    let list = features::read_list(&read(inp.root, "docs/fork/graphics-features.tsv")?)?;
    let decls = features::read_declarations(&read(inp.root, "crates/fork-gfx/src/atlas.rs")?);
    let tests = extenders::test_texts(inp.root, "crates");
    let features = features::classify(&list, &decls, &tests)?;
    Ok(Progress {
        port,
        families,
        d3d9: d3d9(inp.root, inp.sources)?,
        extenders,
        features,
    })
}

/// The counts a pull request must not lower without saying why, by name.
pub fn counts(p: &Progress) -> Vec<(String, usize)> {
    let mut c = vec![
        ("engine.by_hand".to_string(), p.port.by_hand),
        (
            "engine.tables".into(),
            p.port.startup_table + p.port.shutdown_table,
        ),
        ("native.typed".into(), p.port.typed),
        ("native.families".into(), p.families.len()),
        (
            "native.stage2_families".into(),
            p.families.iter().filter(|f| f.stage2).count(),
        ),
        ("d3d9.methods_carried".into(), p.d3d9.carried.len()),
        (
            "d3d9.methods_answered".into(),
            p.d3d9.carried.len() + p.d3d9.answered_only.len(),
        ),
        ("d3d9.states_mapped".into(), p.d3d9.mapped.len()),
        ("features.done".into(), p.features.done.len()),
        (
            "features.declared".into(),
            p.features.done.len() + p.features.declared.len(),
        ),
    ];
    for e in &p.extenders {
        c.push((format!("extenders.{}.tested", e.id), e.tested.len()));
        c.push((
            format!("extenders.{}.built", e.id),
            e.tested.len() + e.untested.len(),
        ));
    }
    c.sort();
    c
}

fn summary(p: &Progress) -> Value {
    let port = &p.port;
    let stage2: Vec<&str> = p
        .families
        .iter()
        .filter(|f| f.stage2)
        .map(|f| f.name.as_str())
        .collect();
    let fam: Vec<&str> = p.families.iter().map(|f| f.name.as_str()).collect();
    let d = &p.d3d9;
    let f = &p.features;
    let pairs = |v: &[(String, Vec<String>)]| {
        Value::Arr(
            v.iter()
                .map(|(id, keys)| obj([("id", Value::str(id)), ("settings", Value::strs(keys))]))
                .collect(),
        )
    };
    obj([
        ("schema", Value::num(1u64)),
        (
            "generated_by",
            Value::str("scripts/fork/progress (docs/fork/PROGRESS.md); do not edit"),
        ),
        (
            "pins",
            Value::Arr(
                pins::PINS
                    .iter()
                    .map(|p| {
                        obj([
                            ("id", Value::str(p.id)),
                            ("repo", Value::str(p.repo)),
                            ("commit", Value::str(p.commit)),
                            ("version", Value::str(p.version)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "counts",
            Value::Obj(
                counts(p)
                    .into_iter()
                    .map(|(k, n)| (k, Value::num(n as u64)))
                    .collect(),
            ),
        ),
        (
            "engine_port",
            obj([
                ("game_functions", Value::num(port.game_functions() as u64)),
                ("by_hand", Value::num(port.by_hand as u64)),
                ("startup_table", Value::num(port.startup_table as u64)),
                ("shutdown_table", Value::num(port.shutdown_table as u64)),
                ("traced", Value::num(port.traced as u64)),
                ("open", Value::num(port.open as u64)),
                ("platform", Value::num(port.platform as u64)),
                ("library", Value::num(port.library as u64)),
            ]),
        ),
        (
            "native_64bit",
            obj([
                ("typed", Value::num(port.typed as u64)),
                ("translated_by_hand", Value::num(port.by_hand as u64)),
                ("families", Value::strs(&fam)),
                ("stage2_families", Value::strs(&stage2)),
            ]),
        ),
        (
            "d3d9_on_wgpu",
            obj([
                ("device_methods", Value::num(d.methods.len() as u64)),
                ("carried", Value::strs(&d.carried)),
                ("answered_only", Value::strs(&d.answered_only)),
                ("not_answered", Value::strs(&d.not_answered)),
                ("render_states", Value::num(d.states as u64)),
                ("mapped", Value::strs(&d.mapped)),
                ("warned", Value::strs(&d.warned)),
                ("ignored", Value::strs(&d.ignored)),
            ]),
        ),
        (
            "extenders",
            Value::Arr(
                p.extenders
                    .iter()
                    .map(|e| {
                        obj([
                            ("id", Value::str(&e.id)),
                            ("name", Value::str(&e.name)),
                            ("reference", Value::str(&e.reference)),
                            ("crate", Value::str(&e.crate_dir)),
                            ("total", Value::num(e.total as u64)),
                            ("tested", Value::num(e.tested.len() as u64)),
                            ("untested", Value::strs(&e.untested)),
                            ("missing", Value::strs(&e.missing)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "engine_features",
            obj([
                ("capabilities", Value::num(f.total as u64)),
                ("done", pairs(&f.done)),
                ("declared_untested", pairs(&f.declared)),
                ("open", Value::strs(&f.open)),
            ]),
        ),
    ])
}

/// Every output file: (path relative to the root, contents). The pictures are drawn from the
/// summary ([`draw`]), never from the counts directly.
pub fn render(p: &Progress) -> Vec<(&'static str, String)> {
    let summary = summary(p);
    let pictures = draw::all(&summary).expect("a summary this tool wrote draws");
    std::iter::once(json::write(&summary))
        .chain(pictures)
        .enumerate()
        .map(|(i, text)| (OUTPUTS[i], text))
        .collect()
}

/// The pictures of a summary file (`progress draw`).
pub fn render_summary(summary_json: &str) -> Result<Vec<(&'static str, String)>, String> {
    let summary = json::parse(summary_json)?;
    Ok(draw::all(&summary)?
        .into_iter()
        .enumerate()
        .map(|(i, text)| (OUTPUTS[i + 1], text))
        .collect())
}

/// The counts of a summary written before (`counts`), by name.
pub fn baseline_counts(summary_json: &str) -> Result<Vec<(String, usize)>, String> {
    let v = json::parse(summary_json)?;
    let Some(Value::Obj(m)) = v.get("counts") else {
        return Err("the baseline summary has no counts".into());
    };
    Ok(m.iter()
        .filter_map(|(k, n)| n.as_u64().map(|n| (k.clone(), n as usize)))
        .collect())
}

/// The counts that went down from `before` to `after` (a count that is gone counts as 0).
pub fn drops(before: &[(String, usize)], after: &[(String, usize)]) -> Vec<(String, usize, usize)> {
    before
        .iter()
        .filter_map(|(k, b)| {
            let a = after.iter().find(|(n, _)| n == k).map_or(0, |(_, a)| *a);
            (a < *b).then(|| (k.clone(), *b, a))
        })
        .collect()
}

/// The check's report, as Markdown.
pub fn report(p: &Progress, before: &[(String, usize)]) -> String {
    let mut s =
        String::from("### Progress counts\n\n| Count | Before | Now |\n| --- | ---: | ---: |\n");
    for (k, n) in counts(p) {
        let b = before
            .iter()
            .find(|(x, _)| *x == k)
            .map_or("-".to_string(), |(_, b)| b.to_string());
        s.push_str(&format!("| `{k}` | {b} | {n} |\n"));
    }
    s.push_str("\n### Built commands without a test\n\n");
    for e in &p.extenders {
        if e.untested.is_empty() {
            continue;
        }
        let first: Vec<&str> = e.untested.iter().take(12).map(String::as_str).collect();
        s.push_str(&format!(
            "- {}: {} ({}{})\n",
            e.name,
            e.untested.len(),
            first.join(", "),
            if e.untested.len() > first.len() {
                ", ..."
            } else {
                ""
            }
        ));
    }
    s
}
