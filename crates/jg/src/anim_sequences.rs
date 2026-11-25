// SPDX-License-Identifier: GPL-3.0-only
//! JohnnyGuitar NVSE's `GetSequenceAnimGroup` (with group 5 of the profile's missing commands: 2
//! calls in one plugin). Written from the command's GECK Wiki page (see `NOTICE`,
//! `provenance/05-body-parts.md`); no plugin code was read.
//!
//! - `ref.GetSequenceAnimGroup sequence`: the anim group of the sequence the reference plays in
//!   one of its eight sequence slots (0 idle, 1 movement, 2 left arm, 3 left hand, 4 weapon, 5
//!   weapon up, 6 weapon down, 7 special idle). The reference's animation (`GetAnimData`, the
//!   reference virtual `+0x1E4`), its current sequence of that slot (`Animation::pCurrentSequence`
//!   `+0xE0`, eight pointers), the sequence's group (`BSAnimGroupSequence::spAnimGroup` `+0x74`)
//!   and the group's id (`TESAnimGroup` `+0x10`). The answer is the id's low byte, the anim group
//!   as the page's list of anim groups numbers it (the high byte is the weapon kind and hand).
//!
//! A slot out of range, a reference with no animation or a slot with no sequence answers -1, so
//! no answer can be taken for the idle group 0.

use crate::commands::Cmd;
use crate::identity::OPCODE_BASE;
use engine_core::prelude::*;
use nvse::commands::Read;
use nvse::table::{Param, Returns};
use nvse::{handled, CommandArgs};

/// `kParamType_Integer`.
const INT: u32 = 0x01;

const ONE_INT: &[Param] = &[Param {
    name: "int",
    kind: INT,
    optional: false,
}];

/// The commands, in the order of their opcodes.
pub static COMMANDS: [Cmd; 1] = [Cmd {
    opcode: OPCODE_BASE + 0x6f,
    name: "GetSequenceAnimGroup",
    short: "",
    params: ONE_INT,
    execute: get_sequence_anim_group,
    eval: None,
    needs_ref: true,
    returns: Returns::Default,
}];

/// `TESObjectREFR::GetAnimData` (virtual, byte offset in the table).
pub const REFR_GET_ANIM_DATA: u32 = 0x1e4;
/// `Animation::pCurrentSequence[8]`, `BSAnimGroupSequence::spAnimGroup`, the group's id.
pub const CURRENT_SEQUENCES: u32 = 0xe0;
pub const SEQUENCE_ANIM_GROUP: u32 = 0x74;
pub const ANIM_GROUP_ID: u32 = 0x10;
/// The eight sequence slots.
const SLOTS: u32 = 8;

/// The anim group the reference plays in `slot`, or `None`.
pub fn sequence_anim_group(e: &mut Engine, refr: u32, slot: u32) -> Option<u8> {
    if refr == 0 || slot >= SLOTS {
        return None;
    }
    let animation = e.vcall(refr, REFR_GET_ANIM_DATA, &[]).u32();
    if animation == 0 {
        return None;
    }
    let sequence = e.mem.u32(animation + CURRENT_SEQUENCES + slot * 4);
    if sequence == 0 {
        return None;
    }
    let group = e.mem.u32(sequence + SEQUENCE_ANIM_GROUP);
    if group == 0 {
        return None;
    }
    Some(e.mem.u16(group + ANIM_GROUP_ID) as u8)
}

fn get_sequence_anim_group(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, -1.0);
    let read = Read::of(e, &a);
    if read.ok {
        let slot = read.int(0, u32::MAX);
        if let Some(group) = sequence_anim_group(e, a.this_obj, slot) {
            a.set_result(e, f64::from(group));
        }
    }
    handled(true)
}

#[cfg(test)]
#[path = "anim_sequences_tests.rs"]
mod tests;
