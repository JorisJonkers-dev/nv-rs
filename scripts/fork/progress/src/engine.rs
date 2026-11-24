// SPDX-License-Identifier: GPL-3.0-only
//! The engine port and its native 64-bit code, from upstream's own function ledger
//! (`scripts/ledger`, run with `--tsv`): every function of FalloutNV.exe 1.4.0.525 in the engine
//! map with the status the ledger derives from the code (translation markers in the engine
//! crates, the generated start-up and shut-down tables, typed markers of registered functions).
//! The typed families (ADR-0014) are read from their declarations (`pub static X: Family`).

use std::path::Path;

/// The ledger's counts as upstream's progress bar splits them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Port {
    /// Translated by hand: a translation marker in an engine crate.
    pub by_hand: usize,
    /// Static initializers run from the generated start-up table.
    pub startup_table: usize,
    /// `atexit` destructors run from the generated shut-down table.
    pub shutdown_table: usize,
    /// Still open, or only cited outside the engine crates.
    pub open: usize,
    pub traced: usize,
    /// Stood in for by the platform layer, or compiler and runtime-library code: not game code.
    pub platform: usize,
    pub library: usize,
    /// Translated functions with a registered typed version (`// Typed from <addr>`).
    pub typed: usize,
}

impl Port {
    /// The game functions upstream's bar counts: everything but platform and library code.
    pub fn game_functions(&self) -> usize {
        self.by_hand + self.startup_table + self.shutdown_table + self.open + self.traced
    }
}

/// The ledger's per-function TSV (`address size name subsystem unit status rust typed`).
pub fn read_ledger(text: &str) -> Result<Port, String> {
    let mut p = Port::default();
    let mut lines = text.lines();
    let head = lines.next().unwrap_or("");
    let cols: Vec<&str> = head.split('\t').collect();
    let col = |n: &str| {
        cols.iter()
            .position(|c| *c == n)
            .ok_or(format!("ledger TSV: no column {n}"))
    };
    let (status, rust, typed) = (col("status")?, col("rust")?, col("typed")?);
    for (i, l) in lines.enumerate() {
        if l.is_empty() {
            continue;
        }
        let f: Vec<&str> = l.split('\t').collect();
        let get = |k: usize| f.get(k).copied().unwrap_or("");
        match get(status) {
            "translated" => p.by_hand += 1,
            "static init (table)" => {
                let locs = get(rust);
                if locs.contains("atexit_destructors.tsv") {
                    p.shutdown_table += 1;
                } else if locs.contains("static_initializers.tsv") {
                    p.startup_table += 1;
                } else {
                    return Err(format!(
                        "ledger TSV line {}: a table function in no table",
                        i + 2
                    ));
                }
            }
            "traced" => p.traced += 1,
            "open" => p.open += 1,
            "platform" => p.platform += 1,
            "library" => p.library += 1,
            other => return Err(format!("ledger TSV line {}: status {other:?}", i + 2)),
        }
        if get(typed) == "yes" {
            p.typed += 1;
        }
    }
    if p.game_functions() == 0 {
        return Err("ledger TSV: no functions".into());
    }
    Ok(p)
}

/// One typed family as declared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Family {
    pub name: String,
    pub file: String,
    /// It reads its objects through the stage-2 store, so it can move them to host memory.
    pub stage2: bool,
}

/// Every `pub static NAME: Family = Family { name: "...", ..., stage2: bool, ... }` under
/// `crates/`, sorted by name.
pub fn families(root: &Path) -> Vec<Family> {
    let mut files = Vec::new();
    crate::extenders::rust_files(&root.join("crates"), &mut files);
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
        // A family declared in test code is no family of the engine.
        if rel.split('/').any(|c| c == "tests") || rel.ends_with("_tests.rs") {
            continue;
        }
        out.extend(families_in(&crate::d3d9::code_only(&text), &rel));
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn families_in(text: &str, file: &str) -> Vec<Family> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(k) = rest.find(": Family = Family {") {
        let line_start = rest[..k].rfind('\n').map_or(0, |n| n + 1);
        let decl = &rest[line_start..k];
        rest = &rest[k..];
        let end = rest.find("};").unwrap_or(rest.len());
        let body = &rest[..end];
        rest = &rest[end..];
        if !decl.trim_start().starts_with("pub static ") {
            continue;
        }
        let field = |n: &str| {
            body.find(&format!("{n}:"))
                .map(|i| body[i + n.len() + 1..].trim_start())
                .map(|v| v.split([',', '\n']).next().unwrap_or("").trim().to_string())
        };
        let Some(name) = field("name") else { continue };
        out.push(Family {
            name: name.trim_matches('"').to_string(),
            file: file.to_string(),
            stage2: field("stage2").as_deref() == Some("true"),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ledger_splits_as_upstream_does() {
        let tsv = "address\tsize\tname\tsubsystem\tunit\tstatus\trust\ttyped
00401000\t1\t\t\t\ttranslated\tcrates/engine-x/src/a.rs:3\tyes
00401010\t1\t\t\t\ttranslated\tcrates/engine-x/src/a.rs:9\t
00401020\t1\t\t\t\tstatic init (table)\tcrates/engine-core/data/static_initializers.tsv:7\t
00401030\t1\t\t\t\tstatic init (table)\tcrates/engine-core/data/atexit_destructors.tsv:9\t
00401040\t1\t\t\t\tplatform\t\t
00401050\t1\t\t\t\tlibrary\t\t
00401060\t1\t\t\t\topen\t\t
";
        let p = read_ledger(tsv).unwrap();
        assert_eq!(
            p,
            Port {
                by_hand: 2,
                startup_table: 1,
                shutdown_table: 1,
                open: 1,
                traced: 0,
                platform: 1,
                library: 1,
                typed: 1
            }
        );
        assert_eq!(p.game_functions(), 5);
        assert!(
            read_ledger("address\tstatus\n").is_err(),
            "a column missing"
        );
        assert!(read_ledger(&tsv.replace("open", "mystery")).is_err());
    }

    #[test]
    fn families_from_their_declarations() {
        let src = r#"
pub static LOCKFREE: Family = Family {
    name: "lockfree",
    id: FAMILY,
    past_gate: true,
    stage2: true,
    funcs,
};
static PRIVATE: Family = Family { name: "hidden", stage2: false, };
pub static LEAVES: Family = Family {
    name: "leaves",
    stage2: false,
    funcs: leaf_funcs,
};
"#;
        let f = families_in(src, "x.rs");
        assert_eq!(
            f.iter()
                .map(|f| (f.name.as_str(), f.stage2))
                .collect::<Vec<_>>(),
            [("lockfree", true), ("leaves", false)]
        );
    }
}
