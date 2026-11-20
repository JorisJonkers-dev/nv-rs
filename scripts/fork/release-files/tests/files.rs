//! The generated files against the schemas checked into `.fork/schemas/`, and the checked-in
//! `.fork/capabilities.json` against the command tables: change a command table and this fails
//! until `task capabilities` (or `release-files capabilities --out .fork/capabilities.json`) has
//! rewritten the file.

use release_files::{capabilities, manifest, to_text, ManifestInput};
use serde_json::Value;
use std::path::PathBuf;

fn repo(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(path)
}

fn load(path: &str) -> Value {
    let text = std::fs::read_to_string(repo(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn input() -> ManifestInput {
    ManifestInput {
        version: "2026-01-01.1".into(),
        upstream_base_commit: "a".repeat(40),
        source_commit: "b".repeat(40),
        source_tree_hash: "c".repeat(40),
    }
}

fn errors(schema: &Value, instance: &Value) -> Vec<String> {
    release_files::schema_errors(schema, instance).expect("the schema compiles")
}

#[test]
fn the_schemas_are_valid_json_schemas() {
    for name in ["nv-rs-capabilities.v1", "nv-rs-release-manifest.v1"] {
        let schema = load(&format!(".fork/schemas/{name}.json"));
        assert!(
            jsonschema::meta::is_valid(&schema),
            "{name} is not a valid schema"
        );
    }
}

#[test]
fn the_generated_capabilities_validate() {
    let schema = load(".fork/schemas/nv-rs-capabilities.v1.json");
    assert_eq!(errors(&schema, &capabilities()), Vec::<String>::new());
}

#[test]
fn the_generated_manifest_validates() {
    let schema = load(".fork/schemas/nv-rs-release-manifest.v1.json");
    assert_eq!(errors(&schema, &manifest(&input())), Vec::<String>::new());
}

#[test]
fn the_schemas_reject_what_they_should() {
    let caps = load(".fork/schemas/nv-rs-capabilities.v1.json");
    let man = load(".fork/schemas/nv-rs-release-manifest.v1.json");
    let mut c = capabilities();
    c["extenders"][0]["commands"]
        .as_object_mut()
        .unwrap()
        .remove("total");
    assert!(!errors(&caps, &c).is_empty(), "a missing total passes");
    let mut m = manifest(&input());
    m["upstream_base_commit"] = "not-a-commit".into();
    assert!(!errors(&man, &m).is_empty(), "a bad commit id passes");
    let mut m = manifest(&input());
    m.as_object_mut()
        .unwrap()
        .remove("plugins_txt_ordering_switch");
    assert!(!errors(&man, &m).is_empty(), "a missing switch passes");
}

/// The counts are the tables': implemented is the number of names, never more than the total,
/// and the totals are what the tables hold.
#[test]
fn the_counts_come_from_the_tables() {
    let c = capabilities();
    let ext = |i: usize| &c["extenders"][i]["commands"];
    for i in 0..2 {
        let names = ext(i)["implemented_names"].as_array().unwrap().len() as u64;
        assert_eq!(ext(i)["implemented"].as_u64().unwrap(), names);
        assert!(names <= ext(i)["total"].as_u64().unwrap());
        assert!(names > 0);
    }
    assert_eq!(
        ext(0)["total"].as_u64().unwrap(),
        nvse::names::NAMES.len() as u64
    );
    // JIP's listed handlers, plus the commands its identity and hooks register on their own.
    assert!(ext(1)["implemented"].as_u64().unwrap() >= jip::commands::built() as u64);
    assert_eq!(ext(1)["total"].as_u64().unwrap(), 1118);
}

/// JIP LN's entry carries the version a mod's check reads (`GetPluginVersion "JIP NVSE Plugin"`:
/// Tale of Two Wastelands wants 57.2 or later) and says when the engine answers "installed".
#[test]
fn jip_reports_the_version_tale_of_two_wastelands_reads() {
    let c = capabilities();
    let jip = &c["extenders"][1];
    assert_eq!(jip["id"], "jip-ln");
    assert!(jip["version"].as_f64().unwrap() >= 57.2);
    assert!(jip["reported_installed"]
        .as_str()
        .unwrap()
        .contains("TaleOfTwoWastelands.esm"));
    assert!(c["extenders"][0].get("version").is_none());
}

#[test]
fn the_manifest_hashes_are_the_ones_the_engine_checks() {
    let m = manifest(&input());
    let sections = m["executable"]["sections"].as_array().unwrap();
    assert_eq!(sections.len(), engine_core::exe::SECTIONS.len());
    for (got, (name, _, _, hash)) in sections.iter().zip(engine_core::exe::SECTIONS.iter()) {
        assert_eq!(got["name"], *name);
        assert_eq!(got["hash"], format!("{hash:016x}"));
    }
    assert_eq!(m["plugins_txt_ordering_switch"], false);
}

#[test]
fn the_checked_in_capabilities_match_the_tables() {
    // A Windows checkout may have turned the line ends into CR LF.
    let have = std::fs::read_to_string(repo(".fork/capabilities.json"))
        .expect(".fork/capabilities.json is checked in")
        .replace("\r\n", "\n");
    assert!(
        have == to_text(&capabilities()),
        ".fork/capabilities.json disagrees with the command tables: regenerate it with \
         `cargo run --locked --manifest-path scripts/fork/release-files/Cargo.toml -- capabilities --out .fork/capabilities.json`"
    );
}
