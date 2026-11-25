// SPDX-License-Identifier: GPL-3.0-only
//! The form traits through their records: a base effect's codes, every interior lighting trait
//! read and written, every V.A.T.S. trait read and written, and what a wrong form or trait
//! answers.

use super::*;
use crate::test_support::{call, engine, n, r, set_script_ref, z};
use fork_catalogue::{covers, ids};

const GET_AV: u32 = 0x3103;
const GET_ARCHETYPE: u32 = 0x3104;
const GET_LIGHTING: u32 = 0x310b;
const SET_LIGHTING: u32 = 0x310c;
const GET_VATS: u32 = 0x3169;
const SET_VATS: u32 = 0x316a;

const EFFECT: u32 = 0x0300_4000;
const CELL: u32 = 0x0300_4400;
const INTERIOR: u32 = 0x0300_4800;
const EXTERIOR_CELL: u32 = 0x0300_4c00;
const WEAPON: u32 = 0x0300_5000;

fn setup() -> Engine {
    let mut e = engine();
    e.mem.set_u8(EFFECT + FORM_TYPE, TYPE_EFFECT_SETTING);
    e.mem.set_u32(EFFECT + ARCHETYPE, 34);
    e.mem.set_u32(EFFECT + ACTOR_VALUE, 15);
    for cell in [CELL, EXTERIOR_CELL] {
        e.mem.set_u8(cell + FORM_TYPE, TYPE_CELL);
        e.mem.set_u32(cell + CELL_DATA, INTERIOR);
    }
    e.mem.set_u8(CELL + CELL_FLAGS, 1);
    // Ambient 10 20 30, directional 40 50 60, fog 70 80 90 (red is the low byte).
    e.mem.set_u32(INTERIOR, 0x001e_140a);
    e.mem.set_u32(INTERIOR + 4, 0x003c_3228);
    e.mem.set_u32(INTERIOR + 8, 0x005a_5046);
    e.mem.set_f32(INTERIOR + 0x0c, 64.0);
    e.mem.set_f32(INTERIOR + 0x10, 4000.0);
    e.mem.set_u32(INTERIOR + 0x14, (-45i32) as u32);
    e.mem.set_u32(INTERIOR + 0x18, 120);
    e.mem.set_f32(INTERIOR + 0x1c, 0.5);
    e.mem.set_f32(INTERIOR + 0x20, 6000.0);
    e.mem.set_f32(INTERIOR + 0x24, 1.5);
    e.mem.set_u8(WEAPON + FORM_TYPE, TYPE_WEAPON);
    let vats = WEAPON + WEAPON_VATS;
    e.mem.set_f32(vats + 4, 25.0);
    e.mem.set_f32(vats + 8, 2.0);
    e.mem.set_f32(vats + 0xc, 30.0);
    e.mem.set_u8(vats + 0x10, 1);
    e.mem.set_u8(vats + 0x11, 0);
    set_script_ref(&mut e, 1, EFFECT);
    set_script_ref(&mut e, 2, CELL);
    set_script_ref(&mut e, 3, WEAPON);
    set_script_ref(&mut e, 4, EXTERIOR_CELL);
    e
}

fn run(e: &mut Engine, opcode: u32, args: &[Vec<u8>]) -> f64 {
    let (ok, v) = call(e, opcode, 0, args);
    assert_eq!(ok, 1);
    v
}

fn lighting(e: &mut Engine, cell: u16, id: i32) -> f64 {
    run(e, GET_LIGHTING, &[r(cell), n(id)])
}

fn vats(e: &mut Engine, id: i32) -> f64 {
    run(e, GET_VATS, &[r(3), n(id)])
}

#[test]
fn records_sit_at_their_opcodes_with_their_names() {
    let e = setup();
    let opcodes: Vec<u32> = COMMANDS.iter().map(|c| c.opcode).collect();
    assert_eq!(
        opcodes,
        [
            GET_AV,
            GET_ARCHETYPE,
            GET_LIGHTING,
            SET_LIGHTING,
            GET_VATS,
            SET_VATS
        ]
    );
    let names: Vec<&str> = COMMANDS.iter().map(|c| c.name).collect();
    assert_eq!(names, crate::COMMAND_NAMES[11..17]);
    for c in &COMMANDS {
        let record = nvse::table::record_of(&e, c.opcode);
        assert_eq!(
            nvse::table::guest_text(&e, e.mem.u32(record)),
            c.name.as_bytes()
        );
    }
}

/// The page's examples: a value modifier's actor value (15) and an archetype code.
#[test]
fn a_base_effect_answers_its_actor_value_and_archetype() {
    covers(&[
        ids::extenders::johnnyguitar::GETBASEEFFECTAV,
        ids::extenders::johnnyguitar::GETBASEEFFECTARCHETYPE,
    ]);
    let mut e = setup();
    assert_eq!(run(&mut e, GET_AV, &[r(1)]), 15.0);
    assert_eq!(run(&mut e, GET_ARCHETYPE, &[r(1)]), 34.0);
    // No actor value is -1 in the record.
    e.mem.set_u32(EFFECT + ACTOR_VALUE, u32::MAX);
    assert_eq!(run(&mut e, GET_AV, &[r(1)]), -1.0);
    // Not a base effect.
    assert_eq!(run(&mut e, GET_AV, &[r(3)]), 0.0);
    assert_eq!(run(&mut e, GET_ARCHETYPE, &[r(2)]), 0.0);
}

#[test]
fn every_interior_lighting_trait_reads_its_place() {
    covers(&[ids::extenders::johnnyguitar::GETINTERIORLIGHTINGTRAITNUMERIC]);
    let mut e = setup();
    let want = [
        10.0, 20.0, 30.0, 40.0, 50.0, 60.0, -45.0, 120.0, 0.5, 70.0, 80.0, 90.0, 64.0, 4000.0, 1.5,
        6000.0,
    ];
    for (id, v) in want.into_iter().enumerate() {
        assert_eq!(lighting(&mut e, 2, id as i32), v, "trait {id}");
    }
}

#[test]
fn every_interior_lighting_trait_is_written_in_its_place() {
    covers(&[
        ids::extenders::johnnyguitar::SETINTERIORLIGHTINGTRAITNUMERIC,
        ids::extenders::johnnyguitar::GETINTERIORLIGHTINGTRAITNUMERIC,
    ]);
    let mut e = setup();
    let values = [
        1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.5, 9.5, 11.0, 12.0, 13.0, 14.5, 15.5, 16.5, 17.5,
    ];
    for (id, v) in values.into_iter().enumerate() {
        run(&mut e, SET_LIGHTING, &[r(2), n(id as i32), z(v)]);
    }
    let read: Vec<f64> = (0..16).map(|id| lighting(&mut e, 2, id)).collect();
    // The rotation Z (7) is a whole number.
    let mut want = values.to_vec();
    want[7] = 8.0;
    assert_eq!(read, want);
    // The ambient colour's fourth byte is untouched.
    assert_eq!(e.mem.u32(INTERIOR), 0x0003_0201);
}

/// A colour byte takes the whole part held to 0 to 255.
#[test]
fn a_colour_trait_is_held_to_a_byte() {
    covers(&[ids::extenders::johnnyguitar::SETINTERIORLIGHTINGTRAITNUMERIC]);
    let mut e = setup();
    run(&mut e, SET_LIGHTING, &[r(2), n(0), z(300.0)]);
    run(&mut e, SET_LIGHTING, &[r(2), n(1), z(-5.0)]);
    run(&mut e, SET_LIGHTING, &[r(2), n(2), z(35.9)]);
    assert_eq!(lighting(&mut e, 2, 0), 255.0);
    assert_eq!(lighting(&mut e, 2, 1), 0.0);
    assert_eq!(lighting(&mut e, 2, 2), 35.0);
}

/// An exterior cell, another form or a trait outside 0 to 15 answers 0 and writes nothing.
#[test]
fn a_wrong_cell_form_or_trait_answers_zero_and_changes_nothing() {
    covers(&[
        ids::extenders::johnnyguitar::GETINTERIORLIGHTINGTRAITNUMERIC,
        ids::extenders::johnnyguitar::SETINTERIORLIGHTINGTRAITNUMERIC,
    ]);
    let mut e = setup();
    let before: Vec<u8> = (0..0x28).map(|i| e.mem.u8(INTERIOR + i)).collect();
    assert_eq!(lighting(&mut e, 4, 0), 0.0);
    assert_eq!(lighting(&mut e, 3, 0), 0.0);
    assert_eq!(lighting(&mut e, 2, 16), 0.0);
    assert_eq!(lighting(&mut e, 2, -1), 0.0);
    run(&mut e, SET_LIGHTING, &[r(4), n(0), z(99.0)]);
    run(&mut e, SET_LIGHTING, &[r(2), n(16), z(99.0)]);
    let after: Vec<u8> = (0..0x28).map(|i| e.mem.u8(INTERIOR + i)).collect();
    assert_eq!(before, after);
}

#[test]
fn every_vats_trait_reads_and_writes_its_place() {
    covers(&[
        ids::extenders::johnnyguitar::GETWEAPONVATSTRAITNUMERIC,
        ids::extenders::johnnyguitar::SETWEAPONVATSTRAITNUMERIC,
    ]);
    let mut e = setup();
    let read: Vec<f64> = (1..=5).map(|id| vats(&mut e, id)).collect();
    assert_eq!(read, [25.0, 2.0, 30.0, 1.0, 0.0]);
    for (id, v) in [(1, 50.0), (2, 1.5), (3, 20.0), (4, 0.0), (5, 7.0)] {
        run(&mut e, SET_VATS, &[r(3), n(id), z(v)]);
    }
    let read: Vec<f64> = (1..=5).map(|id| vats(&mut e, id)).collect();
    // The two flags are 0 or 1.
    assert_eq!(read, [50.0, 1.5, 20.0, 0.0, 1.0]);
    assert_eq!(e.mem.u8(WEAPON + WEAPON_VATS + 0x11), 1);
}

#[test]
fn a_vats_trait_of_another_form_or_out_of_range_answers_zero() {
    covers(&[
        ids::extenders::johnnyguitar::GETWEAPONVATSTRAITNUMERIC,
        ids::extenders::johnnyguitar::SETWEAPONVATSTRAITNUMERIC,
    ]);
    let mut e = setup();
    assert_eq!(vats(&mut e, 0), 0.0);
    assert_eq!(vats(&mut e, 6), 0.0);
    assert_eq!(run(&mut e, GET_VATS, &[r(1), n(1)]), 0.0);
    run(&mut e, SET_VATS, &[r(3), n(0), z(9.0)]);
    run(&mut e, SET_VATS, &[r(1), n(1), z(9.0)]);
    assert_eq!(vats(&mut e, 1), 25.0);
    assert_eq!(e.mem.u32(WEAPON + WEAPON_VATS), 0);
}
