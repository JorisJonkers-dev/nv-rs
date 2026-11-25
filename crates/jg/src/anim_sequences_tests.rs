// SPDX-License-Identifier: GPL-3.0-only
//! `GetSequenceAnimGroup` through its record: an actor whose animation plays a power attack in the
//! weapon slot and idles in slot 0, a slot with no sequence, a slot out of range and a reference
//! with no animation, with a stand-in for the reference virtual `GetAnimData`.

use super::*;
use crate::test_support::{call, engine, n};
use fork_catalogue::{covers, ids};

const GET_SEQUENCE_ANIM_GROUP: u32 = 0x316f;

const ACTOR: u32 = 0x0300_4000;
const NO_ANIMATION: u32 = 0x0300_4100;
const VTABLE: u32 = 0x0300_4200;
const VTABLE_NONE: u32 = 0x0300_4600;
const GET_ANIM_DATA: u32 = 0x0300_4a00;
const GET_NO_ANIM_DATA: u32 = 0x0300_4a10;
const ANIMATION: u32 = 0x0300_5000;
const IDLE_SEQUENCE: u32 = 0x0300_5200;
const WEAPON_SEQUENCE: u32 = 0x0300_5300;
const IDLE_GROUP: u32 = 0x0300_5400;
const ATTACK_GROUP: u32 = 0x0300_5500;

fn setup() -> Engine {
    let mut e = engine();
    e.mem.set_u32(ACTOR, VTABLE);
    e.mem.set_u32(NO_ANIMATION, VTABLE_NONE);
    e.mem.set_u32(VTABLE + REFR_GET_ANIM_DATA, GET_ANIM_DATA);
    e.mem
        .set_u32(VTABLE_NONE + REFR_GET_ANIM_DATA, GET_NO_ANIM_DATA);
    e.register(GET_ANIM_DATA, |_, _| Ret {
        eax: ANIMATION,
        ..Ret::default()
    });
    e.register(GET_NO_ANIM_DATA, |_, _| Ret::default());
    // Slot 0 idles (group 0 of a one-handed weapon: 0x0300), slot 4 plays a power attack
    // (group 99 of a two-handed melee weapon: 0x0463); the other slots play nothing.
    e.mem.set_u32(ANIMATION + CURRENT_SEQUENCES, IDLE_SEQUENCE);
    e.mem
        .set_u32(ANIMATION + CURRENT_SEQUENCES + 4 * 4, WEAPON_SEQUENCE);
    e.mem
        .set_u32(IDLE_SEQUENCE + SEQUENCE_ANIM_GROUP, IDLE_GROUP);
    e.mem
        .set_u32(WEAPON_SEQUENCE + SEQUENCE_ANIM_GROUP, ATTACK_GROUP);
    e.mem.set_u16(IDLE_GROUP + ANIM_GROUP_ID, 0x0300);
    e.mem.set_u16(ATTACK_GROUP + ANIM_GROUP_ID, 0x0463);
    e
}

#[test]
fn the_record_sits_at_its_opcode_with_its_name() {
    let e = setup();
    assert_eq!(COMMANDS[0].opcode, GET_SEQUENCE_ANIM_GROUP);
    assert!(crate::COMMAND_NAMES.contains(&COMMANDS[0].name));
    let record = nvse::table::record_of(&e, GET_SEQUENCE_ANIM_GROUP);
    assert_eq!(
        nvse::table::guest_text(&e, e.mem.u32(record)),
        b"GetSequenceAnimGroup"
    );
}

/// The anim group of the slot's sequence, as the page's anim group list numbers it (the low
/// byte: MigPowerAttack compares the weapon slot's group with 97 to 101).
#[test]
fn a_slot_answers_the_anim_group_of_its_sequence() {
    covers(&[ids::extenders::johnnyguitar::GETSEQUENCEANIMGROUP]);
    let mut e = setup();
    assert_eq!(
        call(&mut e, GET_SEQUENCE_ANIM_GROUP, ACTOR, &[n(4)]),
        (1, 99.0)
    );
    assert_eq!(
        call(&mut e, GET_SEQUENCE_ANIM_GROUP, ACTOR, &[n(0)]),
        (1, 0.0)
    );
}

/// No answer (-1): a slot with no sequence, a slot out of range, a reference with no animation,
/// no reference.
#[test]
fn what_plays_nothing_answers_minus_one() {
    covers(&[ids::extenders::johnnyguitar::GETSEQUENCEANIMGROUP]);
    let mut e = setup();
    assert_eq!(
        call(&mut e, GET_SEQUENCE_ANIM_GROUP, ACTOR, &[n(1)]),
        (1, -1.0)
    );
    assert_eq!(
        call(&mut e, GET_SEQUENCE_ANIM_GROUP, ACTOR, &[n(8)]),
        (1, -1.0)
    );
    assert_eq!(
        call(&mut e, GET_SEQUENCE_ANIM_GROUP, ACTOR, &[n(-1)]),
        (1, -1.0)
    );
    assert_eq!(
        call(&mut e, GET_SEQUENCE_ANIM_GROUP, NO_ANIMATION, &[n(4)]),
        (1, -1.0)
    );
    assert_eq!(call(&mut e, GET_SEQUENCE_ANIM_GROUP, 0, &[n(4)]), (1, -1.0));
}
