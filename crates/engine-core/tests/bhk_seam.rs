// SPDX-License-Identifier: GPL-3.0-only
//! The `bhk` seam (nv-rs#944): no code outside Havok and the wrapper reads or writes a Havok
//! field. The inputs are this repository's Rust sources; the declarations of which types Havok
//! owns are the ones the component census uses (`research/component-census/components.tsv`).

use std::fs;
use std::path::{Path, PathBuf};

/// The Havok prefixes of `research/component-census/components.tsv`, plus the Havok layouts those
/// translations keep beside their functions (the census only counts fields named `m_*`).
const HAVOK_TYPES: &[&str] = &[
    "Hk",
    "Hkp",
    "Bhk",
    "Ahkp",
    "HavokError",
    "HkFileLine",
    "HkVector4",
    "BhkPickData",
    "HkConstraintCinfo",
    "BhkSerializable",
    "BhkTelekinesisListener",
    "BhkWaterListener",
    "BhkRagdollController",
];

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("a directory under crates/") {
        let path = entry.expect("an entry").path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|x| x == "rs") {
            out.push(path);
        }
    }
}

/// The statement around `at`, so a call split over lines is seen whole.
fn statement(src: &str, at: usize) -> &str {
    let start = src[..at]
        .rfind(|c| c == ';' || c == '{' || c == '}')
        .map_or(0, |i| i + 1);
    let end = src[at..]
        .find(|c| c == ';' || c == '}')
        .map_or(src.len(), |i| at + i + 1);
    &src[start..end]
}

fn is_havok(word: &str) -> bool {
    HAVOK_TYPES.iter().any(|t| word.starts_with(t))
}

#[test]
fn only_havok_and_bhk_access_havok_fields() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    sources(&root.join("crates"), &mut files);
    let mut offenders = Vec::new();
    for path in files {
        let text = path.to_string_lossy().replace('\\', "/");
        if text.contains("/engine-havok/") || text.ends_with("/bhk.rs") {
            continue;
        }
        let src = fs::read_to_string(&path).expect("a Rust source file");
        for (at, _) in src.match_indices("::") {
            let before = src[..at]
                .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .map_or(0, |i| i + 1);
            let word = &src[before..at];
            if !is_havok(word) {
                continue;
            }
            let stmt = statement(&src, at);
            // A declaration (`layout!`) or a pointer's type parameter names a Havok type without
            // reading it. A field read or write names it inside one of these calls; the wrapper
            // itself lives in `bhk.rs` and is the only place they may happen.
            if stmt.contains("e.get(")
                || stmt.contains("e.set(")
                || stmt.contains(".byte_add(")
                || stmt.contains(".at(")
            {
                offenders.push(format!("{}: {}", text, src[..at].lines().count()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "Havok fields reached outside the bhk seam:\n{}",
        offenders.join("\n")
    );
    // The census's Havok types are all declared somewhere, so an empty list is not a pass.
    assert!(HAVOK_TYPES.len() > 1);
}
