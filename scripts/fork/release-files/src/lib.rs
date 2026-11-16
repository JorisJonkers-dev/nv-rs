//! Fork-only. The two machine-readable files a nv-rs release carries for nvmod (nv-linux
//! docs/architecture/integration.md, sections "nv-rs" and "Built in per target"):
//!
//! - `capabilities.json`: per built-in script extender (xNVSE, JIP LN) the implemented and total
//!   command counts and the implemented command names, read from the real command tables.
//! - `release-manifest.json`: the version, the upstream base commit, the expected hashes of the
//!   supported game executable's sections, the play-patch step and whether a plugins.txt
//!   ordering switch exists.
//!
//! Names, counts and hashes of this repository's own tables only: no game data. The schemas are
//! in `.fork/schemas/`.

use engine_core::prelude::*;
use serde_json::{json, Value};

// The one program-wide list of translated functions `Engine::new` registers: none here, the
// tool reads tables only.
engine_core::assemble!();

/// The contract versions this tool writes (`nv-rs-capabilities.vN`, `nv-rs-release-manifest.vN`).
pub const SCHEMA_VERSION: u64 = 1;

/// xNVSE's commands: the built ones plus the console commands it re-exports (copies of the
/// console's own records, `con_*` in the name table), against the 654 of its name table.
/// Names in opcode order.
fn xnvse() -> (Vec<String>, usize) {
    let mut built: Vec<(u32, String)> = nvse::commands::all()
        .iter()
        .map(|c| (c.opcode, c.name.to_string()))
        .collect();
    for (i, (name, _)) in nvse::names::NAMES.iter().enumerate() {
        if name.starts_with("con_") {
            built.push((nvse::names::FIRST_OPCODE + i as u32, name.to_string()));
        }
    }
    built.sort();
    (
        built.into_iter().map(|(_, n)| n).collect(),
        nvse::names::NAMES.len(),
    )
}

/// An engine with xNVSE's table installed over empty stand-ins of the exe's console and script
/// tables, as the crates' own tests make one (`crates/jip/src/test_support.rs`).
fn engine() -> Engine {
    let mut e = Engine::new();
    e.map(0x0118_e000, 0x9000);
    e.map(0x0108_7000, 0x3000);
    e.map(0x0300_0000, 0x1_0000);
    nvse::table::install(&mut e);
    e.register(0x00fd_f394, |_, _| Ret {
        eax: 0x48f,
        ..Ret::default()
    });
    // The extenders' installs also patch game code at fixed addresses, and the tool has no
    // image of the exe. Stand in for it instead of for one address at a time: every page of the
    // image reads zero, and every function the wrap table lists is a stand-in that returns 0, so
    // a new patch upstream adds needs nothing here. Only the command tables are read afterwards.
    e.map(0x0040_0000, 0x00f0_0000);
    for entry in engine_core::extend::all_wraps() {
        if !e.is_registered(entry.addr) {
            e.register(entry.addr, |_, _| Ret::default());
        }
    }
    e
}

/// JIP LN's commands, read from the table JIP installs when it answers "installed": a slot that
/// holds a command and has an execute or an eval is implemented (that covers the commands of its
/// own hooks and the identity as well as the listed handlers). Against the slots that hold a
/// command (a slot that is empty in JIP's own table is not a command). A command JIP registers
/// at two opcodes counts, and is named, twice.
fn jip_ln() -> (Vec<String>, usize) {
    let mut e = engine();
    jip::install_as(&mut e, true);
    let mut total = 0;
    let mut names = Vec::new();
    for slot in jip::slots::SLOTS.iter() {
        if slot.kind == jip::table::Kind::Empty {
            continue;
        }
        total += 1;
        let record = nvse::table::record_of(&e, slot.opcode);
        if e.mem.u32(record + 0x18) != 0 || e.mem.u32(record + 0x20) != 0 {
            names.push(slot.name.to_string());
        }
    }
    (names, total)
}

fn extender(id: &str, display: &str, source: &str, names: Vec<String>, total: usize) -> Value {
    json!({
        "id": id,
        "name": display,
        "provided_by_source": source,
        "commands": {
            "implemented": names.len(),
            "total": total,
            "implemented_names": names,
        },
    })
}

/// `capabilities.json`, generated from the command tables.
pub fn capabilities() -> Value {
    let (x_names, x_total) = xnvse();
    let (j_names, j_total) = jip_ln();
    json!({
        "schema_version": SCHEMA_VERSION,
        "extenders": [
            extender("xnvse", "xNVSE", "crates/nvse", x_names, x_total),
            extender("jip-ln", "JIP LN NVSE", "crates/jip", j_names, j_total),
        ],
    })
}

/// What the manifest needs from the build that it cannot read from the tables.
pub struct ManifestInput {
    /// The release version, as the release build names it.
    pub version: String,
    /// The last upstream commit the fork line was replayed to (the `Upstream-Commit` trailer).
    pub upstream_base_commit: String,
    /// The fork commit the binaries are built from.
    pub source_commit: String,
    /// The git tree hash of that commit (a source build is checked against it).
    pub source_tree_hash: String,
}

/// `release-manifest.json`. The executable's expected section hashes are the ones
/// `engine_core::exe` checks before it maps the player's executable.
pub fn manifest(input: &ManifestInput) -> Value {
    let sections: Vec<Value> = engine_core::exe::SECTIONS
        .iter()
        .map(|(name, va, vsize, hash)| {
            json!({
                "name": name,
                "virtual_address": format!("0x{va:08x}"),
                "virtual_size": format!("0x{vsize:08x}"),
                "hash": format!("{hash:016x}"),
            })
        })
        .collect();
    json!({
        "schema_version": SCHEMA_VERSION,
        "version": input.version,
        "upstream_base_commit": input.upstream_base_commit,
        "source_commit": input.source_commit,
        "source_tree_hash": input.source_tree_hash,
        "executable": {
            "file_name": "FalloutNV.exe",
            "game_version": "1.4.0.525",
            "hash_algorithm": "fnv1a-64",
            "hashed_bytes": "raw bytes of each listed section, as the file stores them",
            "sections": sections,
        },
        "play_patch": {
            "standalone_step": false,
            "command": null,
            "arguments": [],
            "description": "play-patch is a crate linked into nv-viewer, not a program: translations fixed during a play-build run, registered over the engine's own when the game starts. It takes no base copy and no instance; there is nothing for a manager to run.",
        },
        "installation_check": {
            "command": "nv-viewer",
            "arguments": ["<data folder>", "--check"],
            "description": "Checks that the installation's FalloutNV.exe is the supported build and what a run will find; exit status 0 when the game can start, 1 when not.",
        },
        "plugins_txt_ordering_switch": false,
        "files": {
            "capabilities": "capabilities.json",
            "checksums": "SHA256SUMS",
        },
    })
}

/// A pretty JSON text with a trailing newline, for the files in the repository and the release.
pub fn to_text(v: &Value) -> String {
    let mut s = serde_json::to_string_pretty(v).expect("a JSON value serialises");
    s.push('\n');
    s
}

/// The ways `instance` breaks `schema` (JSON Schema 2020-12), one text each; empty when it validates.
pub fn schema_errors(schema: &Value, instance: &Value) -> Result<Vec<String>, String> {
    let validator = jsonschema::validator_for(schema).map_err(|e| format!("the schema: {e}"))?;
    Ok(validator
        .iter_errors(instance)
        .map(|e| format!("{}: {e}", e.instance_path))
        .collect())
}
