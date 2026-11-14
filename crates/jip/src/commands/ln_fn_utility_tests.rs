//! Tests of the utility commands built so far: the version, a form list as an array, and the two
//! commands that count and sort the forms of an array by type. Forms are built at the raw offsets:
//! the type byte at +4, the id at +0xC, the base form of a reference at +0x20 and the vtable
//! word at +0xF0 that marks a reference (`IS_REFERENCE`).

use super::*;
use crate::commands::test_support::*;
use nvse::vars::{Key, Value, KEY_NUMBER};

const FMAP: u32 = FORMS + 0x20000;
const ITEMS: u32 = FORMS + 0x21000;
const REF_VT: u32 = FORMS + 0x22000;
const PLAIN_VT: u32 = FORMS + 0x22100;
const LIST: u32 = FORMS + 0x1000;
const NODES: u32 = FORMS + 0x1100;
/// Forms: A and B of type 0x1f, C of type 0x20, REFR (type 0x3a) of base A, REFR2 of base C.
const A: u32 = FORMS + 0x2000;
const B: u32 = FORMS + 0x2100;
const C: u32 = FORMS + 0x2200;
const REFR: u32 = FORMS + 0x2300;
const REFR2: u32 = FORMS + 0x2400;

fn exists(e: &mut Engine, form: u32, id: u32, n: u32) {
    let item = ITEMS + 0x10 * n;
    let bucket = FMAP + 0x100 + 4 * (id % 16);
    e.mem.set_u32(item, e.mem.u32(bucket));
    e.mem.set_u32(item + 4, id);
    e.mem.set_u32(item + 8, form);
    e.mem.set_u32(bucket, item);
}

fn setup() -> Engine {
    let mut e = engine();
    e.map(0x011c_5000, 0x1000);
    e.mem.set_u32(0x011c_54c0, FMAP);
    e.mem.set_u32(FMAP + 4, 16);
    e.mem.set_u32(FMAP + 8, FMAP + 0x100);
    e.mem.set_u32(REF_VT + 0xf0, 0x008d_0360);
    for (i, (f, t, id, vt)) in [
        (A, 0x1f, 0x0100_0001, PLAIN_VT),
        (B, 0x1f, 0x0100_0002, PLAIN_VT),
        (C, 0x20, 0x0100_0003, PLAIN_VT),
        (REFR, 0x3a, 0xff00_0001, REF_VT),
        (REFR2, 0x3a, 0xff00_0002, REF_VT),
    ]
    .into_iter()
    .enumerate()
    {
        form(&mut e, f, t, id);
        e.mem.set_u32(f, vt);
        exists(&mut e, f, id, i as u32);
    }
    e.mem.set_u32(REFR + 0x20, A);
    e.mem.set_u32(REFR2 + 0x20, C);
    e
}

fn array_of(e: &mut Engine, values: &[Value]) -> u32 {
    let id = vars::create_array(e, KEY_NUMBER, true, 6);
    for (i, v) in values.iter().enumerate() {
        vars::set_element(e, id, &Key::Number(i as f64), v.clone());
    }
    id
}

fn ids(e: &Engine, id: u32) -> Vec<Data> {
    values(e, id).expect("an array").1
}

fn forms(ids: &[u32]) -> Vec<Value> {
    ids.iter().map(|&i| Value::Form(i)).collect()
}

#[test]
fn the_version() {
    let mut e = setup();
    assert_eq!(call(&mut e, 0x2200, &[]).1, 57.3);
}

#[test]
fn a_list_as_an_array_skips_empty_items() {
    let mut e = setup();
    e.mem.set_u8(LIST + 4, 0x55);
    form(&mut e, LIST, 0x55, 0x0100_0099);
    set_ref(&mut e, 1, LIST);
    // The inline first node (+0x18) holds A, then an empty node, then B.
    e.mem.set_u32(LIST + 0x18, A);
    e.mem.set_u32(LIST + 0x1c, NODES);
    e.mem.set_u32(NODES, 0);
    e.mem.set_u32(NODES + 4, NODES + 8);
    e.mem.set_u32(NODES + 8, B);
    e.mem.set_u32(NODES + 12, 0);
    let (_, id) = call(&mut e, 0x2212, &[r(1)]);
    assert_eq!(
        ids(&e, id as u32),
        vec![Data::Form(0x0100_0001), Data::Form(0x0100_0002)]
    );
}

#[test]
fn counting_forms_of_a_type() {
    let mut e = setup();
    let arr = array_of(
        &mut e,
        &forms(&[
            0x0100_0001,
            0x0100_0002,
            0x0100_0003,
            0xff00_0001,
            0x0200_0000,
        ]),
    );
    assert_eq!(call(&mut e, 0x2300, &[n(arr as i32), n(0x1f)]).1, 2.0);
    assert_eq!(
        call(&mut e, 0x2300, &[n(arr as i32), n(0x3a)]).1,
        1.0,
        "a reference by its own type"
    );
    assert_eq!(call(&mut e, 0x2300, &[n(arr as i32), n(0x99)]).1, 0.0);
    // Not a plain array: nothing.
    let map = vars::create_array(&mut e, nvse::vars::KEY_STRING, false, 6);
    assert_eq!(call(&mut e, 0x2300, &[n(map as i32), n(0x1f)]).1, 0.0);
}

#[test]
fn sorting_forms_by_type_takes_each_group_from_the_end() {
    let mut e = setup();
    let list = array_of(
        &mut e,
        &forms(&[
            0x0100_0001,
            0x0100_0003,
            0x0100_0002,
            0xff00_0002,
            0xff00_0001,
        ]),
    );
    // Types 0x20 first (C, and REFR2 whose base is C), then 0x1f (A, B and REFR of base A).
    let types = array_of(&mut e, &[Value::Number(32.0), Value::Number(31.0)]);
    let (_, id) = call(&mut e, 0x22ff, &[n(list as i32), n(types as i32)]);
    assert_eq!(
        ids(&e, id as u32),
        vec![
            Data::Form(0xff00_0002),
            Data::Form(0x0100_0003),
            Data::Form(0xff00_0001),
            Data::Form(0x0100_0002),
            Data::Form(0x0100_0001),
        ]
    );
    // A type nothing has: the forms in their order.
    let other = array_of(&mut e, &[Value::Number(1.0)]);
    let (_, id) = call(&mut e, 0x22ff, &[n(list as i32), n(other as i32)]);
    assert_eq!(ids(&e, id as u32).len(), 5);
    assert_eq!(ids(&e, id as u32)[0], Data::Form(0x0100_0001));
}
