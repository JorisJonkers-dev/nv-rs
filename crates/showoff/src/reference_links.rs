// SPDX-License-Identifier: GPL-3.0-only
//! ShowOff xNVSE's `SetEnableParent` and `GetEquippedWeapon` (group 9 of the profile's missing
//! commands). Written from the commands' GECK Wiki pages (see `NOTICE`,
//! `provenance/09-enable-parent-equipped-weapon.md`); no plugin code was read.
//!
//! - `ref.SetEnableParent [newParent]` (alias `SetParentRef`): makes `newParent` the calling
//!   reference's enable parent; without one (or null), removes the enable parent. The work is the
//!   game's own: `0056aa10` (leave the old parent's child list, store the new parent; the game
//!   keeps only a persistent parent) and, as the game's own copy of a reference does
//!   (`0055b9c0`), `0041dcd0` on the new parent's extra data (join its child list), so the parent's
//!   later enable and disable reach the reference. A parent that is the reference or has it
//!   above it in its own enable-parent chain is refused: such a loop never ends in the game's walk.
//! - `actor.GetEquippedWeapon`: the base form of the actor's current weapon
//!   (`Actor::GetCurrentWeapon`, `008a1710`); with none, the game's fists weapon (`011ca278`).
//!   Another kind of reference answers 0.

use crate::commands::Cmd;
use crate::identity::OPCODE_BASE;
use engine_core::prelude::*;
use nvse::commands::{dynamic_cast, rtti, Read};
use nvse::table::{Param, Returns};
use nvse::{handled, CommandArgs};

/// `kParamType_AnyForm`.
const ANY_FORM: u32 = 0x3d;

const ONE_OPTIONAL_FORM: &[Param] = &[Param {
    name: "form",
    kind: ANY_FORM,
    optional: true,
}];

/// The commands, in the order of their opcodes.
pub static COMMANDS: [Cmd; 2] = [
    Cmd {
        opcode: OPCODE_BASE + 0x9d,
        name: "SetEnableParent",
        short: "SetParentRef",
        params: ONE_OPTIONAL_FORM,
        execute: set_enable_parent,
        eval: None,
        needs_ref: true,
        expression: false,
        returns: Returns::Default,
    },
    Cmd {
        opcode: OPCODE_BASE + 0x9e,
        name: "GetEquippedWeapon",
        short: "",
        params: &[],
        execute: get_equipped_weapon,
        eval: None,
        needs_ref: true,
        expression: false,
        returns: Returns::Form,
    },
];

/// `TESObjectREFR`'s extra data list (`thiscall`, the list) and the enable parent stored in it.
pub const REFR_EXTRA_LIST: u32 = 0x005d_43c0;
pub const REFR_ENABLE_PARENT: u32 = 0x0056_a9f0;
/// Re-parents the enable state (`thiscall`, the new parent or null).
pub const REFR_SET_ENABLE_PARENT: u32 = 0x0056_aa10;
/// Adds a child to the enable-state child list of an extra data list (`thiscall`, the child).
pub const EXTRA_ADD_ENABLE_CHILD: u32 = 0x0041_dcd0;
/// `Actor::GetCurrentWeapon` (`thiscall`): the weapon's base form, or 0.
pub const ACTOR_GET_CURRENT_WEAPON: u32 = 0x008a_1710;
/// The game's fists weapon (`TESObjectWEAP*`).
pub const FISTS_WEAPON: u32 = 0x011c_a278;

/// `TESForm::refID`.
const FORM_ID: u32 = 0x0c;
/// How far up an enable-parent chain the loop test looks; the game's chains are a few links.
const MAX_CHAIN: usize = 256;

fn reference(e: &mut Engine, form: u32) -> u32 {
    if form == 0 {
        return 0;
    }
    dynamic_cast(e, form, rtti::TES_FORM, rtti::TES_OBJECT_REFR)
}

/// Whether making `parent` the enable parent of `child` closes a loop: `parent` is `child`, or
/// `child` is in `parent`'s own chain (a chain too long to follow counts as a loop).
fn closes_loop(e: &mut Engine, child: u32, parent: u32) -> bool {
    let mut at = parent;
    for _ in 0..MAX_CHAIN {
        if at == 0 {
            return false;
        }
        if at == child {
            return true;
        }
        at = e.call(REFR_ENABLE_PARENT, &args![at]).u32();
    }
    true
}

/// `ref.SetEnableParent [newParent]`.
fn set_enable_parent(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let child = reference(e, a.this_obj);
    if child == 0 {
        return handled(true);
    }
    let read = Read::of(e, &a);
    if !read.ok {
        return handled(true);
    }
    let given = read.form(0);
    let parent = reference(e, given);
    if given != 0 && parent == 0 {
        // Not a reference: nothing changes.
        return handled(true);
    }
    if parent != 0 && closes_loop(e, child, parent) {
        return handled(true);
    }
    e.call(REFR_SET_ENABLE_PARENT, &args![child, parent]);
    if parent != 0 && e.call(REFR_ENABLE_PARENT, &args![child]).u32() == parent {
        let list = e.call(REFR_EXTRA_LIST, &args![parent]).u32();
        if list != 0 {
            e.call(EXTRA_ADD_ENABLE_CHILD, &args![list, child]);
        }
    }
    handled(true)
}

/// `actor.GetEquippedWeapon`: the weapon's form id, the fists' without one, 0 for another kind of
/// reference.
fn get_equipped_weapon(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_form_result(e, 0);
    if a.this_obj == 0 {
        return handled(true);
    }
    let actor = dynamic_cast(e, a.this_obj, rtti::TES_FORM, rtti::ACTOR);
    if actor == 0 {
        return handled(true);
    }
    let mut weapon = e.call(ACTOR_GET_CURRENT_WEAPON, &args![actor]).u32();
    if weapon == 0 {
        weapon = e.global::<u32>(FISTS_WEAPON);
    }
    if weapon != 0 {
        let id = e.mem.u32(weapon + FORM_ID);
        a.set_form_result(e, id);
    }
    handled(true)
}

#[cfg(test)]
#[path = "reference_links_tests.rs"]
mod tests;
