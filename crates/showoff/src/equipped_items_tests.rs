// SPDX-License-Identifier: GPL-3.0-only
//! Tests of `GetEquippedItems` through its record: an actor wearing a hat in two slots, a
//! non-playable armor and a quest weapon, and carrying a ring it does not wear; the default, a
//! slot mask and the filters; a reference that is not an actor. The inventory is built as the
//! game lays it out (container changes, entry list, worn extra data lists).

use super::*;
use crate::test_support::{call, engine, n};
use fork_catalogue::{covers, ids};
use nvse::vars::{Data, Element};

const GET_EQUIPPED_ITEMS: u32 = 0x3cd3;

const ACTOR: u32 = 0x0400_0000;
const NOT_ACTOR: u32 = 0x0400_0400;
const CHANGES: u32 = 0x0400_0800;
const DATA: u32 = 0x0400_0900;
/// The items, each with its biped part at `+0x30`.
const HAT: u32 = 0x0400_1000;
const ARMOR: u32 = 0x0400_1400;
const WEAPON: u32 = 0x0400_1800;
const RING: u32 = 0x0400_1c00;
const PART: u32 = 0x30;
/// Entry list nodes, entries, extend-data nodes and extra data lists, one block per item.
const LISTS: u32 = 0x0400_4000;

const HEAD: u32 = 1 << 0;
const HAIR: u32 = 1 << 1;
const UPPER_BODY: u32 = 1 << 2;
const WEAPON_SLOT: u32 = 1 << 5;
const NOSE_RING: u32 = 1 << 12;

/// `EXTRA_GET_BY_TYPE` (`BaseExtraList::GetByType`), the worn extra data type and the presence
/// bits of an extra data list (`+8`).
const GET_BY_TYPE: u32 = 0x0041_0220;
const WORN: u32 = 0x16;

fn setup() -> Engine {
    let mut e = engine();
    e.map(0x0400_0000, 0x8000);
    e.register(0x00ec_43fb, |_, a| {
        let (form, to) = (a[0], a[3]);
        let cast = match (form, to) {
            (ACTOR, rtti::ACTOR) => ACTOR,
            (HAT | ARMOR | RING, rtti::TES_BIPED_MODEL_FORM) => form + PART,
            (WEAPON, rtti::TES_OBJECT_WEAP) => WEAPON,
            _ => 0,
        };
        Ret {
            eax: cast,
            ..Ret::default()
        }
    });
    e.register(GET_BY_TYPE, |_, a| Ret {
        eax: if a[0] == ACTOR + 0x44 && a[1] == 0x15 {
            CHANGES
        } else {
            0
        },
        ..Ret::default()
    });
    e.mem.set_u32(CHANGES + 0xc, DATA);
    let items = [
        (HAT, HEAD | HAIR, true),
        (ARMOR, UPPER_BODY, true),
        (WEAPON, 0, true),
        (RING, NOSE_RING, false),
    ];
    let mut prev_node = DATA;
    for (i, (item, mask, worn)) in items.into_iter().enumerate() {
        let block = LISTS + i as u32 * 0x100;
        let (node, entry, extend, xlist) = (block, block + 0x20, block + 0x40, block + 0x60);
        e.mem
            .set_u32(prev_node + if prev_node == DATA { 0 } else { 4 }, node);
        e.mem.set_u32(node, entry);
        e.mem.set_u32(entry, extend);
        e.mem.set_u32(entry + 8, item);
        e.mem.set_u32(extend, xlist);
        if worn {
            e.mem.set_u8(xlist + 8 + (WORN >> 3), 1 << (WORN & 7));
        }
        if item == WEAPON {
            e.mem.set_u8(item + 4, 0x28);
        } else {
            e.mem.set_u8(item + 4, 0x18);
            e.mem.set_u32(item + PART + 4, mask);
        }
        e.mem.set_u32(item + 0x0c, 0x0001_0000 + i as u32);
        prev_node = node;
    }
    // The armor is not playable, the weapon is a quest item.
    e.mem.set_u8(ARMOR + PART + 8, 0x40);
    e.mem.set_u32(WEAPON + 8, 0x400);
    e
}

/// The form ids of the array the command answers.
fn items(e: &mut Engine, this: u32, flags: Option<i32>) -> Vec<u32> {
    let args: Vec<Vec<u8>> = flags.map(n).into_iter().collect();
    let (ok, id) = call(e, GET_EQUIPPED_ITEMS, this, &args);
    assert_eq!(ok, 1);
    let id = id as u32;
    let len = nvse::vars::array(e, id).expect("an array").len();
    (0..len)
        .map(
            |i| match nvse::vars::get_element(e, id, &Key::Number(i as f64)) {
                Some(Element {
                    data: Data::Form(f),
                    ..
                }) => f,
                other => panic!("not a form: {other:?}"),
            },
        )
        .collect()
}

#[test]
fn the_record_is_at_its_opcode() {
    let e = engine();
    let record = nvse::table::record_of(&e, GET_EQUIPPED_ITEMS);
    assert_ne!(record, 0);
    assert_eq!(
        nvse::table::guest_text(&e, e.mem.u32(record)),
        b"GetEquippedItems"
    );
    assert!(crate::COMMAND_NAMES.contains(&"GetEquippedItems"));
    let params = e.mem.u32(record + 0x14);
    assert_eq!([e.mem.u32(params + 4), e.mem.u32(params + 8)], [INT, 1]);
}

/// Without flags: every playable worn item once, in slot order (the hat in two slots once; the
/// non-playable armor and the ring that is not worn left out; the quest weapon kept).
#[test]
fn without_flags_the_worn_playable_items_are_listed_once() {
    covers(&[ids::extenders::showoff::GETEQUIPPEDITEMS]);
    let mut e = setup();
    assert_eq!(items(&mut e, ACTOR, None), vec![0x0001_0000, 0x0001_0002]);
    assert_eq!(
        items(&mut e, ACTOR, Some(0)),
        vec![0x0001_0000, 0x0001_0002]
    );
}

/// A slot mask asks only those slots and keeps non-playable items; bit 20 drops them again; bit
/// 21 drops quest items; a reference that is not an actor answers an empty array.
#[test]
fn slots_and_filters_choose_the_items() {
    covers(&[ids::extenders::showoff::GETEQUIPPEDITEMS]);
    let mut e = setup();
    let all_slots = (SLOT_BITS) as i32;
    assert_eq!(
        items(&mut e, ACTOR, Some(all_slots)),
        vec![0x0001_0000, 0x0001_0001, 0x0001_0002]
    );
    assert_eq!(items(&mut e, ACTOR, Some(HAIR as i32)), vec![0x0001_0000]);
    assert_eq!(
        items(&mut e, ACTOR, Some((UPPER_BODY | WEAPON_SLOT) as i32)),
        vec![0x0001_0001, 0x0001_0002]
    );
    assert_eq!(
        items(&mut e, ACTOR, Some((UPPER_BODY | NO_UNPLAYABLE) as i32)),
        Vec::<u32>::new()
    );
    assert_eq!(
        items(&mut e, ACTOR, Some((WEAPON_SLOT | NO_QUEST_ITEMS) as i32)),
        Vec::<u32>::new()
    );
    assert_eq!(
        items(&mut e, ACTOR, Some(NOSE_RING as i32)),
        Vec::<u32>::new()
    );
    assert_eq!(items(&mut e, NOT_ACTOR, None), Vec::<u32>::new());
}
