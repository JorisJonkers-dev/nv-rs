//! Fork-only. The two machine-readable files a nv-rs release carries for nvmod (nv-linux
//! docs/architecture/integration.md, sections "nv-rs" and "Built in per target"):
//!
//! - `capabilities.json`: per built-in script extender (xNVSE, JIP LN, and the plugins that
//!   answer for a game with Tale of Two Wastelands) the implemented and total command counts and
//!   the implemented command names, read from the real command tables; for an extender that
//!   answers "installed" only on a condition, the version it reports and the condition.
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
/// own hooks and the identity as well as the listed handlers). Against the commands its slots
/// hold (a slot that is empty in JIP's own table is not a command). A command JIP registers at
/// two opcodes (`SetLinkedReference`, at `0x2281` and `0x2634`) is one command: it counts, and
/// is named, once, and it is implemented when one of its slots is.
fn jip_ln() -> (Vec<String>, usize) {
    let mut e = engine();
    jip::install_as(&mut e, true);
    let slots = jip::slots::SLOTS
        .iter()
        .filter(|slot| slot.kind != jip::table::Kind::Empty)
        .map(|slot| {
            let record = nvse::table::record_of(&e, slot.opcode);
            let built = e.mem.u32(record + 0x18) != 0 || e.mem.u32(record + 0x20) != 0;
            (slot.name, built)
        });
    commands_of_slots(slots)
}

/// The implemented names and the number of commands of a plugin's slots, in slot order: a name
/// held by several slots is one command, implemented when any of its slots is, named once at its
/// first slot.
pub fn commands_of_slots<'a>(
    slots: impl IntoIterator<Item = (&'a str, bool)>,
) -> (Vec<String>, usize) {
    let mut order: Vec<&str> = Vec::new();
    let mut built: std::collections::HashMap<&str, bool> = std::collections::HashMap::new();
    for (name, is_built) in slots {
        match built.get_mut(name) {
            Some(b) => *b |= is_built,
            None => {
                order.push(name);
                built.insert(name, is_built);
            }
        }
    }
    let names = order
        .iter()
        .filter(|n| built[*n])
        .map(|n| (*n).to_string())
        .collect();
    (names, order.len())
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

/// What `reported_installed` says for a plugin `host::fork_ttw` switches on.
const WITH_TTW: &str =
    "when the data folder holds TaleOfTwoWastelands.esm (NV_TTW_PLUGINS=1 forces it, 0 turns it off)";

/// A plugin that answers "installed" only for a game with Tale of Two Wastelands (nv-rs#399): the
/// extender entry, the version it hands xNVSE and the condition. `names` are the commands built;
/// `total` is the number of opcode slots the real plugin registers at that version.
fn ttw_plugin(
    id: &str,
    display: &str,
    source: &str,
    version: u32,
    names: &[&str],
    total: usize,
) -> Value {
    let names = names.iter().map(|n| n.to_string()).collect();
    let mut entry = extender(id, display, source, names, total);
    entry["version"] = json!(version);
    entry["reported_installed"] = json!(WITH_TTW);
    entry
}

/// JohnnyGuitar NVSE's public registration list holds 412 commands from `0x3100` (2026-11-20).
const JG_TOTAL: usize = 412;

/// ShowOff xNVSE 1.84 registers the opcodes `0x3C93` to `0x3D69` (its public registration list).
const SHOWOFF_TOTAL: usize = 0x3d69 - 0x3c93 + 1;

/// lStewieAl's Tweaks' script commands (docs/FIXES.md, "lStewieAl's Tweaks": eight, none built).
const STEWIE_COMMANDS: usize = 8;

/// lStewieAl's Tweaks: no command of it is built and Tale of Two Wastelands calls none, so its
/// entry says what the answer stands for.
fn stewie_entry() -> Value {
    let mut entry = ttw_plugin(
        "stewie-tweaks",
        "lStewieAl's Tweaks",
        "crates/stewie",
        stewie::VERSION,
        &[],
        STEWIE_COMMANDS,
    );
    entry["reported_installed"] = json!(format!(
        "{WITH_TTW}, and only with the row NV_STEWIE on: it stands for the engine fixes built so far (docs/FIXES.md), not for its script commands, which Tale of Two Wastelands does not call"
    ));
    entry
}

/// `capabilities.json`, generated from the command tables.
pub fn capabilities() -> Value {
    let (x_names, x_total) = xnvse();
    let (j_names, j_total) = jip_ln();
    let mut jip_entry = extender("jip-ln", "JIP LN NVSE", "crates/jip", j_names, j_total);
    jip_entry["version"] = json!(jip::identity::version());
    jip_entry["reported_installed"] = json!(
        "when the data folder holds TaleOfTwoWastelands.esm (NV_JIP_INSTALLED=1 forces it, 0 turns it off)"
    );
    json!({
        "schema_version": SCHEMA_VERSION,
        "extenders": [
            extender("xnvse", "xNVSE", "crates/nvse", x_names, x_total),
            jip_entry,
            ttw_plugin(
                "johnnyguitar",
                "JohnnyGuitar NVSE",
                "crates/jg",
                jg::identity::VERSION_INT,
                &jg::COMMAND_NAMES,
                JG_TOTAL,
            ),
            ttw_plugin(
                "showoff",
                "ShowOff xNVSE",
                "crates/showoff",
                showoff::identity::VERSION_INT,
                &showoff::COMMAND_NAMES,
                SHOWOFF_TOTAL,
            ),
            stewie_entry(),
            ttw_plugin(
                "ttw-nvse",
                "Tale of Two Wastelands plugin (ttw_nvse)",
                "crates/ttw-nvse",
                ttw_nvse::VERSION,
                &ttw_nvse::COMMAND_NAMES,
                ttw_nvse::COMMAND_NAMES.len(),
            ),
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
