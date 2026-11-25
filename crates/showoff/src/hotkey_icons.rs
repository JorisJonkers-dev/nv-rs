// SPDX-License-Identifier: GPL-3.0-only
//! ShowOff xNVSE's `GetItemHotkeyIconPath` (group 8 of the profile's missing commands). Written
//! from the command's GECK Wiki page (see `NOTICE`, `provenance/08-hotkey-icon-path.md`); no
//! plugin code was read.
//!
//! - `GetItemHotkeyIconPath item [onlyOverride]`: the hotkey icon override `SetItemHotkeyIconPath`
//!   set for the item, else (without `onlyOverride`) the item's own texture path, as
//!   `GetTexturePath` answers it. `SetItemHotkeyIconPath` is not built, so no override exists:
//!   the command answers the texture path, or an empty string with `onlyOverride` 1. A reference
//!   stands for its base form.

use crate::commands::Cmd;
use crate::identity::OPCODE_BASE;
use engine_core::prelude::*;
use nvse::commands::string::assign_to_string_var;
use nvse::commands::{dynamic_cast, form_or_caller, rtti, Read};
use nvse::table::{Param, Returns};
use nvse::{handled, CommandArgs};

/// `kParamType_AnyForm`, `_Integer`.
const ANY_FORM: u32 = 0x3d;
const INT: u32 = 0x01;

const FORM_AND_OPTIONAL_INT: &[Param] = &[
    Param {
        name: "form",
        kind: ANY_FORM,
        optional: false,
    },
    Param {
        name: "int",
        kind: INT,
        optional: true,
    },
];

/// The commands, in the order of their opcodes.
pub static COMMANDS: [Cmd; 1] = [Cmd {
    opcode: OPCODE_BASE + 0xbb,
    name: "GetItemHotkeyIconPath",
    short: "",
    params: FORM_AND_OPTIONAL_INT,
    execute: get_item_hotkey_icon_path,
    eval: None,
    needs_ref: false,
    expression: false,
    returns: Returns::String,
}];

/// `TESTexture`'s type descriptor (the part of an item that holds its icon, `TESIcon` derives
/// from it), and its path: a `BSStringT` (`m_data`, the text) at `+4`.
pub const RTTI_TES_TEXTURE: u32 = 0x0118_3218;
pub const TEXTURE_PATH: u32 = 4;

/// The item's texture path, as `GetTexturePath` reads it; empty when it has none.
pub fn texture_path(e: &mut Engine, form: u32) -> Vec<u8> {
    let texture = dynamic_cast(e, form, rtti::TES_FORM, RTTI_TES_TEXTURE);
    if texture == 0 {
        return Vec::new();
    }
    let text = e.mem.u32(texture + TEXTURE_PATH);
    if text == 0 {
        Vec::new()
    } else {
        e.mem.cstr(text).to_vec()
    }
}

fn get_item_hotkey_icon_path(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    let mut path = Vec::new();
    if read.ok {
        let only_override = read.int(1, 0) != 0;
        if let Some(item) = form_or_caller(e, read.form(0), a.this_obj) {
            // No override can exist (`SetItemHotkeyIconPath` is not built).
            if !only_override && item != 0 {
                path = texture_path(e, item);
            }
        }
    }
    assign_to_string_var(e, &a, &path);
    handled(true)
}

#[cfg(test)]
#[path = "hotkey_icons_tests.rs"]
mod tests;
