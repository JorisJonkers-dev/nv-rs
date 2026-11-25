// SPDX-License-Identifier: GPL-3.0-only
//! JohnnyGuitar NVSE's form traits (group 3 of the profile's missing commands: form data; 20 calls
//! in five plugins of the profile). Written from the commands' GECK Wiki pages (see `NOTICE`,
//! `provenance/03-form-traits.md`); no plugin code was read.
//!
//! - `GetBaseEffectAV effect` and `GetBaseEffectArchetype effect`: a base effect's actor value
//!   and archetype codes (the `DATA` record's last two words, `EffectSetting` +0x9C and +0x98).
//! - `GetInteriorLightingTraitNumeric cell trait` and `SetInteriorLightingTraitNumeric cell trait
//!   value`: one of the sixteen numbers of an interior cell's lighting (`INTERIOR_DATA`, the
//!   cell's `pCellData` at +0x48): the red, green and blue of the ambient (0 to 2), directional
//!   (3 to 5) and fog (9 to 11) colours, the directional rotation (6 XY, 7 Z) and fade (8), the fog
//!   near (12), far (13) and power (14) and the clip distance (15).
//! - `GetWeaponVATSTraitNumeric weapon trait` and `SetWeaponVATSTraitNumeric weapon trait value`:
//!   one of the five numbers of a weapon's V.A.T.S. special attack (`OBJ_WEAP_VATS_SPECIAL` at
//!   +0x370): skill (1), damage multiplier (2), action points (3), silent (4), mod required (5).
//!
//! A form of another type, a cell that is not an interior, or a trait outside the page's range
//! answers 0 and changes nothing (the pages say nothing of these cases).

use crate::commands::Cmd;
use crate::identity::OPCODE_BASE;
use engine_core::prelude::*;
use nvse::commands::Read;
use nvse::table::{Param, Returns};
use nvse::{handled, CommandArgs};

/// `kParamType_Integer`, `_Float`, `_AnyForm`.
const INT: u32 = 0x01;
const FLOAT: u32 = 0x02;
const ANY_FORM: u32 = 0x3d;

const fn p(name: &'static str, kind: u32) -> Param {
    Param {
        name,
        kind,
        optional: false,
    }
}

const ONE_FORM: &[Param] = &[p("form", ANY_FORM)];
const FORM_INT: &[Param] = &[p("form", ANY_FORM), p("int", INT)];
const FORM_INT_FLOAT: &[Param] = &[p("form", ANY_FORM), p("int", INT), p("float", FLOAT)];

const fn cmd(offset: u32, name: &'static str, params: &'static [Param], execute: AbiFn) -> Cmd {
    Cmd {
        opcode: OPCODE_BASE + offset,
        name,
        short: "",
        params,
        execute,
        eval: None,
        needs_ref: false,
        returns: Returns::Default,
    }
}

/// The commands, in the order of their opcodes.
pub static COMMANDS: [Cmd; 6] = [
    cmd(0x03, "GetBaseEffectAV", ONE_FORM, get_base_effect_av),
    cmd(
        0x04,
        "GetBaseEffectArchetype",
        ONE_FORM,
        get_base_effect_archetype,
    ),
    cmd(
        0x0b,
        "GetInteriorLightingTraitNumeric",
        FORM_INT,
        get_interior_lighting,
    ),
    cmd(
        0x0c,
        "SetInteriorLightingTraitNumeric",
        FORM_INT_FLOAT,
        set_interior_lighting,
    ),
    cmd(0x69, "GetWeaponVATSTraitNumeric", FORM_INT, get_weapon_vats),
    cmd(
        0x6a,
        "SetWeaponVATSTraitNumeric",
        FORM_INT_FLOAT,
        set_weapon_vats,
    ),
];

/// `TESForm::typeID` (+4) of a base effect, a cell and a weapon.
const FORM_TYPE: u32 = 4;
const TYPE_EFFECT_SETTING: u8 = 0x10;
const TYPE_CELL: u8 = 0x39;
const TYPE_WEAPON: u8 = 0x28;

/// `EffectSetting`'s `data.eArchetype` and `data.eActorValue`.
const ARCHETYPE: u32 = 0x98;
const ACTOR_VALUE: u32 = 0x9c;

/// `TESObjectCELL::cCellFlags` (bit 0: interior) and `pCellData`.
const CELL_FLAGS: u32 = 0x24;
const CELL_DATA: u32 = 0x48;

/// `TESObjectWEAP::specialVATSData`.
const WEAPON_VATS: u32 = 0x370;

/// The form if its type is `kind`, else 0.
fn of_type(e: &Engine, form: u32, kind: u8) -> u32 {
    if form != 0 && e.mem.u8(form + FORM_TYPE) == kind {
        form
    } else {
        0
    }
}

fn get_base_effect(e: &mut Engine, w: &[u32], at: u32) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if read.ok {
        let effect = of_type(e, read.form(0), TYPE_EFFECT_SETTING);
        if effect != 0 {
            let v = e.mem.u32(effect + at) as i32;
            a.set_result(e, f64::from(v));
        }
    }
    handled(true)
}

fn get_base_effect_av(e: &mut Engine, w: &[u32]) -> Ret {
    get_base_effect(e, w, ACTOR_VALUE)
}

fn get_base_effect_archetype(e: &mut Engine, w: &[u32]) -> Ret {
    get_base_effect(e, w, ARCHETYPE)
}

/// Where an interior lighting trait lives in `INTERIOR_DATA`.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Slot {
    /// A byte of a packed colour (red is the low byte).
    Byte(u32),
    /// A signed whole number.
    Int(u32),
    Float(u32),
}

/// The page's trait ids, 0 to 15, in `INTERIOR_DATA`: the ambient (+0), directional (+4) and fog
/// (+8) colours, the fog near (+0xC) and far (+0x10), the directional rotation XY (+0x14) and Z
/// (+0x18), the directional fade (+0x1C), the clip distance (+0x20), the fog power (+0x24).
fn lighting_slot(trait_id: u32) -> Option<Slot> {
    Some(match trait_id {
        0..=2 => Slot::Byte(trait_id),
        3..=5 => Slot::Byte(4 + trait_id - 3),
        6 => Slot::Int(0x14),
        7 => Slot::Int(0x18),
        8 => Slot::Float(0x1c),
        9..=11 => Slot::Byte(8 + trait_id - 9),
        12 => Slot::Float(0x0c),
        13 => Slot::Float(0x10),
        14 => Slot::Float(0x24),
        15 => Slot::Float(0x20),
        _ => return None,
    })
}

/// The interior data of `form` when it is an interior cell that has it, else 0.
fn interior_data(e: &Engine, form: u32) -> u32 {
    let cell = of_type(e, form, TYPE_CELL);
    if cell == 0 || e.mem.u8(cell + CELL_FLAGS) & 1 == 0 {
        return 0;
    }
    e.mem.u32(cell + CELL_DATA)
}

fn read_slot(e: &Engine, base: u32, slot: Slot) -> f64 {
    match slot {
        Slot::Byte(at) => f64::from(e.mem.u8(base + at)),
        Slot::Int(at) => f64::from(e.mem.u32(base + at) as i32),
        Slot::Float(at) => f64::from(e.mem.f32(base + at)),
    }
}

/// A byte trait takes the value's whole part held to 0 to 255; a whole-number trait its whole
/// part.
fn write_slot(e: &mut Engine, base: u32, slot: Slot, value: f32) {
    match slot {
        Slot::Byte(at) => e.mem.set_u8(base + at, value.clamp(0.0, 255.0) as u8),
        Slot::Int(at) => e.mem.set_u32(base + at, value as i32 as u32),
        Slot::Float(at) => e.mem.set_f32(base + at, value),
    }
}

fn get_interior_lighting(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if !read.ok {
        return handled(true);
    }
    let data = interior_data(e, read.form(0));
    if let (true, Some(slot)) = (data != 0, lighting_slot(read.int(1, u32::MAX))) {
        let v = read_slot(e, data, slot);
        a.set_result(e, v);
    }
    handled(true)
}

fn set_interior_lighting(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if !read.ok {
        return handled(true);
    }
    let data = interior_data(e, read.form(0));
    if let (true, Some(slot)) = (data != 0, lighting_slot(read.int(1, u32::MAX))) {
        write_slot(e, data, slot, read.float(2, 0.0));
    }
    handled(true)
}

/// The page's trait ids, 1 to 5, in `OBJ_WEAP_VATS_SPECIAL`: skill (+4), damage multiplier (+8),
/// action points (+0xC) as floats; silent (+0x10) and mod required (+0x11) as bytes.
fn vats_slot(trait_id: u32) -> Option<Slot> {
    Some(match trait_id {
        1 => Slot::Float(0x04),
        2 => Slot::Float(0x08),
        3 => Slot::Float(0x0c),
        4 => Slot::Byte(0x10),
        5 => Slot::Byte(0x11),
        _ => return None,
    })
}

fn get_weapon_vats(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if !read.ok {
        return handled(true);
    }
    let weapon = of_type(e, read.form(0), TYPE_WEAPON);
    if let (true, Some(slot)) = (weapon != 0, vats_slot(read.int(1, u32::MAX))) {
        let v = read_slot(e, weapon + WEAPON_VATS, slot);
        a.set_result(e, v);
    }
    handled(true)
}

/// Silent and mod required are flags: any value other than 0 sets them to 1.
fn set_weapon_vats(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if !read.ok {
        return handled(true);
    }
    let weapon = of_type(e, read.form(0), TYPE_WEAPON);
    if let (true, Some(slot)) = (weapon != 0, vats_slot(read.int(1, u32::MAX))) {
        let value = read.float(2, 0.0);
        let value = match slot {
            Slot::Byte(_) => f32::from(u8::from(value != 0.0)),
            _ => value,
        };
        write_slot(e, weapon + WEAPON_VATS, slot, value);
    }
    handled(true)
}

#[cfg(test)]
#[path = "form_traits_tests.rs"]
mod tests;
