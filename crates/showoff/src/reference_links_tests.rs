// SPDX-License-Identifier: GPL-3.0-only
//! Tests of `SetEnableParent` and `GetEquippedWeapon` through their records, with stand-ins for
//! the game's enable-parent functions (a parent word per reference, a child list per extra data
//! list) and for `Actor::GetCurrentWeapon`.

use super::*;
use crate::test_support::{call, engine, r, set_script_ref, RESULT};
use fork_catalogue::{covers, ids};

const SET_ENABLE_PARENT: u32 = 0x3d30;
const GET_EQUIPPED_WEAPON: u32 = 0x3d31;

/// References (persistent unless named so), an actor, a base form, two weapons.
const CHILD: u32 = 0x0400_0000;
const PARENT: u32 = 0x0400_1000;
const OTHER_PARENT: u32 = 0x0400_2000;
const NOT_PERSISTENT: u32 = 0x0400_3000;
const BASE_FORM: u32 = 0x0400_4000;
const ACTOR: u32 = 0x0400_5000;
const UNARMED_ACTOR: u32 = 0x0400_6000;
const WEAPON: u32 = 0x0400_7000;
const FISTS: u32 = 0x0400_8000;

/// The stand-ins' words in each reference: its enable parent, whether it persists, and (in its
/// extra data list) the children that joined it, counted then listed.
const PARENT_WORD: u32 = 0x80;
const PERSISTS: u32 = 0x84;
const EXTRA_LIST: u32 = 0x44;
const CHILD_COUNT: u32 = 0x100;
const CHILDREN: u32 = 0x104;

fn setup() -> Engine {
    let mut e = engine();
    e.map(0x0400_0000, 0x9000);
    e.map(0x011c_a000, 0x1000);
    for refr in [CHILD, PARENT, OTHER_PARENT, ACTOR] {
        e.mem.set_u32(refr + PERSISTS, 1);
    }
    e.mem.set_u32(WEAPON + 0x0c, 0x0001_1111);
    e.mem.set_u32(FISTS + 0x0c, 0x0000_01f4);
    e.mem.set_u32(FISTS_WEAPON, FISTS);
    e.register(0x00ec_43fb, |_, a| {
        let (form, to) = (a[0], a[3]);
        let refr = matches!(
            form,
            CHILD | PARENT | OTHER_PARENT | NOT_PERSISTENT | ACTOR | UNARMED_ACTOR
        );
        let cast = match to {
            rtti::TES_OBJECT_REFR if refr => form,
            rtti::ACTOR if matches!(form, ACTOR | UNARMED_ACTOR) => form,
            _ => 0,
        };
        Ret {
            eax: cast,
            ..Ret::default()
        }
    });
    e.register(REFR_ENABLE_PARENT, |e, a| Ret {
        eax: e.mem.u32(a[0] + PARENT_WORD),
        ..Ret::default()
    });
    // `0056aa10`: leaves the old parent's list (not modelled), keeps only a persistent parent.
    e.register(REFR_SET_ENABLE_PARENT, |e, a| {
        let (child, parent) = (a[0], a[1]);
        if parent == 0 || e.mem.u32(parent + PERSISTS) != 0 {
            e.mem.set_u32(child + PARENT_WORD, parent);
        }
        Ret::default()
    });
    e.register(REFR_EXTRA_LIST, |_, a| Ret {
        eax: a[0] + EXTRA_LIST,
        ..Ret::default()
    });
    e.register(EXTRA_ADD_ENABLE_CHILD, |e, a| {
        let (list, child) = (a[0], a[1]);
        let refr = list - EXTRA_LIST;
        let n = e.mem.u32(refr + CHILD_COUNT);
        e.mem.set_u32(refr + CHILDREN + n * 4, child);
        e.mem.set_u32(refr + CHILD_COUNT, n + 1);
        Ret::default()
    });
    e.register(ACTOR_GET_CURRENT_WEAPON, |_, a| Ret {
        eax: if a[0] == ACTOR { WEAPON } else { 0 },
        ..Ret::default()
    });
    for (i, form) in [PARENT, OTHER_PARENT, NOT_PERSISTENT, BASE_FORM, CHILD]
        .into_iter()
        .enumerate()
    {
        set_script_ref(&mut e, i as u16 + 1, form);
    }
    e
}

fn parent_of(e: &Engine, refr: u32) -> u32 {
    e.mem.u32(refr + PARENT_WORD)
}

fn children_of(e: &Engine, refr: u32) -> Vec<u32> {
    let n = e.mem.u32(refr + CHILD_COUNT);
    (0..n).map(|i| e.mem.u32(refr + CHILDREN + i * 4)).collect()
}

/// The records: the names at their opcodes (`SetParentRef` the short name), an optional form and
/// no parameter, both needing a reference.
#[test]
fn the_records_are_at_their_opcodes() {
    let e = engine();
    for (opcode, name, short) in [
        (SET_ENABLE_PARENT, "SetEnableParent", "SetParentRef"),
        (GET_EQUIPPED_WEAPON, "GetEquippedWeapon", ""),
    ] {
        let record = nvse::table::record_of(&e, opcode);
        assert_ne!(record, 0, "{name}");
        assert_eq!(
            nvse::table::guest_text(&e, e.mem.u32(record)),
            name.as_bytes()
        );
        assert!(crate::COMMAND_NAMES.contains(&name));
        let short_text = e.mem.u32(record + 4);
        if short.is_empty() {
            assert!(short_text == 0 || e.mem.u8(short_text) == 0);
        } else {
            assert_eq!(nvse::table::guest_text(&e, short_text), short.as_bytes());
        }
    }
    let params = e
        .mem
        .u32(nvse::table::record_of(&e, SET_ENABLE_PARENT) + 0x14);
    assert_eq!(
        [e.mem.u32(params + 4), e.mem.u32(params + 8)],
        [ANY_FORM, 1]
    );
}

/// The page's two uses: a new parent is stored and the reference joins its child list (so the
/// parent's enable and disable reach it); a second call moves it to another parent; without a
/// parent the enable parent is removed.
#[test]
fn set_enable_parent_sets_moves_and_removes_the_parent() {
    covers(&[ids::extenders::showoff::SETENABLEPARENT]);
    let mut e = setup();
    assert_eq!(call(&mut e, SET_ENABLE_PARENT, CHILD, &[r(1)]).0, 1);
    assert_eq!(parent_of(&e, CHILD), PARENT);
    assert_eq!(children_of(&e, PARENT), vec![CHILD]);

    call(&mut e, SET_ENABLE_PARENT, CHILD, &[r(2)]);
    assert_eq!(parent_of(&e, CHILD), OTHER_PARENT);
    assert_eq!(children_of(&e, OTHER_PARENT), vec![CHILD]);

    call(&mut e, SET_ENABLE_PARENT, CHILD, &[]);
    assert_eq!(parent_of(&e, CHILD), 0, "no parameter removes it");
    assert_eq!(
        children_of(&e, OTHER_PARENT),
        vec![CHILD],
        "no join without a parent"
    );
}

/// What leaves the enable parent as it was: a parent the game does not keep (not persistent:
/// stored nowhere, joins no list), a form that is not a reference, the reference itself, a parent
/// whose own chain leads back to the reference, and a caller that is not a reference.
#[test]
fn set_enable_parent_refuses_what_the_game_would_not_keep_and_loops() {
    covers(&[ids::extenders::showoff::SETENABLEPARENT]);
    let mut e = setup();
    call(&mut e, SET_ENABLE_PARENT, CHILD, &[r(1)]);

    call(&mut e, SET_ENABLE_PARENT, CHILD, &[r(3)]);
    assert_eq!(
        parent_of(&e, CHILD),
        PARENT,
        "the game keeps only a persistent parent"
    );
    assert!(children_of(&e, NOT_PERSISTENT).is_empty());

    call(&mut e, SET_ENABLE_PARENT, CHILD, &[r(4)]);
    assert_eq!(
        parent_of(&e, CHILD),
        PARENT,
        "a base form is not a reference"
    );

    call(&mut e, SET_ENABLE_PARENT, CHILD, &[r(5)]);
    assert_eq!(parent_of(&e, CHILD), PARENT, "not its own parent");

    // PARENT's own parent is OTHER_PARENT, whose parent would be CHILD: a loop.
    e.mem.set_u32(PARENT + PARENT_WORD, OTHER_PARENT);
    e.mem.set_u32(CHILD + PARENT_WORD, 0);
    e.mem.set_u32(OTHER_PARENT + PARENT_WORD, CHILD);
    call(&mut e, SET_ENABLE_PARENT, CHILD, &[r(1)]);
    assert_eq!(parent_of(&e, CHILD), 0, "a loop is refused");

    call(&mut e, SET_ENABLE_PARENT, BASE_FORM, &[r(1)]);
    assert_eq!(parent_of(&e, BASE_FORM), 0);
}

/// The actor's current weapon as a form; with none the fists; another kind of reference, or no
/// reference, no form.
#[test]
fn get_equipped_weapon_answers_the_weapon_or_the_fists() {
    covers(&[ids::extenders::showoff::GETEQUIPPEDWEAPON]);
    let mut e = setup();
    call(&mut e, GET_EQUIPPED_WEAPON, ACTOR, &[]);
    assert_eq!(e.mem.u32(RESULT), 0x0001_1111);
    call(&mut e, GET_EQUIPPED_WEAPON, UNARMED_ACTOR, &[]);
    assert_eq!(e.mem.u32(RESULT), 0x0000_01f4, "the fists");
    call(&mut e, GET_EQUIPPED_WEAPON, PARENT, &[]);
    assert_eq!(e.mem.u32(RESULT), 0, "not an actor");
    call(&mut e, GET_EQUIPPED_WEAPON, 0, &[]);
    assert_eq!(e.mem.u32(RESULT), 0);
}
