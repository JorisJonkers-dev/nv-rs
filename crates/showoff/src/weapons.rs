// SPDX-License-Identifier: GPL-3.0-only
//! ShowOff xNVSE's weapon commands (part of group 5 of the profile's missing commands):
//! `IsWeaponMelee`, `IsWeaponRanged`, `IsEquippedWeaponMelee`, `GetWeaponHasFlag` and
//! `SetWeaponFlag`. Written from the commands' GECK Wiki pages (see `NOTICE`,
//! `provenance/04-weapons.md`); no plugin code was read.
//!
//! - `IsWeaponMelee [weapon]`: the weapon's type is at most 2 (the page: "does nothing more than
//!   check if the weapon's type is <= 2"); without the argument the calling reference's base form.
//! - `IsWeaponRanged [weapon]`: the type is from 3 to 13.
//! - `IsEquippedWeaponMelee`: the same for the weapon the calling actor has equipped; with none
//!   equipped, 1 (the page: "defaults to 1 (for default fists)").
//! - `GetWeaponHasFlag index [weapon]` (short name `WeaponHasFlag`): whether flag `index` (0 to
//!   21, the page's table) of the weapon is set; indices 0 to 7 are the bits of the first flags
//!   byte, 8 to 21 the bits of the second flags word ("a mashup of `GetWeaponFlags1` and
//!   `GetWeaponFlags2`").
//! - `SetWeaponFlag index on [weapon]` (short name `SetWeaponHasFlag`): sets or clears that flag;
//!   1 for success, 0 otherwise.
//!
//! All but the last are also condition functions.

use crate::commands::Cmd;
use crate::identity::OPCODE_BASE;
use engine_core::prelude::*;
use nvse::commands::inventory_forms::{find_equipped, mask_for_slot};
use nvse::commands::{dynamic_cast, form_or_caller, rtti, Read};
use nvse::table::{Param, Returns};
use nvse::{handled, CommandArgs, EvalArgs};

/// `kParamType_Integer`, `_ObjectID` (an item base form), `_AnyForm`.
const INT: u32 = 0x01;
const OBJECT_ID: u32 = 0x03;
const ANY_FORM: u32 = 0x3d;

const fn p(name: &'static str, kind: u32, optional: bool) -> Param {
    Param {
        name,
        kind,
        optional,
    }
}

const OPTIONAL_WEAPON: &[Param] = &[p("weapon", ANY_FORM, true)];
const NO_PARAMS: &[Param] = &[];
const FLAG_AND_WEAPON: &[Param] = &[p("flagIndex", INT, false), p("weapon", OBJECT_ID, true)];
const FLAG_ON_WEAPON: &[Param] = &[
    p("flagIndex", INT, false),
    p("bFlagOn", INT, false),
    p("weapon", OBJECT_ID, true),
];

/// The commands, in the order of their opcodes.
pub static COMMANDS: [Cmd; 5] = [
    Cmd {
        opcode: OPCODE_BASE + 0x0a,
        name: "IsWeaponMelee",
        short: "",
        params: OPTIONAL_WEAPON,
        execute: is_weapon_melee,
        eval: Some(is_weapon_melee_eval),
        needs_ref: false,
        expression: false,
        returns: Returns::Default,
    },
    Cmd {
        opcode: OPCODE_BASE + 0x0c,
        name: "IsWeaponRanged",
        short: "",
        params: OPTIONAL_WEAPON,
        execute: is_weapon_ranged,
        eval: Some(is_weapon_ranged_eval),
        needs_ref: false,
        expression: false,
        returns: Returns::Default,
    },
    Cmd {
        opcode: OPCODE_BASE + 0x0b,
        name: "IsEquippedWeaponMelee",
        short: "",
        params: NO_PARAMS,
        execute: is_equipped_weapon_melee,
        eval: Some(is_equipped_weapon_melee_eval),
        needs_ref: false,
        expression: false,
        returns: Returns::Default,
    },
    Cmd {
        opcode: OPCODE_BASE + 0x4c,
        name: "GetWeaponHasFlag",
        short: "WeaponHasFlag",
        params: FLAG_AND_WEAPON,
        execute: get_weapon_has_flag,
        eval: Some(get_weapon_has_flag_eval),
        needs_ref: false,
        expression: false,
        returns: Returns::Default,
    },
    Cmd {
        opcode: OPCODE_BASE + 0x7c,
        name: "SetWeaponFlag",
        short: "SetWeaponHasFlag",
        params: FLAG_ON_WEAPON,
        execute: set_weapon_flag,
        eval: None,
        needs_ref: false,
        expression: false,
        returns: Returns::Default,
    },
];

/// `TESObjectWEAP`: the weapon type (a byte), the first flags (a byte) and the second (a word).
const WEAPON_TYPE: u32 = 0xf4;
const FLAGS1: u32 = 0x100;
const FLAGS2: u32 = 0x12c;
/// The flag indices: 0 to 7 in the first flags, 8 to 21 in the second.
const FLAG_COUNT: u32 = 22;
const FIRST_FLAGS_BITS: u32 = 8;
/// The equipment slot of the weapon (`GetEquippedObject 5`).
const WEAPON_SLOT: u32 = 5;
/// The types the page calls melee (up to 2) and ranged (3 to 13).
const MELEE_LAST: u8 = 2;
const RANGED_LAST: u8 = 13;

/// The weapon of a form (a reference's base form) or of the calling reference's base form; 0 for
/// none or a form that is not a weapon.
fn weapon_for(e: &mut Engine, form: u32, this_obj: u32) -> u32 {
    match form_or_caller(e, form, this_obj) {
        Some(base) => dynamic_cast(e, base, rtti::TES_FORM, rtti::TES_OBJECT_WEAP),
        None => 0,
    }
}

fn weapon_type(e: &Engine, weapon: u32) -> u8 {
    e.mem.u8(weapon + WEAPON_TYPE)
}

fn is_melee(e: &Engine, weapon: u32) -> bool {
    weapon != 0 && weapon_type(e, weapon) <= MELEE_LAST
}

fn is_ranged(e: &Engine, weapon: u32) -> bool {
    weapon != 0 && (MELEE_LAST + 1..=RANGED_LAST).contains(&weapon_type(e, weapon))
}

/// The weapon the calling actor has equipped, 0 for none.
fn equipped_weapon(e: &mut Engine, actor: u32) -> u32 {
    if actor == 0 {
        return 0;
    }
    let (item, _) = find_equipped(e, actor, mask_for_slot(WEAPON_SLOT));
    dynamic_cast(e, item, rtti::TES_FORM, rtti::TES_OBJECT_WEAP)
}

fn equipped_is_melee(e: &mut Engine, actor: u32) -> bool {
    match equipped_weapon(e, actor) {
        0 => true,
        weapon => is_melee(e, weapon),
    }
}

/// Whether flag `index` is set; the address of its word and its bit.
fn flag_place(index: u32) -> Option<(u32, u32, u32)> {
    match index {
        i if i < FIRST_FLAGS_BITS => Some((FLAGS1, 1, i)),
        i if i < FLAG_COUNT => Some((FLAGS2, 4, i - FIRST_FLAGS_BITS)),
        _ => None,
    }
}

fn has_flag(e: &Engine, weapon: u32, index: u32) -> bool {
    let Some((at, size, bit)) = flag_place(index) else {
        return false;
    };
    if weapon == 0 {
        return false;
    }
    let word = if size == 1 {
        u32::from(e.mem.u8(weapon + at))
    } else {
        e.mem.u32(weapon + at)
    };
    word & (1 << bit) != 0
}

fn flag(on: bool) -> f64 {
    f64::from(u8::from(on))
}

fn is_weapon_melee(e: &mut Engine, w: &[u32]) -> Ret {
    weapon_answer(e, w, is_melee)
}

fn is_weapon_ranged(e: &mut Engine, w: &[u32]) -> Ret {
    weapon_answer(e, w, is_ranged)
}

fn weapon_answer(e: &mut Engine, w: &[u32], test: fn(&Engine, u32) -> bool) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if read.ok {
        let weapon = weapon_for(e, read.form(0), a.this_obj);
        a.set_result(e, flag(test(e, weapon)));
    }
    handled(true)
}

fn is_weapon_melee_eval(e: &mut Engine, w: &[u32]) -> Ret {
    weapon_eval(e, w, is_melee)
}

fn is_weapon_ranged_eval(e: &mut Engine, w: &[u32]) -> Ret {
    weapon_eval(e, w, is_ranged)
}

fn weapon_eval(e: &mut Engine, w: &[u32], test: fn(&Engine, u32) -> bool) -> Ret {
    let a = EvalArgs::new(w);
    let weapon = weapon_for(e, a.arg1, a.this_obj);
    e.mem.set_f64(a.result, flag(test(e, weapon)));
    handled(true)
}

fn is_equipped_weapon_melee(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    let melee = equipped_is_melee(e, a.this_obj);
    a.set_result(e, flag(melee && a.this_obj != 0));
    handled(true)
}

fn is_equipped_weapon_melee_eval(e: &mut Engine, w: &[u32]) -> Ret {
    let a = EvalArgs::new(w);
    let melee = equipped_is_melee(e, a.this_obj);
    e.mem.set_f64(a.result, flag(melee && a.this_obj != 0));
    handled(true)
}

fn get_weapon_has_flag(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if read.ok {
        let weapon = weapon_for(e, read.form(1), a.this_obj);
        let on = has_flag(e, weapon, read.int(0, 0));
        a.set_result(e, flag(on));
    }
    handled(true)
}

fn get_weapon_has_flag_eval(e: &mut Engine, w: &[u32]) -> Ret {
    let a = EvalArgs::new(w);
    let weapon = weapon_for(e, a.arg2, a.this_obj);
    let on = has_flag(e, weapon, a.arg1);
    e.mem.set_f64(a.result, flag(on));
    handled(true)
}

fn set_weapon_flag(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if !read.ok {
        return handled(true);
    }
    let weapon = weapon_for(e, read.form(2), a.this_obj);
    if let (Some((at, size, bit)), true) = (flag_place(read.int(0, 0)), weapon != 0) {
        let on = read.int(1, 0) != 0;
        let word = if size == 1 {
            u32::from(e.mem.u8(weapon + at))
        } else {
            e.mem.u32(weapon + at)
        };
        let word = if on {
            word | (1 << bit)
        } else {
            word & !(1 << bit)
        };
        if size == 1 {
            e.mem.set_u8(weapon + at, word as u8);
        } else {
            e.mem.set_u32(weapon + at, word);
        }
        a.set_result(e, 1.0);
    }
    handled(true)
}

#[cfg(test)]
#[path = "weapons_tests.rs"]
mod tests;
