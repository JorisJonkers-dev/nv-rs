// SPDX-License-Identifier: GPL-3.0-only
//! `Clamp`, `Lerp` and `Sign` through their records, with the examples of their GECK Wiki pages.

use super::*;
use crate::test_support::{call, engine, s, z};
use fork_catalogue::{covers, ids};

const CLAMP: u32 = 0x3155;
const LERP: u32 = 0x3157;
const SIGN: u32 = 0x315a;

fn run(e: &mut Engine, opcode: u32, args: &[f64]) -> (u32, f64) {
    let args: Vec<Vec<u8>> = args.iter().map(|v| z(*v)).collect();
    call(e, opcode, 0, &args)
}

/// The opcodes are the plugin's registration order from `0x3100`, and the records name three
/// float parameters (one for `Sign`), none optional.
#[test]
fn records_sit_at_their_opcodes() {
    let e = engine();
    for (opcode, name, count) in [(CLAMP, "Clamp", 3), (LERP, "Lerp", 3), (SIGN, "Sign", 1)] {
        let record = nvse::table::record_of(&e, opcode);
        assert_eq!(
            nvse::table::guest_text(&e, e.mem.u32(record)),
            name.as_bytes()
        );
        assert_eq!(e.mem.u16(record + 0x12), count);
        let params = e.mem.u32(record + 0x14);
        for k in 0..u32::from(count) {
            assert_eq!(e.mem.u32(params + 12 * k + 4), 0x02, "float");
            assert_eq!(e.mem.u32(params + 12 * k + 8), 0, "required");
        }
        assert_eq!(
            e.mem.u32(record + 0x18),
            nvse::table::execute_address(opcode)
        );
    }
    let names: Vec<_> = COMMANDS.iter().map(|c| c.name).collect();
    assert_eq!(names, ["Clamp", "Lerp", "Sign"]);
}

/// The three examples of the page, then the inside, the two ends and a degenerate range.
#[test]
fn clamp_follows_the_pages_examples() {
    covers(&[ids::extenders::johnnyguitar::CLAMP]);
    let mut e = engine();
    assert_eq!(run(&mut e, CLAMP, &[5.0, 10.0, 20.0]), (1, 10.0));
    assert_eq!(run(&mut e, CLAMP, &[75.0, 100.0, 50.0]), (1, 100.0));
    assert_eq!(run(&mut e, CLAMP, &[1000.0, 500.0, 50.0]), (1, 50.0));
    assert_eq!(run(&mut e, CLAMP, &[15.0, 10.0, 20.0]), (1, 15.0));
    assert_eq!(run(&mut e, CLAMP, &[10.0, 10.0, 20.0]), (1, 10.0));
    assert_eq!(run(&mut e, CLAMP, &[20.0, 10.0, 20.0]), (1, 20.0));
    assert_eq!(run(&mut e, CLAMP, &[99.0, 20.0, 20.0]), (1, 20.0));
    assert_eq!(run(&mut e, CLAMP, &[-3.5, -10.0, -1.0]), (1, -3.5));
    // A float, as every float argument is: 0.1 is not exact in 32 bits.
    assert_eq!(run(&mut e, CLAMP, &[0.1, 0.0, 1.0]).1, f64::from(0.1f32));
}

/// Both ends exactly (the zip-line example reaches its end point), the middle, and a `t` outside
/// the interval, which goes on along the line.
#[test]
fn lerp_interpolates_between_its_ends() {
    covers(&[ids::extenders::johnnyguitar::LERP]);
    let mut e = engine();
    assert_eq!(run(&mut e, LERP, &[10.0, 20.0, 0.0]), (1, 10.0));
    assert_eq!(run(&mut e, LERP, &[10.0, 20.0, 1.0]), (1, 20.0));
    assert_eq!(run(&mut e, LERP, &[10.0, 20.0, 0.25]), (1, 12.5));
    assert_eq!(run(&mut e, LERP, &[-4.0, 4.0, 0.5]), (1, 0.0));
    assert_eq!(run(&mut e, LERP, &[1.0, 3.0, 1.5]), (1, 4.0));
    // The ends are exact where `v0 + (v1 - v0) * t` would round: 0.1 to 0.3.
    assert_eq!(run(&mut e, LERP, &[0.1, 0.3, 1.0]).1, f64::from(0.3f32));
}

/// The page's example (5 is 1), the negative, and the zero the page leaves open.
#[test]
fn sign_answers_one_or_minus_one() {
    covers(&[ids::extenders::johnnyguitar::SIGN]);
    let mut e = engine();
    assert_eq!(run(&mut e, SIGN, &[5.0]), (1, 1.0));
    assert_eq!(run(&mut e, SIGN, &[-0.001]), (1, -1.0));
    assert_eq!(run(&mut e, SIGN, &[1.0e30]), (1, 1.0));
    assert_eq!(run(&mut e, SIGN, &[0.0]), (1, 0.0));
    assert_eq!(run(&mut e, SIGN, &[f64::NAN]), (1, 0.0));
}

/// A call whose arguments cannot be read (a string where a number goes) answers 0 and the
/// command still reports itself handled: the script goes on with 0.
#[test]
fn unreadable_arguments_answer_zero() {
    covers(&[ids::extenders::johnnyguitar::CLAMP]);
    let mut e = engine();
    let bad = s("x");
    let (ok, result) = call(&mut e, CLAMP, 0, &[bad, z(1.0), z(2.0)]);
    assert_eq!((ok, result), (1, 0.0));
}

/// The pure functions use no state: nothing is registered while the plugin is not installed.
#[test]
fn nothing_is_registered_while_not_installed() {
    let mut e = Engine::new();
    e.map(0x0118_e000, 0x9000);
    nvse::table::install(&mut e);
    crate::install_as(&mut e, false);
    assert_eq!(nvse::table::record_of(&e, CLAMP), 0);
}
