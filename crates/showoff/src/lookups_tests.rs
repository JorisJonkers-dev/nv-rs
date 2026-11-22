// SPDX-License-Identifier: GPL-3.0-only
//! `IsBaseForm` through its record: a base form, a reference, an empty variable.

use super::*;
use crate::test_support::{call, engine, r, set_script_ref};

const IS_BASE_FORM: u32 = 0x3cd4;
const BASE: u32 = 0x0300_4000;
const REFERENCE: u32 = 0x0300_4400;

fn setup() -> Engine {
    let mut e = engine();
    // DYNAMIC_CAST stood in for: a form of type 0x3a (reference) casts to `TESObjectREFR`.
    e.register(0x00ec_43fb, |e, a| {
        let is_ref = a[3] == rtti::TES_OBJECT_REFR && e.mem.u8(a[0] + 4) == 0x3a;
        Ret {
            eax: if is_ref { a[0] } else { 0 },
            ..Ret::default()
        }
    });
    e.mem.set_u8(BASE + 4, 0x28);
    e.mem.set_u8(REFERENCE + 4, 0x3a);
    e
}

fn answer(e: &mut Engine, form: u32) -> (u32, f64) {
    set_script_ref(e, 1, form);
    call(e, IS_BASE_FORM, 0, &[r(1)])
}

#[test]
fn record_sits_at_the_opcode_with_its_short_name() {
    let e = setup();
    let record = nvse::table::record_of(&e, IS_BASE_FORM);
    assert_eq!(
        nvse::table::guest_text(&e, e.mem.u32(record)),
        b"IsBaseForm"
    );
    assert_eq!(
        nvse::table::guest_text(&e, e.mem.u32(record + 4)),
        b"IsBasedForm"
    );
}

#[test]
fn only_a_valid_form_that_is_not_a_reference_is_a_base_form() {
    let mut e = setup();
    assert_eq!(answer(&mut e, BASE), (1, 1.0));
    assert_eq!(answer(&mut e, REFERENCE), (1, 0.0));
    assert_eq!(answer(&mut e, 0), (1, 0.0));
}

#[test]
fn the_name_is_listed_after_the_other_groups() {
    assert_eq!(COMMANDS[0].name, crate::COMMAND_NAMES[9]);
}
