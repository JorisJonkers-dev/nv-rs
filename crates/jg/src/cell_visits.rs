// SPDX-License-Identifier: GPL-3.0-only
//! JohnnyGuitar NVSE's `IsCellVisited` (group 5 of the profile's missing commands, with the
//! body-part traits: 2 calls in one plugin). Written from the command's GECK Wiki page (see
//! `NOTICE`, `provenance/05-body-parts.md`); no plugin code was read.
//!
//! - `IsCellVisited cell`: 1 when the player has visited the cell, else 0. The page's note says it
//!   answers 0 on the first visit when asked from an event that runs before the cell's seen data
//!   is updated, so the answer is whether the cell has seen data: the game's own
//!   `ExtraDataList::GetSeenData` of the cell (`00555bc0`), which the game fills on the first
//!   visit (the local map's seen data, interior and exterior kinds).
//!
//! A form that is not a cell answers 0.

use crate::commands::Cmd;
use crate::identity::OPCODE_BASE;
use engine_core::prelude::*;
use nvse::commands::Read;
use nvse::table::{Param, Returns};
use nvse::{handled, CommandArgs};

/// `kParamType_AnyForm`.
const ANY_FORM: u32 = 0x3d;

const ONE_FORM: &[Param] = &[Param {
    name: "form",
    kind: ANY_FORM,
    optional: false,
}];

/// The commands, in the order of their opcodes.
pub static COMMANDS: [Cmd; 1] = [Cmd {
    opcode: OPCODE_BASE + 0x05,
    name: "IsCellVisited",
    short: "",
    params: ONE_FORM,
    execute: is_cell_visited,
    eval: None,
    needs_ref: false,
    returns: Returns::Default,
}];

/// `TESForm::typeID` (+4) of a cell.
const FORM_TYPE: u32 = 4;
const TYPE_CELL: u8 = 0x39;

/// `ExtraDataList::GetSeenData` of a cell (`(cell)`): its seen data, or 0.
pub const CELL_SEEN_DATA: u32 = 0x0055_5bc0;

fn is_cell_visited(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if read.ok {
        let cell = read.form(0);
        if cell != 0 && e.mem.u8(cell + FORM_TYPE) == TYPE_CELL {
            let seen = e.call(CELL_SEEN_DATA, &args![cell]).u32();
            a.set_result(e, if seen != 0 { 1.0 } else { 0.0 });
        }
    }
    handled(true)
}

#[cfg(test)]
#[path = "cell_visits_tests.rs"]
mod tests;
