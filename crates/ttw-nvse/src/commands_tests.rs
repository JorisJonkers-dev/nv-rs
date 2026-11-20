// SPDX-License-Identifier: GPL-3.0-only
//! Tests of the plugin's commands against how TTW's scripts call them.

use crate::commands::{ENABLE_RADIO_FIX, GET_PLUGINS, SHOW_GENE_PROJECTOR};
use crate::COMMAND_NAMES;
use engine_core::prelude::*;
use nvse::vars::{self, Data, Key};

const DATA: u32 = 0x0300_0000;
const OFFSET: u32 = 0x0300_0f00;
const RESULT: u32 = 0x0300_0f08;
const SCRIPT: u32 = 0x0300_1000;
const LIST: u32 = 0x0300_2000;
const INFOS: u32 = 0x0300_4000;
const DATA_HANDLER: u32 = 0x011c_3f2c;
const HANDLER_AT: u32 = 0x0118_f000;

fn engine(on: bool) -> Engine {
    let mut e = Engine::new();
    e.map(0x0118_e000, 0x9000);
    e.map(0x011d_e000, 0x1000);
    e.map(0x011c_3000, 0x1000);
    e.map(DATA, 0x1_0000);
    nvse::table::install(&mut e);
    e.mem.set_u32(DATA_HANDLER, HANDLER_AT);
    crate::set_answers_installed(&mut e, on);
    crate::install(&mut e);
    e
}

/// Loads a file into the list: node `i`, a file record with its name and header version.
fn load(e: &mut Engine, i: u32, name: &[u8], version: f32) {
    let info = INFOS + 0x800 * i;
    e.mem.write(info + 0x20, name);
    e.mem.set_u8(info + 0x20 + name.len() as u32, 0);
    e.mem.set_f32(info + 0x3dc, version);
    let node = if i == 0 {
        HANDLER_AT + 0x210
    } else {
        0x0300_8000 + 8 * i
    };
    e.mem.set_u32(node, info);
    e.mem.set_u32(node + 4, 0);
    if i > 0 {
        let prev = if i == 1 {
            HANDLER_AT + 0x210
        } else {
            0x0300_8000 + 8 * (i - 1)
        };
        e.mem.set_u32(prev + 4, node);
    }
}

fn int(v: i32) -> Vec<u8> {
    let mut b = vec![b'n'];
    b.extend_from_slice(&v.to_le_bytes());
    b
}

fn call(e: &mut Engine, opcode: u32, args: &[Vec<u8>]) -> (u32, f64) {
    e.mem.set_u16(DATA, opcode as u16);
    let bytes: Vec<u8> = args.concat();
    e.mem.set_u16(DATA + 2, bytes.len() as u16 + 2);
    e.mem.set_u16(DATA + 4, args.len() as u16);
    for (i, b) in bytes.iter().enumerate() {
        e.mem.set_u8(DATA + 6 + i as u32, *b);
    }
    e.mem.set_u32(OFFSET, 4);
    e.mem.set_f64(RESULT, -1.0);
    let record = nvse::table::record_of(e, opcode);
    assert_ne!(record, 0, "{opcode:04x}");
    let params = e.mem.u32(record + 0x14);
    let execute = e.mem.u32(record + 0x18);
    let ok = e
        .call(execute, &[params, DATA, 0, 0, SCRIPT, LIST, RESULT, OFFSET])
        .eax
        & 0xff;
    (ok, e.mem.f64(RESULT))
}

fn element(e: &mut Engine, id: u32, i: usize) -> Data {
    let upper = vars::upper_table(e);
    vars::array(e, id)
        .expect("array")
        .get(&upper, &Key::Number(i as f64))
        .expect("element")
        .data
        .clone()
}

fn name_of(e: &Engine, opcode: u32) -> Vec<u8> {
    nvse::table::guest_text(e, e.mem.u32(nvse::table::record_of(e, opcode)))
}

#[test]
fn the_plugin_is_listed_with_its_identity_when_on() {
    let e = engine(true);
    assert_eq!(
        nvse::plugins::listed(&e, b"ttw_nvse"),
        Some((crate::NAME, crate::VERSION))
    );
    assert_eq!(crate::NAME, "ttw_nvse");
    assert_eq!(crate::VERSION, 1);
    assert_eq!(crate::OPCODE_BASE, 0x3800);
    assert_eq!(crate::FILE, "ttw_nvse.dll");
}

#[test]
fn off_registers_nothing() {
    let e = engine(false);
    assert_eq!(nvse::plugins::listed(&e, b"ttw_nvse"), None);
    assert!(nvse::plugins::built_ins(&e).is_empty());
    assert_eq!(nvse::table::opcode_by_name(&e, b"TTW_GetPlugins"), None);
}

#[test]
fn a_second_install_does_nothing() {
    let mut e = engine(true);
    crate::install(&mut e);
    assert_eq!(nvse::plugins::built_ins(&e).len(), 1);
}

#[test]
fn the_record_names_are_registered_at_their_opcodes() {
    let e = engine(true);
    assert_eq!(
        COMMAND_NAMES,
        ["TTW_ShowGeneProjector", "TTW_GetPlugins", "TTW_EnableRadioFix"]
    );
    assert_eq!(name_of(&e, GET_PLUGINS), b"TTW_GetPlugins");
    assert_eq!(name_of(&e, ENABLE_RADIO_FIX), b"TTW_EnableRadioFix");
    assert_eq!(name_of(&e, SHOW_GENE_PROJECTOR), b"TTW_ShowGeneProjector");
    assert_eq!(
        nvse::table::return_type(&e, GET_PLUGINS),
        nvse::table::Returns::Array
    );
    // Without the game's ShowRaceMenu (this table has none) it has no handler and no parameters.
    let record = nvse::table::record_of(&e, SHOW_GENE_PROJECTOR);
    assert_eq!(e.mem.u32(record + 0x18), 0);
    assert_eq!(e.mem.u16(record + 0x12), 0);
    // The radio fix takes one integer.
    let record = nvse::table::record_of(&e, ENABLE_RADIO_FIX);
    assert_eq!(e.mem.u16(record + 0x12), 1);
    assert_eq!(e.mem.u32(e.mem.u32(record + 0x14) + 4), 0x01);
}

#[test]
fn get_plugins_returns_name_and_version_pairs_in_load_order() {
    let mut e = engine(true);
    load(&mut e, 0, b"FalloutNV.esm", 1.34);
    load(&mut e, 1, b"TaleOfTwoWastelands.esm", 1.32);
    load(&mut e, 2, b"Fallout3.esm", 0.94);
    let (ok, result) = call(&mut e, GET_PLUGINS, &[]);
    assert_eq!(ok, 1);
    let list = result as u32;
    assert_eq!(vars::array(&e, list).expect("list").len(), 3);
    let expected: [(&[u8], f64); 3] = [
        (b"FalloutNV.esm", 1.34),
        (b"TaleOfTwoWastelands.esm", 1.32),
        (b"Fallout3.esm", 0.94),
    ];
    for (i, (name, version)) in expected.iter().enumerate() {
        let Data::Array(pair) = element(&mut e, list, i) else {
            panic!("element {i} is not a pair");
        };
        assert_eq!(element(&mut e, pair, 0), Data::Str(name.to_vec()));
        assert_eq!(element(&mut e, pair, 1), Data::Number(*version));
    }
}

#[test]
fn a_version_of_one_point_three_does_not_compare_below_one_point_three() {
    let mut e = engine(true);
    load(&mut e, 0, b"Converted.esp", 1.3);
    load(&mut e, 1, b"Fallout3Plugin.esp", 0.85);
    let (_, result) = call(&mut e, GET_PLUGINS, &[]);
    let list = result as u32;
    let versions: Vec<f64> = (0..2)
        .map(|i| {
            let Data::Array(pair) = element(&mut e, list, i) else {
                panic!("not a pair");
            };
            match element(&mut e, pair, 1) {
                Data::Number(v) => v,
                other => panic!("{other:?}"),
            }
        })
        .collect();
    // TTW's start-up script flags `fPluginVersion < 1.3` as a mod not converted for TTW.
    assert!(versions[0] >= 1.3);
    assert!(versions[1] < 1.3);
}

#[test]
fn get_plugins_with_no_data_handler_returns_an_empty_array() {
    let mut e = engine(true);
    e.mem.set_u32(DATA_HANDLER, 0);
    let (ok, result) = call(&mut e, GET_PLUGINS, &[]);
    assert_eq!(ok, 1);
    assert_eq!(vars::array(&e, result as u32).expect("array").len(), 0);
}

#[test]
fn an_empty_node_is_skipped() {
    let mut e = engine(true);
    load(&mut e, 0, b"A.esm", 1.34);
    load(&mut e, 1, b"B.esm", 1.34);
    load(&mut e, 2, b"C.esm", 1.34);
    e.mem.set_u32(0x0300_8000 + 8, 0);
    let (_, result) = call(&mut e, GET_PLUGINS, &[]);
    assert_eq!(vars::array(&e, result as u32).expect("array").len(), 2);
}

#[test]
fn a_version_that_is_not_a_number_is_kept_as_it_is() {
    assert!(crate::commands::version_number(f32::NAN).is_nan());
    assert_eq!(
        crate::commands::version_number(f32::INFINITY),
        f64::INFINITY
    );
}

#[test]
fn enable_radio_fix_stores_the_value_and_changes_nothing_else() {
    let mut e = engine(true);
    assert!(!crate::radio_fix(&mut e));
    assert_eq!(call(&mut e, ENABLE_RADIO_FIX, &[int(1)]).0, 1);
    assert!(crate::radio_fix(&mut e));
    assert_eq!(call(&mut e, ENABLE_RADIO_FIX, &[int(0)]).0, 1);
    assert!(!crate::radio_fix(&mut e));
    // A call that gives no value takes it as 0 and still answers true.
    call(&mut e, ENABLE_RADIO_FIX, &[int(1)]);
    assert_eq!(call(&mut e, ENABLE_RADIO_FIX, &[]).0, 1);
    assert!(!crate::radio_fix(&mut e));
}

/// A table without the game's `ShowRaceMenu`: the record has its name alone, a call fails.
#[test]
fn show_gene_projector_without_the_games_race_menu_has_no_handler() {
    let mut e = engine(true);
    let record = nvse::table::record_of(&e, SHOW_GENE_PROJECTOR);
    assert_eq!(e.mem.u32(record + 0x18), 0, "no handler");
    assert!(!e.ext::<crate::State>().gene_projector);
}

/// The game's own `ShowRaceMenu` at a script opcode of the stand-in table: its handler counts
/// its calls at [`MENU_OPENED`], and its parameter list is at [`RACE_MENU_PARAMS`].
const RACE_MENU_OPCODE: u32 = 0x10a0;
const RACE_MENU_EXECUTE: u32 = 0x005c_0000;
const RACE_MENU_PARAMS: u32 = 0x0300_0e00;
const MENU_OPENED: u32 = 0x0300_0e80;

fn engine_with_the_games_race_menu(name: &[u8]) -> Engine {
    let mut e = Engine::new();
    e.map(0x0118_e000, 0x9000);
    e.map(0x011d_e000, 0x1000);
    e.map(0x011c_3000, 0x1000);
    e.map(DATA, 0x1_0000);
    nvse::table::install(&mut e);
    let at = nvse::table::record_of(&e, RACE_MENU_OPCODE);
    assert_ne!(at, 0);
    let long_name = nvse::table::guest_str(&mut e, name);
    let mut record = nvse::table::Record::read(&e, at);
    record.long_name = long_name;
    record.opcode = RACE_MENU_OPCODE;
    record.param_count = 0;
    record.params = RACE_MENU_PARAMS;
    record.execute = RACE_MENU_EXECUTE;
    record.write(&mut e, at);
    e.register(RACE_MENU_EXECUTE, |e, _| {
        let opened = e.mem.u32(MENU_OPENED);
        e.mem.set_u32(MENU_OPENED, opened + 1);
        Ret {
            eax: 1,
            ..Ret::default()
        }
    });
    crate::set_answers_installed(&mut e, true);
    crate::install(&mut e);
    e
}

/// `TTW_ShowGeneProjector` runs the handler of the game's `ShowRaceMenu` with that command's
/// parameter list: a call opens the menu once and succeeds, so the line after it runs (the
/// birth scene's `set CG00.runTimer to 1`).
#[test]
fn show_gene_projector_opens_the_games_race_menu() {
    let mut e = engine_with_the_games_race_menu(b"ShowRaceMenu");
    assert!(e.ext::<crate::State>().gene_projector);
    let record = nvse::table::record_of(&e, SHOW_GENE_PROJECTOR);
    assert_eq!(e.mem.u32(record + 0x18), RACE_MENU_EXECUTE);
    assert_eq!(e.mem.u32(record + 0x14), RACE_MENU_PARAMS);
    assert_eq!(name_of(&e, SHOW_GENE_PROJECTOR), b"TTW_ShowGeneProjector");
    assert_eq!(e.mem.u32(MENU_OPENED), 0);
    let (ok, _) = call(&mut e, SHOW_GENE_PROJECTOR, &[]);
    assert_eq!(ok, 1);
    assert_eq!(e.mem.u32(MENU_OPENED), 1);
    // The game's own record is untouched.
    let game = nvse::table::record_of(&e, RACE_MENU_OPCODE);
    assert_eq!(e.mem.u32(game + 0x18), RACE_MENU_EXECUTE);
    assert_eq!(e.mem.u32(game + 8), RACE_MENU_OPCODE);
}

/// Another command at that opcode is not taken for the race menu.
#[test]
fn show_gene_projector_takes_only_the_command_named_show_race_menu() {
    let e = engine_with_the_games_race_menu(b"ShowBarberMenu");
    let record = nvse::table::record_of(&e, SHOW_GENE_PROJECTOR);
    assert_eq!(e.mem.u32(record + 0x18), 0);
}
