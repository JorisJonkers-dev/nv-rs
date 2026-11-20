// SPDX-License-Identifier: GPL-3.0-only
//! The plugin's commands (`0x3800` up), from how TTW's scripts call them (`NOTICE`).

use crate::State;
use engine_core::prelude::*;
use nvse::commands::Read;
use nvse::table::{self, Param, PluginRecord, Returns};
use nvse::vars::{self, Key, Value, KEY_NUMBER};
use nvse::{handled, CommandArgs};

/// `kParamType_Integer`.
const INT: u32 = 0x01;

/// The opcodes, from the third-party dump and TTW's compiled scripts.
pub const SHOW_GENE_PROJECTOR: u32 = 0x3800;
pub const GET_PLUGINS: u32 = 0x3806;
pub const ENABLE_RADIO_FIX: u32 = 0x3808;

/// The data handler (`0x011c3f2c`), its list of loaded files (`+0x210`: a node is the file and
/// the next node), a file's name (`+0x20`) and its header's version float (`+0x3dc`, the HEDR
/// version).
const DATA_HANDLER: u32 = 0x011c_3f2c;
const MOD_INFO_LIST: u32 = 0x210;
const MOD_NAME: u32 = 0x20;
const HEADER_VERSION: u32 = 0x3dc;

/// A header version as a script sees it: the float's shortest decimal reading, as a double, so
/// 1.3 compares equal to 1.3 and not below it.
pub fn version_number(v: f32) -> f64 {
    if v.is_finite() {
        format!("{v}").parse().unwrap_or(f64::from(v))
    } else {
        f64::from(v)
    }
}

/// The loaded files in load order: name and header version (files with no record skipped).
fn loaded_plugins(e: &mut Engine) -> Vec<(Vec<u8>, f64)> {
    let mut out = Vec::new();
    let mut node = e.global::<u32>(DATA_HANDLER);
    if node == 0 {
        return out;
    }
    node += MOD_INFO_LIST;
    while node != 0 {
        let info = e.mem.u32(node);
        if info != 0 {
            let name = table::guest_text(e, info + MOD_NAME);
            out.push((name, version_number(e.mem.f32(info + HEADER_VERSION))));
        }
        node = e.mem.u32(node + 4);
    }
    out
}

/// `TTW_GetPlugins`: an array with one `[name, version]` pair for each file of the load order,
/// the name as text and the version as a number. Owned by the calling script's mod, as a
/// plugin's array result is.
fn get_plugins(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let modi = vars::script_mod_index(e, a.script);
    let list = vars::create_array(e, KEY_NUMBER, true, modi);
    for (i, (name, version)) in loaded_plugins(e).into_iter().enumerate() {
        let pair = vars::create_array(e, KEY_NUMBER, true, modi);
        vars::set_element(e, pair, &Key::Number(0.0), Value::Str(name));
        vars::set_element(e, pair, &Key::Number(1.0), Value::Number(version));
        vars::set_element(e, list, &Key::Number(i as f64), Value::Array(pair));
    }
    a.set_result(e, f64::from(list));
    handled(true)
}

/// `TTW_EnableRadioFix 0|1`: stores whether it is on. The radio fix itself is not built.
fn enable_radio_fix(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    let read = Read::of(e, &a);
    if read.ok {
        e.ext::<State>().radio_fix = read.int(0, 0) != 0;
    }
    handled(true)
}

const ONE_INT: &[Param] = &[Param {
    name: "int",
    kind: INT,
    optional: false,
}];

/// A parameter list in the built-ins' data (12 bytes a parameter: name, type, optional).
fn guest_params(e: &mut Engine, params: &[Param]) -> u32 {
    let at = table::alloc(e, 12 * params.len() as u32);
    for (i, param) in params.iter().enumerate() {
        let name = table::guest_str(e, param.name.as_bytes());
        let slot = at + 12 * i as u32;
        e.mem.set_u32(slot, name);
        e.mem.set_u32(slot + 4, param.kind);
        e.mem.set_u32(slot + 8, u32::from(param.optional));
    }
    at
}

/// The game's own command that opens the race and sex menu (menu 1036), whose handler
/// `TTW_ShowGeneProjector` runs.
pub const RACE_MENU: &[u8] = b"ShowRaceMenu";

/// The game's record of [`RACE_MENU`], when the game's table has it with a handler.
fn race_menu(e: &Engine) -> Option<table::Record> {
    let opcode = table::opcode_by_name(e, RACE_MENU)?;
    let at = table::record_of(e, opcode);
    (at != 0)
        .then(|| table::Record::read(e, at))
        .filter(|record| record.execute != 0)
}

/// Registers the commands: the table grown to them, `TTW_GetPlugins` and `TTW_EnableRadioFix`
/// with their handlers, and `TTW_ShowGeneProjector` with the handler and parameter list of the
/// game's own `ShowRaceMenu`: TTW's scripts call it where the player chooses face and body and
/// then wait in `MenuMode 1036`, the race and sex menu that command opens. What the real plugin
/// changes in that menu for the gene projector is not in a public document and is not built.
/// Without the game's command (a table that lacks it) the record has its name alone and a call
/// fails.
pub fn install(e: &mut Engine) {
    table::grow_to(e, ENABLE_RADIO_FIX + 1);
    let game = race_menu(e);
    let gene_projector = PluginRecord {
        name: crate::COMMAND_NAMES[0],
        short: "",
        needs_ref: false,
        param_count: game.as_ref().map_or(0, |r| r.param_count),
        params: game.as_ref().map_or(0, |r| r.params),
        execute: game.as_ref().map_or(0, |r| r.execute),
        eval: 0,
        flags: 0,
        expression: false,
        returns: Returns::Default,
    };
    table::add_plugin_record(e, SHOW_GENE_PROJECTOR, &gene_projector);
    e.ext::<State>().gene_projector = game.is_some();
    let built: [(u32, &str, &[Param], AbiFn, Returns); 2] = [
        (
            GET_PLUGINS,
            crate::COMMAND_NAMES[1],
            &[],
            get_plugins,
            Returns::Array,
        ),
        (
            ENABLE_RADIO_FIX,
            crate::COMMAND_NAMES[2],
            ONE_INT,
            enable_radio_fix,
            Returns::Default,
        ),
    ];
    for (opcode, name, params, run, returns) in built {
        let execute = table::execute_address(opcode);
        e.register(execute, run);
        let record = PluginRecord {
            name,
            short: "",
            needs_ref: false,
            param_count: params.len() as u16,
            params: guest_params(e, params),
            execute,
            eval: 0,
            flags: 0,
            expression: false,
            returns,
        };
        table::add_plugin_record(e, opcode, &record);
    }
}
