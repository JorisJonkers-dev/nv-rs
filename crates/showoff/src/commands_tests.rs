// SPDX-License-Identifier: GPL-3.0-only
//! Tests of `ActorHasBaseFlag`: the registration, the bits of an actor base's flags word, a
//! reference without an actor base, bits out of range, and the condition.

use super::*;
use crate::identity::OPCODE_BASE;

const DATA: u32 = 0x0300_0000;
const OFFSET: u32 = 0x0300_0f00;
const RESULT: u32 = 0x0300_0f08;
const SCRIPT: u32 = 0x0300_1000;
const LIST: u32 = 0x0300_2000;
/// A reference, its actor base, and a base that is not one.
const REFR: u32 = 0x0400_0000;
const ACTOR: u32 = 0x0400_1000;
const OTHER: u32 = 0x0400_2000;

fn engine(flags: u32) -> Engine {
    let mut e = Engine::new();
    e.map(0x0118_e000, 0x9000);
    e.map(0x011d_e000, 0x1000);
    e.map(DATA, 0x1_0000);
    e.map(REFR, 0x3000);
    nvse::table::install(&mut e);
    // DYNAMIC_CAST (`0x00EC43FB`) stood in for: a form of type 0x2A (character) or 0x2B
    // (creature) casts to `TESActorBase`, any other form does not.
    e.register(0x00ec_43fb, |e, a| {
        let (form, to) = (a[0], a[3]);
        let actor = to == rtti::TES_ACTOR_BASE && matches!(e.mem.u8(form + 4), 0x2a | 0x2b);
        Ret {
            eax: if actor { form } else { 0 },
            ..Ret::default()
        }
    });
    e.mem.set_u8(ACTOR + 4, 0x2a);
    e.mem.set_u32(ACTOR + ACTOR_BASE_FLAGS, flags);
    e.mem.set_u8(OTHER + 4, 0x28);
    e.mem.set_u32(OTHER + ACTOR_BASE_FLAGS, u32::MAX);
    e.mem.set_u32(REFR + REFR_BASE_FORM, ACTOR);
    crate::install_as(&mut e, true);
    e
}

/// `thisObj.ActorHasBaseFlag bit` through the record's execute: the handler's answer and result.
fn run(e: &mut Engine, this_obj: u32, bit: i32) -> (u32, f64) {
    e.mem.set_u16(DATA, ACTOR_HAS_BASE_FLAG as u16);
    e.mem.set_u16(DATA + 2, 7);
    e.mem.set_u16(DATA + 4, 1);
    e.mem.set_u8(DATA + 6, b'n');
    for (i, b) in bit.to_le_bytes().iter().enumerate() {
        e.mem.set_u8(DATA + 7 + i as u32, *b);
    }
    e.mem.set_u32(OFFSET, 4);
    e.mem.set_f64(RESULT, -1.0);
    let record = nvse::table::record_of(e, ACTOR_HAS_BASE_FLAG);
    let params = e.mem.u32(record + 0x14);
    let execute = e.mem.u32(record + 0x18);
    let ok =
        e.call(
            execute,
            &[params, DATA, this_obj, 0, SCRIPT, LIST, RESULT, OFFSET],
        )
        .eax & 0xff;
    (ok, e.mem.f64(RESULT))
}

/// The condition's eval: the result.
fn eval(e: &mut Engine, this_obj: u32, bit: u32) -> f64 {
    e.mem.set_f64(RESULT, -1.0);
    let record = nvse::table::record_of(e, ACTOR_HAS_BASE_FLAG);
    let eval = e.mem.u32(record + 0x20);
    assert_ne!(eval, 0);
    let ok = e.call(eval, &[this_obj, bit, 0, RESULT]).eax & 0xff;
    assert_eq!(ok, 1);
    e.mem.f64(RESULT)
}

#[test]
fn the_command_is_registered_where_ttw_calls_it() {
    let e = engine(0);
    assert_eq!(ACTOR_HAS_BASE_FLAG, OPCODE_BASE + 0x4d);
    assert_eq!(COMMAND_NAMES, ["GetActorHasBaseFlag"]);
    assert_eq!(crate::COMMAND_NAMES, COMMAND_NAMES);
    let record = nvse::table::record_of(&e, ACTOR_HAS_BASE_FLAG);
    let text = |at: u32| nvse::table::guest_text(&e, e.mem.u32(at));
    assert_eq!(text(record), b"GetActorHasBaseFlag");
    assert_eq!(text(record + 4), b"ActorHasBaseFlag");
    assert_eq!(e.mem.u16(record + 0x12), 1);
    assert_eq!(e.mem.u32(record + 0x24) & 1, 1, "a condition can use it");
    assert_ne!(e.mem.u32(record + 0x20), 0);
    let params = e.mem.u32(record + 0x14);
    assert_eq!(e.mem.u32(params + 4), 1, "one integer");
}

#[test]
fn a_set_bit_answers_one_and_a_clear_bit_zero() {
    let mut e = engine(1 << 5 | 1 << 31 | 1);
    for (bit, want) in [(5, 1.0), (31, 1.0), (0, 1.0), (4, 0.0), (6, 0.0), (30, 0.0)] {
        assert_eq!(run(&mut e, REFR, bit), (1, want), "bit {bit}");
    }
}

#[test]
fn bits_outside_the_word_answer_zero() {
    let mut e = engine(u32::MAX);
    for bit in [32, 33, 255, -1, i32::MIN] {
        assert_eq!(run(&mut e, REFR, bit), (1, 0.0), "bit {bit}");
    }
}

#[test]
fn no_reference_or_no_actor_base_answers_zero() {
    let mut e = engine(u32::MAX);
    assert_eq!(run(&mut e, 0, 5), (1, 0.0));
    e.mem.set_u32(REFR + REFR_BASE_FORM, 0);
    assert_eq!(run(&mut e, REFR, 5), (1, 0.0));
    e.mem.set_u32(REFR + REFR_BASE_FORM, OTHER);
    assert_eq!(run(&mut e, REFR, 5), (1, 0.0));
}

#[test]
fn the_condition_answers_as_the_command_does() {
    let mut e = engine(1 << 5);
    assert_eq!(eval(&mut e, REFR, 5), 1.0);
    assert_eq!(eval(&mut e, REFR, 4), 0.0);
    assert_eq!(eval(&mut e, REFR, 32), 0.0);
    assert_eq!(eval(&mut e, 0, 5), 0.0);
    e.mem.set_u32(REFR + REFR_BASE_FORM, OTHER);
    assert_eq!(eval(&mut e, REFR, 5), 0.0);
}

/// An engine with the table, the plugin not yet installed.
fn bare() -> Engine {
    let mut e = Engine::new();
    e.map(0x0118_e000, 0x9000);
    e.map(0x011d_e000, 0x1000);
    nvse::table::install(&mut e);
    e
}

#[test]
fn the_switch_decides_what_install_builds() {
    // Off: nothing is registered.
    let mut e = bare();
    crate::set_answers_installed(&mut e, false);
    crate::install(&mut e);
    assert!(nvse::plugins::built_ins(&e).is_empty());
    // No switch: the constant, off.
    let mut e = bare();
    crate::install(&mut e);
    assert!(nvse::plugins::built_ins(&e).is_empty());
    // On: the plugin is listed and the command built.
    let mut e = bare();
    crate::set_answers_installed(&mut e, true);
    crate::install(&mut e);
    assert_eq!(nvse::plugins::built_ins(&e).len(), 1);
    assert_ne!(nvse::table::record_of(&e, ACTOR_HAS_BASE_FLAG), 0);
}
