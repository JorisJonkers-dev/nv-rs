// SPDX-License-Identifier: GPL-3.0-only
//! ShowOff xNVSE's `DisableAlt` (with group 9 of the profile's missing commands). Written from
//! the command's GECK Wiki page (see `NOTICE`, `provenance/09-enable-parent-equipped-weapon.md`);
//! no plugin code was read.
//!
//! - `ref.DisableAlt [fadeOut]` (alias `SnigOff`): disables the reference as the game's own
//!   `Disable` does (`Script::DisableFunction`, `005c45e0`), without its one limit: a reference
//!   with an enable parent is disabled too, where `Disable` only warns. The page says the two
//!   are otherwise identical, so the steps are the game's: a reference already disabled
//!   (form flag `0x800`) or with form flag `0x4000` is left alone; with a positive `fadeOut` its
//!   process gets mode 9 and its fade flag (extra data `0x92`, bit 8) is set; xNVSE's `OnDisable`
//!   seam runs (`0x005C47B6`); and the reference is queued as pending-disabled
//!   (`Script::AddPendingDisabledReference`, `005aa500`) with the fade flag.
//!
//! The game's `Disable` also sets the calling script's event list flag 0 (disabled); a command
//! of a plugin has the same event list, so that is done too.

use crate::commands::Cmd;
use crate::identity::OPCODE_BASE;
use engine_core::prelude::*;
use nvse::commands::{dynamic_cast, rtti, Read};
use nvse::table::{Param, Returns};
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
    opcode: OPCODE_BASE + 0x50,
    name: "DisableAlt",
    short: "SnigOff",
    params: ONE_OPTIONAL_INT,
    execute: disable_alt,
    eval: None,
    needs_ref: true,
    expression: false,
    returns: Returns::Default,
}];

/// `TESForm::flags` (+8): disabled, and the flag `Disable` also leaves alone.
const FORM_FLAGS: u32 = 8;
const DISABLED: u32 = 0x800;
const FLAG_4000: u32 = 0x4000;
/// The reference virtual that answers its process (`+0x1D0`), the process virtual that answers
/// its process object (`+0x10`), and the process object's mode setter (`thiscall`, the mode).
pub const REFR_GET_PROCESS: u32 = 0x1d0;
pub const PROCESS_GET_OBJECT: u32 = 0x10;
pub const SET_PROCESS_MODE: u32 = 0x0070_5b10;
/// The fade flag of extra data `0x92`: test and set (`thiscall`; the setter takes the flag).
pub const HAS_FADE_FLAG: u32 = 0x0056_5090;
pub const SET_FADE_FLAG: u32 = 0x0056_50d0;
/// xNVSE's `OnDisable` seam in the game's `Disable`.
pub const ON_DISABLE_SITE: u32 = 0x005c_47b6;
/// `Script::AddPendingDisabledReference` (`cdecl`: the reference, the fade flag).
pub const ADD_PENDING_DISABLED: u32 = 0x005a_a500;

/// Disables `refr` as the game's `Disable` does, enable parent or not.
pub fn disable(e: &mut Engine, refr: u32, fade: i32) {
    if e.mem.u32(refr + FORM_FLAGS) & (DISABLED | FLAG_4000) != 0 {
        return;
    }
    if fade > 0 {
        let process = e.vcall(refr, REFR_GET_PROCESS, &args![]).u32();
        if process != 0 {
            let object = e.vcall(process, PROCESS_GET_OBJECT, &args![]).u32();
            if object != 0 {
                e.call(SET_PROCESS_MODE, &args![object, 9u32]);
            }
        }
        if !e.call(HAS_FADE_FLAG, &args![refr]).bool() {
            e.call(SET_FADE_FLAG, &args![refr, 1u32]);
        }
    }
    e.site(ON_DISABLE_SITE, &[refr]);
    e.call(ADD_PENDING_DISABLED, &args![refr, u32::from(fade != 0)]);
}

/// `ref.DisableAlt [fadeOut]`.
fn disable_alt(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if !read.ok {
        return handled(true);
    }
    let fade = read.int(0, 0) as i32;
    let refr = if a.this_obj == 0 {
        0
    } else {
        dynamic_cast(e, a.this_obj, rtti::TES_FORM, rtti::TES_OBJECT_REFR)
    };
    if refr != 0 {
        disable(e, refr, fade);
    }
    if a.event_list != 0 {
        let flags = e.mem.u8(a.event_list + 4);
        e.mem.set_u8(a.event_list + 4, flags | 1);
    }
    handled(true)
}

#[cfg(test)]
#[path = "disable_alt_tests.rs"]
mod tests;
