// SPDX-License-Identifier: GPL-3.0-only
//! `EditorIDToFormID` and `GetEditorID` through their records, on two forms whose editor ids the
//! stand-ins of the game's lookup and name function answer, as the page's examples use them.

use super::*;
use crate::test_support::{call, engine, s};

const EDITOR_ID_TO_FORM_ID: u32 = 0x3128;
const GET_EDITOR_ID: u32 = 0x3114;

const FORM_A: u32 = 0x0300_4000;
const FORM_B: u32 = 0x0300_4400;
const REF_C: u32 = 0x0300_4800;
const VTABLE: u32 = 0x0300_5000;
const NAMES: u32 = 0x0300_5400;
/// Each form's editor id (a C string) at `form + 0x40`.
const NAME_AT: u32 = 0x40;

fn eax(v: u32) -> Ret {
    Ret {
        eax: v,
        ..Ret::default()
    }
}

/// The game's name function: the text at `form + 0x40`, or 0.
fn name_of(e: &mut Engine, a: &[u32]) -> Ret {
    eax(e.mem.u32(a[0] + NAME_AT))
}

/// The game's lookup (case-blind, as the built-in store is): the forms `A` and `B`.
fn by_name(e: &mut Engine, a: &[u32]) -> Ret {
    let want = nvse::table::guest_text(e, a[0]).to_ascii_lowercase();
    for form in [FORM_A, FORM_B] {
        let at = e.mem.u32(form + NAME_AT);
        if at != 0 && nvse::table::guest_text(e, at).to_ascii_lowercase() == want {
            return eax(form);
        }
    }
    eax(0)
}

fn setup() -> Engine {
    let mut e = engine();
    e.register(FORM_BY_EDITOR_ID, by_name);
    e.register(VTABLE + GET_EDITOR_ID_SLOT, name_of);
    e.mem
        .set_u32(VTABLE + GET_EDITOR_ID_SLOT, VTABLE + GET_EDITOR_ID_SLOT);
    for (form, id, name) in [
        (FORM_A, 0x0001_0a01, Some("Weap10mmPistol")),
        (FORM_B, 0x0001_0b02, Some("GSSunnySmiles")),
        (REF_C, 0x0001_0c03, None),
    ] {
        e.mem.set_u32(form, VTABLE);
        e.mem.set_u32(form + FORM_ID, id);
        if let Some(name) = name {
            let at = NAMES + (form & 0xfff);
            e.mem.write(at, name.as_bytes());
            e.mem.set_u8(at + name.len() as u32, 0);
            e.mem.set_u32(form + NAME_AT, at);
        }
    }
    e
}

fn form_arg(e: &mut Engine, form: u32) -> Vec<u8> {
    let ref_var = 0x0300_a010;
    e.mem.set_u32(ref_var + 8, form);
    e.mem.set_u32(ref_var + 0xc, 0);
    e.mem.set_u32(crate::test_support::SCRIPT + 0x44, ref_var);
    e.mem.set_u32(crate::test_support::SCRIPT + 0x48, 0);
    let mut b = vec![b'r'];
    b.extend_from_slice(&1u16.to_le_bytes());
    b
}

fn string_of(e: &Engine, id: f64) -> String {
    String::from_utf8(nvse::vars::string(e, id as u32).expect("a string")).unwrap()
}

fn id_result(result: f64) -> u32 {
    result.to_bits() as u32
}

#[test]
fn records_sit_at_the_opcodes_with_their_return_types() {
    let e = setup();
    for (opcode, name, short, ret) in [
        (GET_EDITOR_ID, "GetEditorID", "GetEdID", Returns::String),
        (EDITOR_ID_TO_FORM_ID, "EditorIDToFormID", "", Returns::Form),
    ] {
        let record = nvse::table::record_of(&e, opcode);
        assert_eq!(
            nvse::table::guest_text(&e, e.mem.u32(record)),
            name.as_bytes()
        );
        assert_eq!(
            nvse::table::guest_text(&e, e.mem.u32(record + 4)),
            short.as_bytes()
        );
        assert_eq!(nvse::table::return_type(&e, opcode), ret);
    }
}

/// The page's example: a name that a form has answers its form id, the name is case-sensitive
/// (the store is not, so the exact name is checked), and a name no form has answers 0.
#[test]
fn editor_id_to_form_id_finds_a_form_by_its_exact_name() {
    let mut e = setup();
    let r = call(&mut e, EDITOR_ID_TO_FORM_ID, 0, &[s("GSSunnySmiles")]);
    assert_eq!(r.0, 1);
    assert_eq!(id_result(r.1), 0x0001_0b02);
    let r = call(&mut e, EDITOR_ID_TO_FORM_ID, 0, &[s("Weap10mmPistol")]);
    assert_eq!(id_result(r.1), 0x0001_0a01);
    // Another case: the lookup finds the form, the exact check refuses it.
    assert_eq!(
        id_result(call(&mut e, EDITOR_ID_TO_FORM_ID, 0, &[s("gssunnysmiles")]).1),
        0
    );
    assert_eq!(
        id_result(call(&mut e, EDITOR_ID_TO_FORM_ID, 0, &[s("NoSuchForm")]).1),
        0
    );
    assert_eq!(
        call(&mut e, EDITOR_ID_TO_FORM_ID, 0, &[s("NoSuchForm")]).0,
        1
    );
    // Empty text and a prefix of a name are no form.
    assert_eq!(
        id_result(call(&mut e, EDITOR_ID_TO_FORM_ID, 0, &[s("")]).1),
        0
    );
    assert_eq!(
        id_result(call(&mut e, EDITOR_ID_TO_FORM_ID, 0, &[s("GSSunny")]).1),
        0
    );
}

/// The page's example (a weapon's editor id), a form with none answers an empty string, and a
/// call without a form asks the calling reference.
#[test]
fn get_editor_id_answers_the_name_or_an_empty_string() {
    let mut e = setup();
    let arg = form_arg(&mut e, FORM_A);
    let r = call(&mut e, GET_EDITOR_ID, 0, &[arg]);
    assert_eq!(r.0, 1);
    assert_eq!(string_of(&e, r.1), "Weap10mmPistol");
    let arg = form_arg(&mut e, REF_C);
    let r = call(&mut e, GET_EDITOR_ID, 0, &[arg]);
    assert_eq!(string_of(&e, r.1), "");
    let r = call(&mut e, GET_EDITOR_ID, FORM_B, &[]);
    assert_eq!(string_of(&e, r.1), "GSSunnySmiles");
    let r = call(&mut e, GET_EDITOR_ID, 0, &[]);
    assert_eq!(string_of(&e, r.1), "");
}
