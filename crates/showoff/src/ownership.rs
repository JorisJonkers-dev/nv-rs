// SPDX-License-Identifier: GPL-3.0-only
//! ShowOff xNVSE's `SetOwnershipTemp` (group 5 of the profile's missing commands: ownership;
//! 38 calls in one plugin of the profile). Written from the command's GECK Wiki page (see
//! `NOTICE`, `provenance/05-ownership.md`); no plugin code was read.
//!
//! `reference.SetOwnershipTemp [newOwner] [cell]`: sets the owner of the calling reference, or,
//! with a cell given, of that cell, to the NPC or faction base form `newOwner` (a reference is
//! taken as its base form; omitted, the player's base form). "Session persistent as opposed to
//! savebaked": the owner is written into the extra data list as the game's own `SetOwnership`
//! writes it, but the form is not marked as changed, so no save carries the change. 1 for
//! success, 0 when there is neither a calling reference nor a cell.

use crate::commands::Cmd;
use crate::identity::OPCODE_BASE;
use engine_core::prelude::*;
use nvse::commands::{try_get_refr_parent, Read};
use nvse::invref::REFR_EXTRA;
use nvse::table::{Param, Returns};
use nvse::{handled, CommandArgs};

/// `kParamType_AnyForm`.
const ANY_FORM: u32 = 0x3d;

const TWO_FORMS: &[Param] = &[
    Param {
        name: "newOwner",
        kind: ANY_FORM,
        optional: true,
    },
    Param {
        name: "cell",
        kind: ANY_FORM,
        optional: true,
    },
];

/// The commands, in the order of their opcodes.
pub static COMMANDS: [Cmd; 1] = [Cmd {
    opcode: OPCODE_BASE + 0x7d,
    name: "SetOwnershipTemp",
    short: "",
    params: TWO_FORMS,
    execute: set_ownership_temp,
    eval: None,
    needs_ref: false,
    expression: false,
    returns: Returns::Default,
}];

/// `ExtraDataList::SetOwner` (`0x00419700`, thiscall `(list, owner)`): a null owner removes the
/// owner.
const EXTRA_SET_OWNER: u32 = 0x0041_9700;
/// The player character (`0x011dea3c`), whose base form is the default owner.
const PLAYER: u32 = 0x011d_ea3c;
/// `TESObjectCELL`'s own extra data list, and `TESForm::typeID` (`+4`) of a cell (`0x39`).
const CELL_EXTRA: u32 = 0x28;
const FORM_TYPE: u32 = 4;
const TYPE_CELL: u8 = 0x39;

fn set_ownership_temp(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if !read.ok {
        return handled(true);
    }
    let cell = read.form(1);
    let list = if cell != 0 && e.mem.u8(cell + FORM_TYPE) == TYPE_CELL {
        cell + CELL_EXTRA
    } else if a.this_obj != 0 {
        a.this_obj + REFR_EXTRA
    } else {
        return handled(true);
    };
    let owner = match read.form(0) {
        0 => {
            let player = e.mem.u32(PLAYER);
            if player == 0 {
                0
            } else {
                try_get_refr_parent(e, player)
            }
        }
        form => try_get_refr_parent(e, form),
    };
    e.call(EXTRA_SET_OWNER, &args![list, owner]);
    a.set_result(e, 1.0);
    handled(true)
}

#[cfg(test)]
#[path = "ownership_tests.rs"]
mod tests;
