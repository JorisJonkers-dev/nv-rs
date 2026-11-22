// SPDX-License-Identifier: GPL-3.0-only
//! `SetOwnershipTemp` through its record: the owner given to a cell or to the calling reference,
//! a reference as owner, the player as the default.

use super::*;
use crate::test_support::{call, engine, r, set_script_ref};
use nvse::commands::rtti;

const SET_OWNERSHIP_TEMP: u32 = 0x3d10;
const FACTION: u32 = 0x0300_3000;
const NPC_REF: u32 = 0x0300_3400;
const NPC_BASE: u32 = 0x0300_3800;
const PLAYER_REF: u32 = 0x0300_3c00;
const PLAYER_BASE: u32 = 0x0300_4000;
const CELL: u32 = 0x0300_4400;
const TARGET: u32 = 0x0300_4800;
const NOT_A_CELL: u32 = 0x0300_4c00;
const LOG: u32 = 0x0300_5000;

fn setup() -> Engine {
    let mut e = engine();
    e.map(0x011d_e000, 0x1000);
    // DYNAMIC_CAST: a form of type 0x3a casts to the reference class.
    e.register(0x00ec_43fb, |e, a| {
        let is_ref = a[3] == rtti::TES_OBJECT_REFR && e.mem.u8(a[0] + 4) == 0x3a;
        Ret {
            eax: if is_ref { a[0] } else { 0 },
            ..Ret::default()
        }
    });
    // `ExtraDataList::SetOwner`: logs (list, owner) as the last call.
    e.register(EXTRA_SET_OWNER, |e, a| {
        let n = e.mem.u32(LOG);
        e.mem.set_u32(LOG, n + 1);
        e.mem.set_u32(LOG + 4 + 8 * n, a[0]);
        e.mem.set_u32(LOG + 8 + 8 * n, a[1]);
        Ret::default()
    });
    for (form, kind) in [(NPC_REF, 0x3a), (PLAYER_REF, 0x3a), (CELL, 0x39)] {
        e.mem.set_u8(form + 4, kind);
    }
    e.mem.set_u8(NOT_A_CELL + 4, 0x08);
    e.mem.set_u32(NPC_REF + 0x20, NPC_BASE);
    e.mem.set_u32(PLAYER_REF + 0x20, PLAYER_BASE);
    e.mem.set_u32(PLAYER, PLAYER_REF);
    e
}

/// The (list, owner) pairs the setter was called with.
fn sets(e: &Engine) -> Vec<(u32, u32)> {
    (0..e.mem.u32(LOG))
        .map(|i| (e.mem.u32(LOG + 4 + 8 * i), e.mem.u32(LOG + 8 + 8 * i)))
        .collect()
}

#[test]
fn record_sits_at_the_opcode_with_its_name() {
    let e = setup();
    assert_eq!(COMMANDS[0].opcode, SET_OWNERSHIP_TEMP);
    let record = nvse::table::record_of(&e, SET_OWNERSHIP_TEMP);
    assert_eq!(
        nvse::table::guest_text(&e, e.mem.u32(record)),
        b"SetOwnershipTemp"
    );
}

#[test]
fn a_cell_gets_the_owner_without_a_calling_reference() {
    let mut e = setup();
    set_script_ref(&mut e, 1, FACTION);
    set_script_ref(&mut e, 2, CELL);
    assert_eq!(call(&mut e, SET_OWNERSHIP_TEMP, 0, &[r(1), r(2)]), (1, 1.0));
    assert_eq!(sets(&e), [(CELL + CELL_EXTRA, FACTION)]);
}

#[test]
fn a_reference_as_owner_is_taken_as_its_base_form() {
    let mut e = setup();
    set_script_ref(&mut e, 1, NPC_REF);
    set_script_ref(&mut e, 2, CELL);
    call(&mut e, SET_OWNERSHIP_TEMP, 0, &[r(1), r(2)]);
    assert_eq!(sets(&e), [(CELL + CELL_EXTRA, NPC_BASE)]);
}

#[test]
fn the_player_is_the_default_owner() {
    let mut e = setup();
    call(&mut e, SET_OWNERSHIP_TEMP, TARGET, &[]);
    assert_eq!(sets(&e), [(TARGET + REFR_EXTRA, PLAYER_BASE)]);
}

#[test]
fn without_a_cell_the_calling_reference_is_owned() {
    let mut e = setup();
    set_script_ref(&mut e, 1, FACTION);
    assert_eq!(call(&mut e, SET_OWNERSHIP_TEMP, TARGET, &[r(1)]), (1, 1.0));
    assert_eq!(sets(&e), [(TARGET + REFR_EXTRA, FACTION)]);
}

#[test]
fn nothing_to_own_changes_nothing_and_answers_0() {
    let mut e = setup();
    set_script_ref(&mut e, 1, FACTION);
    assert_eq!(call(&mut e, SET_OWNERSHIP_TEMP, 0, &[r(1)]), (1, 0.0));
    // A second form that is no cell does not take the place of the calling reference either.
    set_script_ref(&mut e, 2, NOT_A_CELL);
    assert_eq!(call(&mut e, SET_OWNERSHIP_TEMP, 0, &[r(1), r(2)]), (1, 0.0));
    assert_eq!(sets(&e), []);
}

#[test]
fn the_name_is_listed_after_the_other_groups() {
    assert_eq!(COMMANDS[0].name, crate::COMMAND_NAMES[15]);
}
