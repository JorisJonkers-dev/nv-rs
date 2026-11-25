// SPDX-License-Identifier: GPL-3.0-only
//! JohnnyGuitar NVSE's body-part string traits (group 5 of the profile's missing commands: 2 calls
//! in one plugin). Written from the command's GECK Wiki page (see `NOTICE`,
//! `provenance/05-body-parts.md`); no plugin code was read.
//!
//! - `GetBodyPartTraitString bodyPartData type trait`: one of the five names of one body part of
//!   a body part data form. The part (`type`, 0 torso to 14 weapon) is the form's `bodyParts[type]`
//!   (`BGSBodyPartData` +0x34, fifteen pointers); the trait is one of the part's `BSStringT` names
//!   (`BGSBodyPart`, engine-fallout-forms `bgsbodypart.rs`): 1 part node (`NodeName` +0x04),
//!   2 V.A.T.S. target (`TargetName` +0x0C), 3 start node (`IKStartNodeName` +0x14), 4 part name
//!   (`PartName` +0x1C), 5 target bone (`GoreObjectName` +0x24, the gore effects' target bone).
//!
//! A form that is not body part data, a type outside 0 to 14, a part the form does not have, or a
//! trait outside 1 to 5 answers an empty string (the page says nothing of these cases).

use crate::commands::Cmd;
use crate::identity::OPCODE_BASE;
use engine_core::prelude::*;
use nvse::commands::string::assign_to_string_var;
use nvse::commands::Read;
use nvse::table::{Param, Returns};
use nvse::{handled, CommandArgs};

/// `kParamType_Integer`, `_AnyForm`.
const INT: u32 = 0x01;
const ANY_FORM: u32 = 0x3d;

const FORM_INT_INT: &[Param] = &[
    Param {
        name: "form",
        kind: ANY_FORM,
        optional: false,
    },
    Param {
        name: "int",
        kind: INT,
        optional: false,
    },
    Param {
        name: "int",
        kind: INT,
        optional: false,
    },
];

/// The commands, in the order of their opcodes.
pub static COMMANDS: [Cmd; 1] = [Cmd {
    opcode: OPCODE_BASE + 0x92,
    name: "GetBodyPartTraitString",
    short: "",
    params: FORM_INT_INT,
    execute: get_body_part_trait_string,
    eval: None,
    needs_ref: false,
    returns: Returns::String,
}];

/// `TESForm::typeID` (+4) of a body part data form.
const FORM_TYPE: u32 = 4;
const TYPE_BODY_PART_DATA: u8 = 0x57;

/// `BGSBodyPartData::bodyParts[15]`.
const BODY_PARTS: u32 = 0x34;
const PART_COUNT: u32 = 15;

/// The place in `BGSBodyPart` of the `BSStringT` of each trait id 1 to 5.
fn name_at(trait_id: u32) -> Option<u32> {
    Some(match trait_id {
        1 => 0x04,
        2 => 0x0c,
        3 => 0x14,
        4 => 0x1c,
        5 => 0x24,
        _ => return None,
    })
}

/// The text of the trait, or `None` when the form, part or trait is not there.
fn trait_text(e: &Engine, form: u32, part: u32, trait_id: u32) -> Option<Vec<u8>> {
    if form == 0 || e.mem.u8(form + FORM_TYPE) != TYPE_BODY_PART_DATA || part >= PART_COUNT {
        return None;
    }
    let at = name_at(trait_id)?;
    let body_part = e.mem.u32(form + BODY_PARTS + 4 * part);
    if body_part == 0 {
        return None;
    }
    let text = e.mem.u32(body_part + at);
    if text == 0 {
        return None;
    }
    Some(nvse::table::guest_text(e, text))
}

fn get_body_part_trait_string(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    let text = if read.ok {
        trait_text(
            e,
            read.form(0),
            read.int(1, u32::MAX),
            read.int(2, u32::MAX),
        )
    } else {
        None
    };
    assign_to_string_var(e, &a, &text.unwrap_or_default());
    handled(true)
}

#[cfg(test)]
#[path = "body_parts_tests.rs"]
mod tests;
