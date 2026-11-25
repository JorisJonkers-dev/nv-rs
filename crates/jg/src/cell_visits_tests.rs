// SPDX-License-Identifier: GPL-3.0-only
//! `IsCellVisited` through its record: a cell with seen data, one without, and a form that is not
//! a cell, with a stand-in for the game's `GetSeenData` of a cell.

use super::*;
use crate::test_support::{call, engine, r, set_script_ref};
use fork_catalogue::{covers, ids};

const IS_CELL_VISITED: u32 = 0x3105;

const VISITED: u32 = 0x0300_4000;
const NOT_VISITED: u32 = 0x0300_4400;
const WEAPON: u32 = 0x0300_4800;
const SEEN_DATA: u32 = 0x0300_5000;

fn setup() -> Engine {
    let mut e = engine();
    for cell in [VISITED, NOT_VISITED] {
        e.mem.set_u8(cell + FORM_TYPE, TYPE_CELL);
    }
    e.mem.set_u8(WEAPON + FORM_TYPE, 0x28);
    // The game's GetSeenData: the visited cell has seen data. Asked of any other form it would
    // answer seen data too, so a wrong form type is not hidden by it.
    e.register(CELL_SEEN_DATA, |_, a| Ret {
        eax: if a[0] == NOT_VISITED { 0 } else { SEEN_DATA },
        ..Ret::default()
    });
    set_script_ref(&mut e, 1, VISITED);
    set_script_ref(&mut e, 2, NOT_VISITED);
    set_script_ref(&mut e, 3, WEAPON);
    e
}

#[test]
fn the_record_sits_at_its_opcode_with_its_name() {
    let e = setup();
    assert_eq!(COMMANDS[0].opcode, IS_CELL_VISITED);
    assert!(crate::COMMAND_NAMES.contains(&COMMANDS[0].name));
    let record = nvse::table::record_of(&e, IS_CELL_VISITED);
    assert_eq!(
        nvse::table::guest_text(&e, e.mem.u32(record)),
        b"IsCellVisited"
    );
}

/// The page's example asks one cell: 1 when it has seen data, 0 when not, 0 for a form that is
/// not a cell.
#[test]
fn a_cell_with_seen_data_is_visited() {
    covers(&[ids::extenders::johnnyguitar::ISCELLVISITED]);
    let mut e = setup();
    assert_eq!(call(&mut e, IS_CELL_VISITED, 0, &[r(1)]), (1, 1.0));
    assert_eq!(call(&mut e, IS_CELL_VISITED, 0, &[r(2)]), (1, 0.0));
    assert_eq!(call(&mut e, IS_CELL_VISITED, 0, &[r(3)]), (1, 0.0));
}
