//! The generated files against the schemas checked into `.fork/schemas/`, and the checked-in
//! `.fork/capabilities.json` against the command tables: change a command table and this fails
//! until `task capabilities` (or `release-files capabilities --out .fork/capabilities.json`) has
//! rewritten the file.

use release_files::{capabilities, commands_of_slots, manifest, to_text, ManifestInput};
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
    // 1,150 slots: 32 empty, and SetLinkedReference at two opcodes is one command.
    assert_eq!(ext(1)["total"].as_u64().unwrap(), 1117);
}

/// Every extender names each implemented command once: a command a plugin registers at two
/// opcodes is one command (JIP LN's `SetLinkedReference`, at `0x2281` and `0x2634`).
#[test]
fn every_extender_names_each_command_once() {
    let c = capabilities();
    for e in c["extenders"].as_array().unwrap() {
        let names: Vec<&str> = e["commands"]["implemented_names"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_str().unwrap())
            .collect();
        let distinct: std::collections::BTreeSet<&str> = names.iter().copied().collect();
        assert_eq!(
            names.len(),
            distinct.len(),
            "{} names a command twice",
            e["id"]
        );
    }
    let jip = &c["extenders"][1]["commands"];
    let linked = jip["implemented_names"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| *n == "SetLinkedReference")
        .count();
    assert_eq!(linked, 1);
}

/// A name in two slots is one command, implemented when either slot is, named at its first slot.
#[test]
fn a_command_in_two_slots_counts_once() {
    let (names, total) = commands_of_slots([
        ("A", true),
        ("B", false),
        ("C", true),
        ("B", true),
        ("A", true),
        ("D", false),
    ]);
    assert_eq!(names, ["A", "B", "C"]);
    assert_eq!(total, 4);
    let (names, total) = commands_of_slots([("X", false), ("X", false)]);
    assert!(names.is_empty());
    assert_eq!(total, 1);
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

/// Each other plugin Tale of Two Wastelands' start-up check asks for is listed with a version its
/// check accepts, the condition, and exactly the commands of it that are built: the ones TTW's
/// scripts call (id, the least version TTW 3.4 accepts, the built commands).
#[test]
fn the_plugins_tale_of_two_wastelands_asks_for_say_what_they_have() {
    let c = capabilities();
    let plugins: [(&str, f64, &[&str]); 4] = [
        (
            "ttw-nvse",
            1.0,
            &[
                "TTW_ShowGeneProjector",
                "TTW_GetPlugins",
                "TTW_EnableRadioFix",
            ],
        ),
        (
            "johnnyguitar",
            501.0,
            &[
                "GetQuestFailed",
                "IsDLLLoaded",
                "GetExtraMiscStat",
                "ModExtraMiscStat",
                "InitExtraMiscStat",
            ],
        ),
        ("showoff", 180.0, &["GetActorHasBaseFlag"]),
        ("stewie-tweaks", 941.0, &[]),
    ];
    for (id, least, built) in plugins {
        let entry = c["extenders"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["id"] == id)
            .unwrap_or_else(|| panic!("{id} is listed"));
        assert!(entry["version"].as_f64().unwrap() >= least, "{id}");
        let when = entry["reported_installed"].as_str().unwrap();
        assert!(when.contains("TaleOfTwoWastelands.esm"), "{id}: {when}");
        assert!(when.contains("NV_TTW_PLUGINS"), "{id}: {when}");
        let names: Vec<&str> = entry["commands"]["implemented_names"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_str().unwrap())
            .collect();
        // The commands Tale of Two Wastelands calls come first and are all there; the commands
        // built since for other mods follow them (their own tests name them), so the list is
        // not spelled out here a second time.
        assert_eq!(&names[..built.len()], built, "{id}");
        let commands = &entry["commands"];
        assert_eq!(
            commands["implemented"].as_u64().unwrap(),
            names.len() as u64
        );
        assert!(
            commands["total"].as_u64().unwrap() >= names.len() as u64,
            "{id}"
        );
    }
    // Tweaks has no command built: its entry says what the answer stands for.
    let tweaks = c["extenders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == "stewie-tweaks")
        .unwrap();
    assert_eq!(tweaks["commands"]["total"], 8);
    let when = tweaks["reported_installed"].as_str().unwrap();
    assert!(
        when.contains("NV_STEWIE") && when.contains("engine fixes"),
        "{when}"
    );
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

/// The folding by name is for the one command JIP LN registers twice: any other name in two of
/// its slots is a slip in the slot table, which the folding would otherwise hide.
#[test]
fn only_set_linked_reference_sits_in_two_jip_slots() {
    let mut seen = std::collections::BTreeMap::new();
    for slot in jip::slots::SLOTS.iter() {
        if slot.kind != jip::table::Kind::Empty {
            *seen.entry(slot.name).or_insert(0) += 1;
        }
    }
    let repeated: Vec<(&str, i32)> = seen.into_iter().filter(|(_, n)| *n > 1).collect();
    assert_eq!(repeated, [("SetLinkedReference", 2)]);
}
