// SPDX-License-Identifier: GPL-3.0-only
//! Tests of the actor and item reads: the records, the limb conditions and their threshold, aiming
//! and blocking through the game's own answers, the consumption sound, other references and the
//! conditions.

use super::*;
use crate::test_support::{call, engine, n, r, set_script_ref, RESULT};
use fork_catalogue::{covers, ids};

const LIMB: u32 = 0x3cc7;
const SOUND: u32 = 0x3cfb;
const AIMING: u32 = 0x3d4a;
const BLOCK_OR_AIM: u32 = 0x3d4b;

/// An actor (type 0x3B), a reference that is not one, an ingestible and its sound, a weapon.
const ACTOR: u32 = 0x0400_0000;
const STATIC: u32 = 0x0400_1000;
const POTION: u32 = 0x0400_2000;
const SOUND_FORM: u32 = 0x0400_3000;
const WEAPON: u32 = 0x0400_4000;
const POTION_REF: u32 = 0x0400_5000;
/// The actor value owner's vtable, its value virtual, and the limb conditions it answers.
const OWNER_VTABLE: u32 = 0x0400_6000;
const OWNER_VALUE: u32 = 0x0400_7000;
const CONDITIONS: u32 = 0x0400_8000;
/// What the game's iron sights and blocked answer for the actor.
const IRON_SIGHTS: u32 = 0x0400_8100;
const BLOCKED: u32 = 0x0400_8104;

fn setup() -> Engine {
    let mut e = engine();
    e.map(0x0400_0000, 0x9000);
    e.mem.set_u8(ACTOR + 4, 0x3b);
    e.mem.set_u8(STATIC + 4, 0x3a);
    e.mem.set_u8(POTION + 4, 0x2f);
    e.mem.set_u32(POTION + 0xc8, SOUND_FORM);
    e.mem.set_u32(SOUND_FORM + 0x0c, 0x0001_2345);
    e.mem.set_u8(WEAPON + 4, 0x28);
    e.mem.set_u32(WEAPON + 0xc8, SOUND_FORM);
    e.mem.set_u8(POTION_REF + 4, 0x3a);
    e.mem.set_u32(POTION_REF + 0x20, POTION);
    // DYNAMIC_CAST (`0x00EC43FB`): a character or creature reference casts to `Actor`.
    e.register(0x00ec_43fb, |e, a| {
        let (form, to) = (a[0], a[3]);
        let actor = to == rtti::ACTOR && matches!(e.mem.u8(form + 4), 0x3b | 0x3c);
        Ret {
            eax: if actor { form } else { 0 },
            ..Ret::default()
        }
    });
    // The owner at +0xA4: its value virtual answers the limb conditions, 100 for any other code.
    e.mem.set_u32(ACTOR + ACTOR_VALUE_OWNER, OWNER_VTABLE);
    e.mem.set_u32(OWNER_VTABLE + OWNER_GET_VALUE, OWNER_VALUE);
    for limb in 0..7 {
        e.mem.set_f32(CONDITIONS + 4 * limb, 100.0);
    }
    e.register_double(OWNER_VALUE, |e, a| {
        assert_eq!(a[0], ACTOR + ACTOR_VALUE_OWNER);
        let st0 = match a[1] {
            0x19..=0x1f => f64::from(e.mem.f32(CONDITIONS + 4 * (a[1] - 0x19))),
            _ => 100.0,
        };
        Ret {
            st0,
            ..Ret::default()
        }
    });
    e.register(ACTOR_GET_IRON_SIGHTS, |e, a| {
        assert_eq!(a[0], ACTOR);
        Ret {
            eax: e.mem.u32(IRON_SIGHTS),
            ..Ret::default()
        }
    });
    e.register(ACTOR_GET_BLOCKED, |e, a| {
        assert_eq!(a[0], ACTOR);
        Ret {
            eax: e.mem.u32(BLOCKED),
            ..Ret::default()
        }
    });
    e
}

fn eval(e: &mut Engine, opcode: u32, this_obj: u32, arg1: u32, arg2: u32) -> f64 {
    let record = nvse::table::record_of(e, opcode);
    let at = e.mem.u32(record + 0x20);
    assert_ne!(at, 0, "{opcode:04x} has a condition");
    e.mem.set_f64(RESULT, -1.0);
    e.call(at, &[this_obj, arg1, arg2, RESULT]);
    e.mem.f64(RESULT)
}

fn condition(e: &mut Engine, limb: u32, value: f32) {
    e.mem.set_f32(CONDITIONS + 4 * limb, value);
}

#[test]
fn records_sit_at_their_opcodes_with_their_names() {
    let e = setup();
    for (opcode, name, needs_ref, cond) in [
        (LIMB, "IsLimbCrippled", true, true),
        (SOUND, "GetIngestibleConsumeSound", false, false),
        (AIMING, "IsAiming", true, true),
        (BLOCK_OR_AIM, "IsBlockingOrAiming", true, true),
    ] {
        let c = COMMANDS.iter().find(|c| c.opcode == opcode).unwrap();
        assert_eq!(c.name, name);
        assert_eq!(c.needs_ref, needs_ref, "{name}");
        assert_eq!(c.eval.is_some(), cond, "{name}");
        assert_ne!(nvse::table::record_of(&e, opcode), 0, "{name}");
    }
}

/// The page's two examples (the head; any limb), each limb by its index, and the brain.
#[test]
fn a_limb_at_or_below_zero_is_crippled() {
    covers(&[ids::extenders::showoff::ISLIMBCRIPPLED]);
    let mut e = setup();
    assert_eq!(call(&mut e, LIMB, ACTOR, &[]), (1, 0.0));
    assert_eq!(call(&mut e, LIMB, ACTOR, &[n(0)]), (1, 0.0));
    for limb in 0..7u32 {
        condition(&mut e, limb, 0.0);
        assert_eq!(
            call(&mut e, LIMB, ACTOR, &[n(limb as i32)]),
            (1, 1.0),
            "{limb}"
        );
        assert_eq!(call(&mut e, LIMB, ACTOR, &[]), (1, 1.0), "any, {limb}");
        assert_eq!(call(&mut e, LIMB, ACTOR, &[n(-1)]), (1, 1.0), "-1, {limb}");
        let other = (limb + 1) % 7;
        assert_eq!(call(&mut e, LIMB, ACTOR, &[n(other as i32)]), (1, 0.0));
        condition(&mut e, limb, 100.0);
    }
    condition(&mut e, 2, -10.0);
    assert_eq!(call(&mut e, LIMB, ACTOR, &[n(2)]), (1, 1.0));
}

/// The threshold moves the line: a limb at 30 is crippled for a threshold of 30, not for 29.
#[test]
fn the_threshold_sets_the_crippled_line() {
    covers(&[ids::extenders::showoff::ISLIMBCRIPPLED]);
    let mut e = setup();
    condition(&mut e, 4, 30.0);
    assert_eq!(call(&mut e, LIMB, ACTOR, &[n(4), n(30)]), (1, 1.0));
    assert_eq!(call(&mut e, LIMB, ACTOR, &[n(4), n(29)]), (1, 0.0));
    assert_eq!(call(&mut e, LIMB, ACTOR, &[n(-1), n(30)]), (1, 1.0));
    assert_eq!(eval(&mut e, LIMB, ACTOR, 4, 30), 1.0);
    assert_eq!(eval(&mut e, LIMB, ACTOR, 4, 29), 0.0);
    assert_eq!(eval(&mut e, LIMB, ACTOR, u32::MAX, 30), 1.0);
}

/// Another reference, no reference or a limb outside -1 to 6: 0.
#[test]
fn a_limb_of_a_non_actor_or_out_of_range_is_not_crippled() {
    covers(&[ids::extenders::showoff::ISLIMBCRIPPLED]);
    let mut e = setup();
    for limb in 0..7 {
        condition(&mut e, limb, 0.0);
    }
    assert_eq!(call(&mut e, LIMB, STATIC, &[]), (1, 0.0));
    assert_eq!(call(&mut e, LIMB, 0, &[]), (1, 0.0));
    assert_eq!(call(&mut e, LIMB, ACTOR, &[n(7)]), (1, 0.0));
    assert_eq!(call(&mut e, LIMB, ACTOR, &[n(-2)]), (1, 0.0));
    assert_eq!(eval(&mut e, LIMB, STATIC, 0, 0), 0.0);
}

/// Aiming is the game's iron sights; blocking adds the game's blocked answer.
#[test]
fn aiming_and_blocking_are_the_games_answers() {
    covers(&[
        ids::extenders::showoff::ISAIMING,
        ids::extenders::showoff::ISBLOCKINGORAIMING,
    ]);
    let mut e = setup();
    for (iron, blocked, aim, either) in [
        (0, 0, 0.0, 0.0),
        (1, 0, 1.0, 1.0),
        (0, 1, 0.0, 1.0),
        (1, 1, 1.0, 1.0),
    ] {
        e.mem.set_u32(IRON_SIGHTS, iron);
        e.mem.set_u32(BLOCKED, blocked);
        assert_eq!(call(&mut e, AIMING, ACTOR, &[]), (1, aim));
        assert_eq!(call(&mut e, BLOCK_OR_AIM, ACTOR, &[]), (1, either));
        assert_eq!(eval(&mut e, AIMING, ACTOR, 0, 0), aim);
        assert_eq!(eval(&mut e, BLOCK_OR_AIM, ACTOR, 0, 0), either);
    }
    // Only the byte of the iron sights answer counts.
    e.mem.set_u32(IRON_SIGHTS, 0x100);
    e.mem.set_u32(BLOCKED, 0);
    assert_eq!(call(&mut e, AIMING, ACTOR, &[]), (1, 0.0));
}

/// Another reference or none is neither aiming nor blocking (the game is not asked).
#[test]
fn a_non_actor_is_not_aiming_or_blocking() {
    covers(&[
        ids::extenders::showoff::ISAIMING,
        ids::extenders::showoff::ISBLOCKINGORAIMING,
    ]);
    let mut e = setup();
    e.mem.set_u32(IRON_SIGHTS, 1);
    e.mem.set_u32(BLOCKED, 1);
    for reference in [STATIC, 0] {
        assert_eq!(call(&mut e, AIMING, reference, &[]), (1, 0.0));
        assert_eq!(call(&mut e, BLOCK_OR_AIM, reference, &[]), (1, 0.0));
        assert_eq!(eval(&mut e, AIMING, reference, 0, 0), 0.0);
        assert_eq!(eval(&mut e, BLOCK_OR_AIM, reference, 0, 0), 0.0);
    }
}

/// The ingestible's sound as a form; a reference stands for its base form.
#[test]
fn an_ingestible_answers_its_consumption_sound() {
    covers(&[ids::extenders::showoff::GETINGESTIBLECONSUMESOUND]);
    let mut e = setup();
    set_script_ref(&mut e, 1, POTION);
    call(&mut e, SOUND, 0, &[r(1)]);
    assert_eq!(e.mem.u32(RESULT), 0x0001_2345);
    call(&mut e, SOUND, POTION_REF, &[]);
    assert_eq!(e.mem.u32(RESULT), 0x0001_2345);
    // No sound.
    e.mem.set_u32(POTION + 0xc8, 0);
    call(&mut e, SOUND, 0, &[r(1)]);
    assert_eq!(e.mem.u32(RESULT), 0);
}

/// Another form, or no form and no reference, answers no form.
#[test]
fn another_form_has_no_consumption_sound() {
    covers(&[ids::extenders::showoff::GETINGESTIBLECONSUMESOUND]);
    let mut e = setup();
    set_script_ref(&mut e, 1, WEAPON);
    call(&mut e, SOUND, 0, &[r(1)]);
    assert_eq!(e.mem.u32(RESULT), 0);
    call(&mut e, SOUND, 0, &[]);
    assert_eq!(e.mem.u32(RESULT), 0);
}
