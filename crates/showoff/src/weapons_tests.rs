// SPDX-License-Identifier: GPL-3.0-only
//! The weapon commands through their records: weapons laid out where the stand-in cast puts the
//! weapon class, a reference to one, and an actor wearing one (container changes laid out as
//! `crates/nvse`'s inventory tests lay them, `GetByType` stood in for).

use super::*;
use crate::test_support::{call, engine, n, r, set_script_ref, RESULT};
use fork_catalogue::{covers, ids};

const IS_WEAPON_MELEE: u32 = 0x3c9d;
const IS_EQUIPPED_WEAPON_MELEE: u32 = 0x3c9e;
const IS_WEAPON_RANGED: u32 = 0x3c9f;
const GET_WEAPON_HAS_FLAG: u32 = 0x3cdf;
const SET_WEAPON_FLAG: u32 = 0x3d0f;

const FIST: u32 = 0x0300_3000;
const RIFLE: u32 = 0x0300_3400;
const ARMOR: u32 = 0x0300_3800;
const REFERENCE: u32 = 0x0300_3c00;
const ACTOR: u32 = 0x0300_4000;
const CHANGES: u32 = 0x0300_4400;
const XDATA: u32 = 0x0300_4800;
const NODES: u32 = 0x0300_5000;

fn put_extra(e: &mut Engine, list: u32, kind: u32, data: u32) {
    let at = list + 8 + (kind >> 3);
    let bits = e.mem.u8(at) | (1 << (kind & 7));
    e.mem.set_u8(at, bits);
    e.mem.set_u32(list + 0x100 + 4 * kind, data);
}

fn setup() -> Engine {
    let mut e = engine();
    // DYNAMIC_CAST: type 0x28 casts to the weapon class, 0x3a to the reference class.
    e.register(0x00ec_43fb, |e, a| {
        let (form, to) = (a[0], a[3]);
        let kind = e.mem.u8(form + 4);
        let ok = (to == rtti::TES_OBJECT_WEAP && kind == 0x28)
            || (to == rtti::TES_OBJECT_REFR && kind == 0x3a);
        Ret {
            eax: if ok { form } else { 0 },
            ..Ret::default()
        }
    });
    // `BaseExtraList::GetByType`.
    e.register(0x0041_0220, |e, a| Ret {
        eax: e.mem.u32(a[0] + 0x100 + 4 * a[1]),
        ..Ret::default()
    });
    for (form, kind, weapon_type) in [(FIST, 0x28, 1u8), (RIFLE, 0x28, 5), (ARMOR, 0x18, 1)] {
        e.mem.set_u8(form + 4, kind);
        e.mem.set_u8(form + WEAPON_TYPE, weapon_type);
    }
    e.mem.set_u8(REFERENCE + 4, 0x3a);
    e.mem.set_u32(REFERENCE + 0x20, RIFLE);
    e
}

/// The actor wears `weapon` (the worn bit on its stack's extra data list).
fn wear(e: &mut Engine, weapon: u32) {
    e.mem.set_u8(ACTOR + 4, 0x3b);
    put_extra(e, ACTOR + 0x44, 0x15, CHANGES);
    e.mem.set_u32(CHANGES + 0xc, CHANGES + 0x20);
    e.mem.set_u32(CHANGES + 0x20, NODES);
    let entry = NODES + 0x100;
    e.mem.set_u32(NODES, entry);
    e.mem.set_u32(entry, NODES + 0x40);
    e.mem.set_u32(entry + 8, weapon);
    e.mem.set_u32(NODES + 0x40, XDATA);
    put_extra(e, XDATA, 0x16, 0);
}

fn eval(e: &mut Engine, opcode: u32, this_obj: u32, arg1: u32, arg2: u32) -> f64 {
    let record = nvse::table::record_of(e, opcode);
    let eval = e.mem.u32(record + 0x20);
    assert_ne!(eval, 0);
    e.mem.set_f64(RESULT, 99.0);
    e.call(eval, &[this_obj, arg1, arg2, RESULT]);
    e.mem.f64(RESULT)
}

#[test]
fn records_sit_at_the_opcodes_with_their_names() {
    let e = setup();
    for (c, opcode) in COMMANDS.iter().zip([
        IS_WEAPON_MELEE,
        IS_WEAPON_RANGED,
        IS_EQUIPPED_WEAPON_MELEE,
        GET_WEAPON_HAS_FLAG,
        SET_WEAPON_FLAG,
    ]) {
        assert_eq!(c.opcode, opcode, "{}", c.name);
        let record = nvse::table::record_of(&e, opcode);
        assert_eq!(
            nvse::table::guest_text(&e, e.mem.u32(record)),
            c.name.as_bytes()
        );
        assert_eq!(
            nvse::table::guest_text(&e, e.mem.u32(record + 4)),
            c.short.as_bytes()
        );
        assert_eq!(
            e.mem.u32(record + 0x20) != 0,
            c.eval.is_some(),
            "{}",
            c.name
        );
    }
}

#[test]
fn the_names_are_listed_after_the_other_groups() {
    let names: Vec<&str> = COMMANDS.iter().map(|c| c.name).collect();
    assert_eq!(names, crate::COMMAND_NAMES[10..15]);
}

/// The type decides: 1 is melee, 5 ranged; a form that is not a weapon is neither; a reference is
/// its base form; no argument asks the calling reference.
#[test]
fn melee_and_ranged_follow_the_weapon_type() {
    covers(&[
        ids::extenders::showoff::ISWEAPONMELEE,
        ids::extenders::showoff::ISWEAPONRANGED,
    ]);
    let mut e = setup();
    for (i, form) in [FIST, RIFLE, ARMOR, REFERENCE].into_iter().enumerate() {
        set_script_ref(&mut e, i as u16 + 1, form);
    }
    let ask = |e: &mut Engine, op, arg: u16| call(e, op, 0, &[r(arg)]);
    assert_eq!(ask(&mut e, IS_WEAPON_MELEE, 1), (1, 1.0));
    assert_eq!(ask(&mut e, IS_WEAPON_MELEE, 2), (1, 0.0));
    assert_eq!(ask(&mut e, IS_WEAPON_MELEE, 3), (1, 0.0));
    assert_eq!(ask(&mut e, IS_WEAPON_RANGED, 1), (1, 0.0));
    assert_eq!(ask(&mut e, IS_WEAPON_RANGED, 2), (1, 1.0));
    assert_eq!(ask(&mut e, IS_WEAPON_RANGED, 3), (1, 0.0));
    assert_eq!(
        ask(&mut e, IS_WEAPON_RANGED, 4),
        (1, 1.0),
        "a reference's base"
    );
    assert_eq!(call(&mut e, IS_WEAPON_RANGED, REFERENCE, &[]), (1, 1.0));
    assert_eq!(call(&mut e, IS_WEAPON_MELEE, 0, &[]), (1, 0.0));
    // The boundaries: 2 is melee, 3 and 13 ranged, 14 neither.
    for (t, melee, ranged) in [(2, 1.0, 0.0), (3, 0.0, 1.0), (13, 0.0, 1.0), (14, 0.0, 0.0)] {
        e.mem.set_u8(RIFLE + WEAPON_TYPE, t);
        assert_eq!(ask(&mut e, IS_WEAPON_MELEE, 2).1, melee, "{t}");
        assert_eq!(ask(&mut e, IS_WEAPON_RANGED, 2).1, ranged, "{t}");
    }
    // As conditions.
    assert_eq!(eval(&mut e, IS_WEAPON_MELEE, 0, FIST, 0), 1.0);
    assert_eq!(eval(&mut e, IS_WEAPON_RANGED, REFERENCE, 0, 0), 0.0);
    assert_eq!(eval(&mut e, IS_WEAPON_RANGED, 0, 0, 0), 0.0);
}

/// The equipped weapon decides, and with none equipped the answer is 1 (the page's fists).
#[test]
fn an_equipped_weapon_is_asked_and_none_means_melee() {
    covers(&[ids::extenders::showoff::ISEQUIPPEDWEAPONMELEE]);
    let mut e = setup();
    assert_eq!(call(&mut e, IS_EQUIPPED_WEAPON_MELEE, ACTOR, &[]), (1, 1.0));
    wear(&mut e, RIFLE);
    assert_eq!(call(&mut e, IS_EQUIPPED_WEAPON_MELEE, ACTOR, &[]), (1, 0.0));
    assert_eq!(eval(&mut e, IS_EQUIPPED_WEAPON_MELEE, ACTOR, 0, 0), 0.0);
    e.mem.set_u32(NODES + 0x108, FIST);
    assert_eq!(call(&mut e, IS_EQUIPPED_WEAPON_MELEE, ACTOR, &[]), (1, 1.0));
    assert_eq!(eval(&mut e, IS_EQUIPPED_WEAPON_MELEE, ACTOR, 0, 0), 1.0);
    // Without a calling reference there is no actor to ask.
    assert_eq!(call(&mut e, IS_EQUIPPED_WEAPON_MELEE, 0, &[]).1, 0.0);
}

/// Indices 0-7 are the first flags' bits and 8-21 the second flags' bits; 22 and up, and a
/// negative index, are no flag.
#[test]
fn flags_are_read_by_index_over_both_flag_words() {
    covers(&[ids::extenders::showoff::GETWEAPONHASFLAG]);
    let mut e = setup();
    set_script_ref(&mut e, 1, RIFLE);
    set_script_ref(&mut e, 2, ARMOR);
    e.mem.set_u8(RIFLE + FLAGS1, 0b0000_0110);
    e.mem.set_u32(RIFLE + FLAGS2, (1 << 0) | (1 << 13));
    let has = |e: &mut Engine, i: i32| call(e, GET_WEAPON_HAS_FLAG, 0, &[n(i), r(1)]).1;
    let on: Vec<i32> = (-1..24).filter(|&i| has(&mut e, i) == 1.0).collect();
    assert_eq!(on, [1, 2, 8, 21]);
    // The calling reference's base form without the argument; a non-weapon has no flags.
    assert_eq!(call(&mut e, GET_WEAPON_HAS_FLAG, REFERENCE, &[n(1)]).1, 1.0);
    assert_eq!(call(&mut e, GET_WEAPON_HAS_FLAG, 0, &[n(1), r(2)]).1, 0.0);
    assert_eq!(eval(&mut e, GET_WEAPON_HAS_FLAG, 0, 21, RIFLE), 1.0);
    assert_eq!(eval(&mut e, GET_WEAPON_HAS_FLAG, REFERENCE, 2, 0), 1.0);
    assert_eq!(eval(&mut e, GET_WEAPON_HAS_FLAG, 0, 3, RIFLE), 0.0);
}

/// Setting turns one flag on or off and nothing else; it answers 1, or 0 for a bad index or a
/// form that is not a weapon.
#[test]
fn a_flag_is_set_and_cleared_by_index() {
    covers(&[ids::extenders::showoff::SETWEAPONFLAG]);
    let mut e = setup();
    set_script_ref(&mut e, 1, RIFLE);
    set_script_ref(&mut e, 2, ARMOR);
    e.mem.set_u8(RIFLE + FLAGS1, 0b0000_0010);
    e.mem.set_u32(RIFLE + FLAGS2, 1 << 5);
    assert_eq!(
        call(&mut e, SET_WEAPON_FLAG, 0, &[n(7), n(1), r(1)]),
        (1, 1.0)
    );
    assert_eq!(e.mem.u8(RIFLE + FLAGS1), 0b1000_0010);
    assert_eq!(call(&mut e, SET_WEAPON_FLAG, 0, &[n(1), n(0), r(1)]).1, 1.0);
    assert_eq!(e.mem.u8(RIFLE + FLAGS1), 0b1000_0000);
    assert_eq!(
        call(&mut e, SET_WEAPON_FLAG, 0, &[n(21), n(5), r(1)]).1,
        1.0
    );
    assert_eq!(e.mem.u32(RIFLE + FLAGS2), (1 << 5) | (1 << 13));
    assert_eq!(
        call(&mut e, SET_WEAPON_FLAG, 0, &[n(13), n(0), r(1)]).1,
        1.0
    );
    assert_eq!(e.mem.u32(RIFLE + FLAGS2), 1 << 13);
    // The calling reference's base form without the argument.
    assert_eq!(
        call(&mut e, SET_WEAPON_FLAG, REFERENCE, &[n(0), n(1)]).1,
        1.0
    );
    assert_eq!(e.mem.u8(RIFLE + FLAGS1), 0b1000_0001);
    // Failures change nothing.
    for args in [[n(22), n(1), r(1)], [n(-1), n(1), r(1)], [n(1), n(1), r(2)]] {
        assert_eq!(call(&mut e, SET_WEAPON_FLAG, 0, &args), (1, 0.0));
    }
    assert_eq!(call(&mut e, SET_WEAPON_FLAG, 0, &[n(1), n(1)]).1, 0.0);
    assert_eq!(e.mem.u8(RIFLE + FLAGS1), 0b1000_0001);
    assert_eq!(e.mem.u32(RIFLE + FLAGS2), 1 << 13);
}
