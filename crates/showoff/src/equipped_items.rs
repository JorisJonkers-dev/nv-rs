// SPDX-License-Identifier: GPL-3.0-only
//! ShowOff xNVSE's `GetEquippedItems` (with group 9 of the profile's missing commands). Written
//! from the command's GECK Wiki page (see `NOTICE`,
//! `provenance/09-enable-parent-equipped-weapon.md`); no plugin code was read.
//!
//! - `actor.GetEquippedItems [flags]`: an array of the base forms the actor has equipped, one
//!   each, in slot order (head first). The low 20 bits of `flags` choose the equip slots (none:
//!   every slot); bit 20 leaves out items that are not playable, bit 21 quest items, bit 22 items
//!   with no slot. With no flags only playable items are listed, as the page says.
//!
//! Each slot is asked as xNVSE's `GetEquippedObject` asks it ([`find_equipped`]: the first stack
//! that uses the slot and is worn), so an item in several slots is listed once and an item with no
//! slot is never found (bit 22 therefore changes nothing). A reference that is not an actor
//! answers an empty array.

use crate::commands::Cmd;
use crate::identity::OPCODE_BASE;
use engine_core::prelude::*;
use nvse::commands::inventory_forms::find_equipped;
use nvse::commands::{dynamic_cast, rtti, Read};
use nvse::table::{Param, Returns};
use nvse::vars::{self, Key, Value, KEY_NUMBER};
use nvse::{handled, CommandArgs};

/// `kParamType_Integer`.
const INT: u32 = 0x01;

const ONE_OPTIONAL_INT: &[Param] = &[Param {
    name: "int",
    kind: INT,
    optional: true,
}];

/// The commands, in the order of their opcodes.
pub static COMMANDS: [Cmd; 1] = [Cmd {
    opcode: OPCODE_BASE + 0x40,
    name: "GetEquippedItems",
    short: "",
    params: ONE_OPTIONAL_INT,
    execute: get_equipped_items,
    eval: None,
    needs_ref: true,
    expression: false,
    returns: Returns::Array,
}];

/// The page's flags: twenty equip slots, then the three filters.
pub const SLOTS: u32 = 20;
pub const SLOT_BITS: u32 = (1 << SLOTS) - 1;
pub const NO_UNPLAYABLE: u32 = 1 << 20;
pub const NO_QUEST_ITEMS: u32 = 1 << 21;

/// `TESForm::flags` (+8) and its quest item bit (`TESForm::IsQuestItem`, `00401190`).
const FORM_FLAGS: u32 = 8;
const QUEST_ITEM: u32 = 0x400;
/// `TESBipedModelForm`'s flags byte (+8) and its non-playable bit; a weapon's first flags byte
/// (+0x100) and its non-playable bit (xNVSE's `IsPlayable`).
const BIPED_FLAGS: u32 = 8;
const BIPED_NON_PLAYABLE: u8 = 0x40;
const WEAPON_FLAGS1: u32 = 0x100;
const WEAPON_NON_PLAYABLE: u8 = 0x80;
/// `TESForm::refID`.
const FORM_ID: u32 = 0x0c;

/// Whether the item is playable, as xNVSE's `IsPlayable` answers for armor and weapons; any other
/// item counts as playable.
fn playable(e: &mut Engine, item: u32) -> bool {
    let biped = dynamic_cast(e, item, rtti::TES_FORM, rtti::TES_BIPED_MODEL_FORM);
    if biped != 0 {
        return e.mem.u8(biped + BIPED_FLAGS) & BIPED_NON_PLAYABLE == 0;
    }
    let weapon = dynamic_cast(e, item, rtti::TES_FORM, rtti::TES_OBJECT_WEAP);
    if weapon != 0 {
        return e.mem.u8(weapon + WEAPON_FLAGS1) & WEAPON_NON_PLAYABLE == 0;
    }
    true
}

/// The equipped items of `actor` that `flags` keeps, in slot order, each once.
pub fn equipped_items(e: &mut Engine, actor: u32, flags: u32) -> Vec<u32> {
    let slots = match flags & SLOT_BITS {
        0 => SLOT_BITS,
        chosen => chosen,
    };
    let no_unplayable = flags == 0 || flags & NO_UNPLAYABLE != 0;
    let mut items = Vec::new();
    for slot in 0..SLOTS {
        if slots & (1 << slot) == 0 {
            continue;
        }
        let (item, _) = find_equipped(e, actor, 1 << slot);
        if item == 0 || items.contains(&item) {
            continue;
        }
        if no_unplayable && !playable(e, item) {
            continue;
        }
        if flags & NO_QUEST_ITEMS != 0 && e.mem.u32(item + FORM_FLAGS) & QUEST_ITEM != 0 {
            continue;
        }
        items.push(item);
    }
    items
}

/// `actor.GetEquippedItems [flags]`: the array (owned by the calling script's mod).
fn get_equipped_items(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if !read.ok {
        return handled(true);
    }
    let flags = read.int(0, 0);
    let actor = if a.this_obj == 0 {
        0
    } else {
        dynamic_cast(e, a.this_obj, rtti::TES_FORM, rtti::ACTOR)
    };
    let items = if actor == 0 {
        Vec::new()
    } else {
        equipped_items(e, actor, flags)
    };
    let modi = vars::script_mod_index(e, a.script);
    let array = vars::create_array(e, KEY_NUMBER, true, modi);
    for (i, item) in items.into_iter().enumerate() {
        let id = e.mem.u32(item + FORM_ID);
        vars::set_element(e, array, &Key::Number(i as f64), Value::Form(id));
    }
    a.set_result(e, f64::from(array));
    handled(true)
}

#[cfg(test)]
#[path = "equipped_items_tests.rs"]
mod tests;
