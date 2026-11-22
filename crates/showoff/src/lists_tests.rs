// SPDX-License-Identifier: GPL-3.0-only
//! `ListAddList` and `ListAddArray` through their records, on form lists laid out in the test's
//! memory, a bump heap standing in for the game's, and a stand-in for the game's form lookup. The
//! answers pinned are the pages' (1 for success, 0 for an error) and the decisions of
//! `provenance/01-lists.md`; the commands have not been run in the real game.

use super::*;
use crate::test_support::{add_local, call, call_expr, engine, n, r, set_script_ref};
use nvse::vars::{Key, Value};

const LIST_ADD_LIST: u32 = 0x3cad;
const LIST_ADD_ARRAY: u32 = 0x3d1c;

/// The destination list (holding `F1`), the source (holding `F2`, `F3`, `F4`), an empty list, four
/// forms that are no list and the bump heap.
const DEST: u32 = 0x0200_0000;
const SOURCE: u32 = 0x0200_0400;
const EMPTY: u32 = 0x0200_0800;
/// A list holding `F4` only.
const ONE: u32 = 0x0200_0c00;
const F: [u32; 4] = [0x0200_1000, 0x0200_1400, 0x0200_1800, 0x0200_1c00];
const NODES: u32 = 0x0200_4000;
const HEAP: u32 = 0x0200_8000;
const ID_BASE: u32 = 0x0001_0000;

fn eax(v: u32) -> Ret {
    Ret {
        eax: v,
        ..Ret::default()
    }
}

fn alloc(e: &mut Engine, size: u32) -> u32 {
    let at = e.mem.u32(HEAP).max(HEAP + 0x10);
    e.mem.set_u32(HEAP, at + size.next_multiple_of(0x10));
    for i in 0..size {
        e.mem.set_u8(at + i, 0);
    }
    at
}

/// The form of an id: the lists and `F1` to `F4` have the ids `0x10010`, `0x10011`, `0x10012`,
/// `0x10021` to `0x10024`; any other id has no form.
fn form_of(id: u32) -> u32 {
    match id.wrapping_sub(ID_BASE) {
        0x10 => DEST,
        0x11 => SOURCE,
        0x12 => EMPTY,
        0x13 => ONE,
        i @ 0x21..=0x24 => F[(i - 0x21) as usize],
        _ => 0,
    }
}

fn id_of(form: u32) -> u32 {
    let at = [DEST, SOURCE, EMPTY, ONE].iter().position(|&f| f == form);
    match at {
        Some(i) => ID_BASE + 0x10 + i as u32,
        None => ID_BASE + 0x21 + F.iter().position(|&f| f == form).unwrap() as u32,
    }
}

fn setup() -> Engine {
    let mut e = engine();
    e.map(0x0200_0000, 0x10000);
    e.map(0x011c_3000, 0x1000);
    e.register(0x0040_1000, |e, a| eax(alloc(e, a[0])));
    e.register(0x0040_1030, |_, _| eax(0));
    e.register(LOOKUP_FORM, |_, a| eax(form_of(a[0])));
    for (form, kind) in [
        (DEST, 0x55),
        (SOURCE, 0x55),
        (EMPTY, 0x55),
        (ONE, 0x55),
        (F[0], 0x1f),
        (F[1], 0x1f),
        (F[2], 0x1f),
        (F[3], 0x1f),
    ] {
        e.mem.set_u8(form + 4, kind);
        e.mem.set_u32(form + 0xc, id_of(form));
    }
    e.mem.set_u32(ONE + 0x18, F[3]);
    // DEST: F1. SOURCE: F2, F3, F4.
    e.mem.set_u32(DEST + 0x18, F[0]);
    e.mem.set_u32(SOURCE + 0x18, F[1]);
    e.mem.set_u32(SOURCE + 0x1c, NODES);
    e.mem.set_u32(NODES, F[2]);
    e.mem.set_u32(NODES + 4, NODES + 8);
    e.mem.set_u32(NODES + 8, F[3]);
    e.mem.set_u32(NODES + 12, 0);
    e.mem.set_u32(0x0300_0000 + 0x1000 + 0xc, 0x0500_0d00);
    // The script's references: 1 DEST, 2 SOURCE, 3 EMPTY, 4 to 7 the forms, 8 the list of one form.
    for (i, f) in [DEST, SOURCE, EMPTY, F[0], F[1], F[2], F[3], ONE]
        .into_iter()
        .enumerate()
    {
        set_script_ref(&mut e, i as u16 + 1, f);
    }
    e
}

/// The list's forms as `1` to `4` for `F1` to `F4`.
fn members(e: &Engine, list: u32) -> Vec<u32> {
    nvse::invref::list_items(e, list + 0x18)
        .into_iter()
        .map(|f| F.iter().position(|&x| x == f).unwrap() as u32 + 1)
        .collect()
}

fn add_list(e: &mut Engine, args: &[Vec<u8>]) -> f64 {
    let (ok, result) = call(e, LIST_ADD_LIST, 0, args);
    assert_eq!(ok, 1, "the handler always answers handled");
    result
}

#[test]
fn the_commands_are_registered_where_the_plugin_lists_them() {
    let e = setup();
    assert_eq!(COMMANDS[0].opcode, 0x3cad);
    assert_eq!(COMMANDS[1].opcode, 0x3d1c);
    assert_eq!(COMMANDS[0].name, crate::COMMAND_NAMES[1]);
    assert_eq!(COMMANDS[1].name, crate::COMMAND_NAMES[2]);
    for c in &COMMANDS {
        let record = nvse::table::record_of(&e, c.opcode);
        assert_ne!(record, 0);
        let text = |at: u32| nvse::table::guest_text(&e, e.mem.u32(at));
        assert_eq!(text(record), c.name.as_bytes());
        assert_eq!(text(record + 4), c.short.as_bytes());
        assert_eq!(e.mem.u16(record + 0x12), 3);
    }
    let parse = |opcode: u32| e.mem.u32(nvse::table::record_of(&e, opcode) + 0x1c);
    assert_ne!(
        parse(0x3cad),
        parse(0x3d1c),
        "only ListAddArray is parsed as an expression"
    );
    assert_eq!(parse(0x3d1c), nvse::table::EXPRESSION_PARSE);
}

#[test]
fn a_list_is_appended_at_the_end_in_its_order() {
    let mut e = setup();
    assert_eq!(add_list(&mut e, &[r(1), r(2)]), 1.0);
    assert_eq!(members(&e, DEST), [1, 2, 3, 4]);
    assert_eq!(members(&e, SOURCE), [2, 3, 4], "the source is not changed");
    // Again: duplicates are allowed, as `ListAddForm` allows them.
    assert_eq!(add_list(&mut e, &[r(1), r(2)]), 1.0);
    assert_eq!(members(&e, DEST), [1, 2, 3, 4, 2, 3, 4]);
    // The end given as -2, and any other negative index.
    let mut e = setup();
    assert_eq!(add_list(&mut e, &[r(1), r(2), n(-2)]), 1.0);
    assert_eq!(add_list(&mut e, &[r(1), r(2), n(-7)]), 1.0);
    assert_eq!(members(&e, DEST), [1, 2, 3, 4, 2, 3, 4]);
}

#[test]
fn an_index_puts_the_forms_there_with_their_order_kept() {
    let mut e = setup();
    assert_eq!(add_list(&mut e, &[r(1), r(2), n(0)]), 1.0);
    assert_eq!(members(&e, DEST), [2, 3, 4, 1]);
    // Into the middle of a longer list: before the form that is there.
    let mut e = setup();
    assert_eq!(add_list(&mut e, &[r(1), r(2)]), 1.0);
    assert_eq!(add_list(&mut e, &[r(1), r(2), n(3)]), 1.0);
    assert_eq!(members(&e, DEST), [1, 2, 3, 2, 3, 4, 4]);
}

#[test]
fn an_error_answers_zero() {
    let mut e = setup();
    // A destination or a source that is no list.
    assert_eq!(add_list(&mut e, &[r(4), r(2)]), 0.0);
    assert_eq!(add_list(&mut e, &[r(1), r(5)]), 0.0);
    // An index with no place.
    assert_eq!(add_list(&mut e, &[r(1), r(2), n(99)]), 0.0);
    assert_eq!(members(&e, DEST), [1], "nothing was added");
    // An index equal to the count has no form to go before: an error, as `ListAddForm`'s.
    assert_eq!(add_list(&mut e, &[r(1), r(2), n(1)]), 0.0);
    assert_eq!(members(&e, DEST), [1]);
    let mut e = setup();
    assert_eq!(add_list(&mut e, &[r(1)]), 0.0, "no source: the read fails");
    assert_eq!(members(&e, DEST), [1]);
}

#[test]
fn an_empty_source_is_a_success_and_a_list_added_to_itself_doubles() {
    let mut e = setup();
    assert_eq!(add_list(&mut e, &[r(1), r(3)]), 1.0);
    assert_eq!(members(&e, DEST), [1]);
    assert_eq!(add_list(&mut e, &[r(2), r(2)]), 1.0);
    assert_eq!(members(&e, SOURCE), [2, 3, 4, 2, 3, 4]);
}

#[test]
fn forms_scripts_added_are_counted_as_listaddform_counts_them() {
    // An index below the count of forms scripts added goes after them (ListAddForm's rule): the
    // order of the source is still kept.
    let mut e = setup();
    assert_eq!(add_list(&mut e, &[r(1), r(2)]), 1.0);
    e.mem.set_u32(DEST + 0x20, 1);
    assert_eq!(add_list(&mut e, &[r(1), r(8), n(0)]), 1.0);
    assert_eq!(
        members(&e, DEST),
        [1, 4, 2, 3, 4],
        "after the one form scripts added"
    );
    assert_eq!(e.mem.u32(DEST + 0x20), 1);
}

/// Array `id` in script local `local`.
fn array_of(e: &mut Engine, local: u16, key: u8, packed: bool, items: &[(Key, Value)]) -> u32 {
    let id = vars::create_array(e, key, packed, 5);
    for (k, v) in items {
        vars::set_element(e, id, k, v.clone());
    }
    add_local(e, local, &format!("a{local}"), f64::from(id));
    id
}

fn form_value(f: u32) -> Value {
    Value::Form(id_of(f))
}

fn list_expr(i: u16) -> Vec<u8> {
    let mut b = vec![b'R'];
    b.extend_from_slice(&i.to_le_bytes());
    b
}

fn array_expr(local: u16) -> Vec<u8> {
    let mut b = vec![b'V', 3, 0, 0];
    b.extend_from_slice(&local.to_le_bytes());
    b
}

fn number_expr(v: f64) -> Vec<u8> {
    let mut b = vec![b'Z'];
    b.extend_from_slice(&v.to_le_bytes());
    b
}

fn add_array(e: &mut Engine, exprs: &[Vec<u8>]) -> f64 {
    let (ok, result) = call_expr(e, LIST_ADD_ARRAY, exprs);
    assert_eq!(ok, 1);
    result
}

fn mixed() -> Vec<(Key, Value)> {
    [
        Value::Number(1.0),
        Value::Number(2.0),
        form_value(F[2]),
        Value::Str(b"x".to_vec()),
        Value::Form(ID_BASE + 0x999),
        form_value(F[1]),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, v)| (Key::Number(i as f64), v))
    .collect()
}

#[test]
fn an_array_adds_its_forms_and_skips_the_rest() {
    let mut e = setup();
    array_of(&mut e, 1, vars::KEY_NUMBER, true, &mixed());
    assert_eq!(add_array(&mut e, &[list_expr(1), array_expr(1)]), 1.0);
    assert_eq!(
        members(&e, DEST),
        [1, 3, 2],
        "numbers, strings and unknown ids skipped"
    );
    // At an index, the order kept.
    let mut e = setup();
    array_of(&mut e, 1, vars::KEY_NUMBER, true, &mixed());
    assert_eq!(
        add_array(&mut e, &[list_expr(1), array_expr(1), number_expr(0.0)]),
        1.0
    );
    assert_eq!(members(&e, DEST), [3, 2, 1]);
}

#[test]
fn a_map_is_added_in_its_key_order() {
    let mut e = setup();
    array_of(
        &mut e,
        1,
        vars::KEY_STRING,
        false,
        &[
            (Key::Str(b"b".to_vec()), form_value(F[3])),
            (Key::Str(b"a".to_vec()), form_value(F[1])),
        ],
    );
    assert_eq!(add_array(&mut e, &[list_expr(1), array_expr(1)]), 1.0);
    assert_eq!(members(&e, DEST), [1, 2, 4]);
    let mut e = setup();
    array_of(
        &mut e,
        1,
        vars::KEY_NUMBER,
        false,
        &[
            (Key::Number(9.0), form_value(F[3])),
            (Key::Number(-1.0), form_value(F[1])),
        ],
    );
    assert_eq!(add_array(&mut e, &[list_expr(1), array_expr(1)]), 1.0);
    assert_eq!(members(&e, DEST), [1, 2, 4]);
}

#[test]
fn an_array_error_answers_zero() {
    let mut e = setup();
    array_of(&mut e, 1, vars::KEY_NUMBER, true, &mixed());
    // A first argument that is no list.
    assert_eq!(add_array(&mut e, &[list_expr(4), array_expr(1)]), 0.0);
    // An array that does not exist (a local holding no array).
    add_local(&mut e, 2, "gone", 0.0);
    assert_eq!(add_array(&mut e, &[list_expr(1), array_expr(2)]), 0.0);
    // An index that is not a number.
    let text = {
        let mut b = vec![b'S'];
        b.extend_from_slice(&1u16.to_le_bytes());
        b.push(b'x');
        b
    };
    assert_eq!(add_array(&mut e, &[list_expr(1), array_expr(1), text]), 0.0);
    // An index with no place; one argument.
    assert_eq!(
        add_array(&mut e, &[list_expr(1), array_expr(1), number_expr(99.0)]),
        0.0
    );
    assert_eq!(add_array(&mut e, &[list_expr(1)]), 0.0);
    assert_eq!(members(&e, DEST), [1], "nothing was added");
}

#[test]
fn an_array_without_forms_is_a_success() {
    let mut e = setup();
    array_of(&mut e, 1, vars::KEY_NUMBER, true, &[]);
    assert_eq!(add_array(&mut e, &[list_expr(1), array_expr(1)]), 1.0);
    assert_eq!(members(&e, DEST), [1]);
}
