// SPDX-License-Identifier: GPL-3.0-only
//! `GetBodyPartTraitString` through its record, on a body part data form with a torso and a head:
//! every trait of a part, as the page's example asks the torso's part name, and what a wrong
//! form, part or trait answers.

use super::*;
use crate::test_support::{call, engine, n, r, set_script_ref};
use fork_catalogue::{covers, ids};

const GET_BODY_PART_TRAIT_STRING: u32 = 0x3192;

const DATA_FORM: u32 = 0x0300_4000;
const OTHER_FORM: u32 = 0x0300_4400;
const TORSO: u32 = 0x0300_4800;
const HEAD: u32 = 0x0300_4c00;
const TEXTS: u32 = 0x0300_5000;

/// Puts `text` (a C string) in the `BSStringT` at `part + at`.
fn set_name(e: &mut Engine, part: u32, at: u32, text: &str) {
    let to = TEXTS + (part & 0xf00) + at * 0x10;
    e.mem.write(to, text.as_bytes());
    e.mem.set_u8(to + text.len() as u32, 0);
    e.mem.set_u32(part + at, to);
    e.mem.set_u16(part + at + 4, text.len() as u16);
}

fn setup() -> Engine {
    let mut e = engine();
    e.mem.set_u8(DATA_FORM + FORM_TYPE, TYPE_BODY_PART_DATA);
    e.mem.set_u32(DATA_FORM + BODY_PARTS, TORSO);
    e.mem.set_u32(DATA_FORM + BODY_PARTS + 4, HEAD);
    for (at, text) in [
        (0x04, "Bip01 Spine2"),
        (0x0c, "Bip01 Spine1"),
        (0x14, "Bip01 Pelvis"),
        (0x1c, "Torso"),
        (0x24, "Bip01 Spine"),
    ] {
        set_name(&mut e, TORSO, at, text);
    }
    // The head has a part name only.
    set_name(&mut e, HEAD, 0x1c, "Head");
    // A form of another type with the same pointers.
    e.mem.set_u8(OTHER_FORM + FORM_TYPE, 0x28);
    e.mem.set_u32(OTHER_FORM + BODY_PARTS, TORSO);
    set_script_ref(&mut e, 1, DATA_FORM);
    set_script_ref(&mut e, 2, OTHER_FORM);
    e
}

fn trait_string(e: &mut Engine, form: u16, part: i32, trait_id: i32) -> String {
    let (ok, id) = call(
        e,
        GET_BODY_PART_TRAIT_STRING,
        0,
        &[r(form), n(part), n(trait_id)],
    );
    assert_eq!(ok, 1);
    String::from_utf8(nvse::vars::string(e, id as u32).expect("a string")).unwrap()
}

#[test]
fn the_record_sits_at_its_opcode_with_its_name_and_a_string_result() {
    let e = setup();
    assert_eq!(COMMANDS[0].opcode, GET_BODY_PART_TRAIT_STRING);
    assert_eq!(COMMANDS[0].returns, Returns::String);
    assert!(crate::COMMAND_NAMES.contains(&COMMANDS[0].name));
    let record = nvse::table::record_of(&e, GET_BODY_PART_TRAIT_STRING);
    assert_eq!(
        nvse::table::guest_text(&e, e.mem.u32(record)),
        b"GetBodyPartTraitString"
    );
}

/// The page's example asks the torso (type 0) for its part name (trait 4); each of the five
/// traits reads its own name.
#[test]
fn every_trait_of_a_part_reads_its_own_name() {
    covers(&[ids::extenders::johnnyguitar::GETBODYPARTTRAITSTRING]);
    let mut e = setup();
    let want = [
        (1, "Bip01 Spine2"),
        (2, "Bip01 Spine1"),
        (3, "Bip01 Pelvis"),
        (4, "Torso"),
        (5, "Bip01 Spine"),
    ];
    for (trait_id, text) in want {
        assert_eq!(
            trait_string(&mut e, 1, 0, trait_id),
            text,
            "trait {trait_id}"
        );
    }
    assert_eq!(trait_string(&mut e, 1, 1, 4), "Head");
}

/// A name the part does not have, a part the form does not have, a type or trait out of range
/// and a form that is not body part data all answer an empty string.
#[test]
fn a_missing_part_name_trait_or_wrong_form_answers_an_empty_string() {
    covers(&[ids::extenders::johnnyguitar::GETBODYPARTTRAITSTRING]);
    let mut e = setup();
    assert_eq!(trait_string(&mut e, 1, 1, 1), "");
    assert_eq!(trait_string(&mut e, 1, 2, 4), "");
    assert_eq!(trait_string(&mut e, 1, 15, 4), "");
    assert_eq!(trait_string(&mut e, 1, -1, 4), "");
    assert_eq!(trait_string(&mut e, 1, 0, 0), "");
    assert_eq!(trait_string(&mut e, 1, 0, 6), "");
    assert_eq!(trait_string(&mut e, 2, 0, 4), "");
}
