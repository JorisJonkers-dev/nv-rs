// SPDX-License-Identifier: GPL-3.0-only
//! Tests of the game-state reads: the records, the Pip-Boy word, the thresholded value and the
//! numbered actor value on an actor, another reference and codes out of range, and the two
//! conditions.

use super::*;
use crate::test_support::{call, engine, RESULT};
use fork_catalogue::{covers, ids};

const PIP_BOY: u32 = 0x3168;
const AV_ALT: u32 = 0x3174;
const THRESHOLD: u32 = 0x31a7;

/// The interface manager, an actor (type 0x3B, a character) and a reference that is not one.
const MANAGER: u32 = 0x0400_0000;
const ACTOR: u32 = 0x0400_1000;
const STATIC: u32 = 0x0400_2000;
/// Where the stand-ins write what they were called with.
const SEEN: u32 = 0x0400_3000;

fn setup() -> Engine {
    let mut e = engine();
    e.map(0x011d_8000, 0x1000);
    e.map(MANAGER, 0x4000);
    e.mem.set_u32(INTERFACE_MANAGER, MANAGER);
    e.mem.set_u8(ACTOR + 4, 0x3b);
    e.mem.set_u8(STATIC + 4, 0x3a);
    // DYNAMIC_CAST (`0x00EC43FB`): a character or creature reference casts to `Actor`.
    e.register(0x00ec_43fb, |e, a| {
        let (form, to) = (a[0], a[3]);
        let actor = to == rtti::ACTOR && matches!(e.mem.u8(form + 4), 0x3b | 0x3c);
        Ret {
            eax: if actor { form } else { 0 },
            ..Ret::default()
        }
    });
    // The clamped value: 10 plus the code, and the owner and code it was asked with.
    e.register_double(GET_CLAMPED_ACTOR_FLOAT_VALUE, |e, a| {
        e.mem.set_u32(SEEN, a[0]);
        e.mem.set_u32(SEEN + 4, a[1]);
        Ret {
            st0: 10.0 + f64::from(a[1]),
            ..Ret::default()
        }
    });
    // The game's GetActorValue condition: 100 plus the code for the actor, nothing written for
    // another reference (as the game's own).
    e.register(GET_ACTOR_VALUE_CONDITION, |e, a| {
        e.mem.set_u32(SEEN + 8, a[1]);
        if a[0] == ACTOR {
            e.mem.set_f64(a[3], 100.0 + f64::from(a[1]));
        }
        Ret {
            eax: 1,
            ..Ret::default()
        }
    });
    e
}

/// A compiled actor value argument: its 16-bit code.
fn av(code: u16) -> Vec<u8> {
    code.to_le_bytes().to_vec()
}

/// A compiled integer.
fn n(v: i32) -> Vec<u8> {
    let mut b = vec![b'n'];
    b.extend_from_slice(&v.to_le_bytes());
    b
}

/// A command's condition eval with `this_obj` and the first parameter: its result.
fn eval(e: &mut Engine, opcode: u32, this_obj: u32, arg1: u32) -> f64 {
    let record = nvse::table::record_of(e, opcode);
    let eval = e.mem.u32(record + 0x20);
    assert_ne!(eval, 0, "{opcode:04x} has a condition");
    e.mem.set_f64(RESULT, -1.0);
    e.call(eval, &[this_obj, arg1, 0, RESULT]);
    e.mem.f64(RESULT)
}

#[test]
fn records_sit_at_their_opcodes_with_their_names() {
    let e = setup();
    for (opcode, name) in [
        (PIP_BOY, "GetPipBoyMode"),
        (AV_ALT, "GetActorValueAlt"),
        (THRESHOLD, "GetThresholdedActorValue"),
    ] {
        let c = COMMANDS.iter().find(|c| c.opcode == opcode).unwrap();
        assert_eq!(c.name, name);
        assert_ne!(nvse::table::record_of(&e, opcode), 0, "{name}");
    }
    let by = |name: &str| COMMANDS.iter().find(|c| c.name == name).unwrap();
    assert!(!by("GetPipBoyMode").needs_ref);
    assert!(by("GetActorValueAlt").needs_ref);
    assert!(by("GetThresholdedActorValue").needs_ref);
    assert_eq!(by("GetActorValueAlt").short, "GetAVAlt");
    assert_eq!(by("GetThresholdedActorValue").short, "GetThresholdAV");
    assert!(by("GetActorValueAlt").eval.is_none());
}

/// The page's six states, read from the game's word; no interface manager is closed.
#[test]
fn the_pip_boy_mode_is_the_games_state_word() {
    covers(&[ids::extenders::johnnyguitar::GETPIPBOYMODE]);
    let mut e = setup();
    for mode in 0..=5u32 {
        e.mem.set_u32(MANAGER + PIP_BOY_MODE, mode);
        assert_eq!(call(&mut e, PIP_BOY, 0, &[]), (1, f64::from(mode)));
        assert_eq!(eval(&mut e, PIP_BOY, 0, 0), f64::from(mode));
    }
    e.mem.set_u32(INTERFACE_MANAGER, 0);
    assert_eq!(call(&mut e, PIP_BOY, 0, &[]), (1, 0.0));
    assert_eq!(eval(&mut e, PIP_BOY, 0, 0), 0.0);
}

/// The actor's owner at +0xA4 is asked for the clamped value of the code given.
#[test]
fn the_thresholded_value_is_the_games_clamped_value() {
    covers(&[ids::extenders::johnnyguitar::GETTHRESHOLDEDACTORVALUE]);
    let mut e = setup();
    // Agility (5) and Barter (32), as the page's and the profile's calls.
    assert_eq!(call(&mut e, THRESHOLD, ACTOR, &[av(5)]), (1, 15.0));
    assert_eq!(e.mem.u32(SEEN), ACTOR + ACTOR_VALUE_OWNER);
    assert_eq!(e.mem.u32(SEEN + 4), 5);
    assert_eq!(call(&mut e, THRESHOLD, ACTOR, &[av(32)]), (1, 42.0));
    assert_eq!(eval(&mut e, THRESHOLD, ACTOR, 76), 86.0);
}

/// Another reference, no reference or a code past the last actor value: 0, and the game is not
/// asked.
#[test]
fn a_thresholded_value_of_a_non_actor_or_bad_code_is_zero() {
    covers(&[ids::extenders::johnnyguitar::GETTHRESHOLDEDACTORVALUE]);
    let mut e = setup();
    e.mem.set_u32(SEEN, 0);
    assert_eq!(call(&mut e, THRESHOLD, STATIC, &[av(5)]), (1, 0.0));
    assert_eq!(call(&mut e, THRESHOLD, 0, &[av(5)]), (1, 0.0));
    assert_eq!(call(&mut e, THRESHOLD, ACTOR, &[av(77)]), (1, 0.0));
    assert_eq!(eval(&mut e, THRESHOLD, STATIC, 5), 0.0);
    assert_eq!(eval(&mut e, THRESHOLD, ACTOR, 0xffff), 0.0);
    assert_eq!(e.mem.u32(SEEN), 0);
}

/// The number goes to the game's GetActorValue as the name would.
#[test]
fn the_actor_value_alt_is_the_games_actor_value_by_number() {
    covers(&[ids::extenders::johnnyguitar::GETACTORVALUEALT]);
    let mut e = setup();
    // The page's example, 16.
    assert_eq!(call(&mut e, AV_ALT, ACTOR, &[n(16)]), (1, 116.0));
    assert_eq!(e.mem.u32(SEEN + 8), 16);
    assert_eq!(call(&mut e, AV_ALT, ACTOR, &[n(0)]), (1, 100.0));
    assert_eq!(call(&mut e, AV_ALT, ACTOR, &[n(76)]), (1, 176.0));
}

/// Another reference, no reference, a negative number or one past the last answers 0 without
/// asking the game.
#[test]
fn an_actor_value_alt_of_a_non_actor_or_bad_code_is_zero() {
    covers(&[ids::extenders::johnnyguitar::GETACTORVALUEALT]);
    let mut e = setup();
    e.mem.set_u32(SEEN + 8, 0xdead);
    assert_eq!(call(&mut e, AV_ALT, STATIC, &[n(16)]), (1, 0.0));
    assert_eq!(call(&mut e, AV_ALT, 0, &[n(16)]), (1, 0.0));
    assert_eq!(call(&mut e, AV_ALT, ACTOR, &[n(77)]), (1, 0.0));
    assert_eq!(call(&mut e, AV_ALT, ACTOR, &[n(-1)]), (1, 0.0));
    assert_eq!(e.mem.u32(SEEN + 8), 0xdead);
}
