// SPDX-License-Identifier: GPL-3.0-only
//! JohnnyGuitar NVSE's editor-id lookups: `EditorIDToFormID` and `GetEditorID` (group 2 of the
//! profile's missing commands; `EditorIDToFormID` is the most called missing command, 858 calls
//! in 17 plugins). Written from the commands' GECK Wiki pages (see `NOTICE`); no plugin code was
//! read.
//!
//! - `EditorIDToFormID "editorID"`: "the Form ID associated with a specified Editor ID string
//!   (case-sensitive)". Answers the form (a reference to the base form), 0 for a name no form
//!   has. The lookup is the game's own `GetFormByEditorID` (`00483A00`), which the built-in
//!   editor-id store answers for every form when its row is on (`crates/johnnyguitar`); the
//!   store matches without case, so the found form's own name is compared exactly and a name that
//!   differs in case answers 0.
//! - `GetEditorID form`: "the Editor ID of a form as a string; if a reference is passed instead
//!   of a base form, the editor ID of that specific reference; if none was set, an empty string".
//!   Asked of the form's `GetName`-slot virtual function (`+0x130`), which is what the store's
//!   getter replaces. The form is optional in the plugin's parameter list; the page does not say
//!   what a call without one means, so it answers the calling reference's, and an empty string
//!   when there is none either.

use crate::commands::Cmd;
use crate::identity::OPCODE_BASE;
use engine_core::prelude::*;
use nvse::commands::string::assign_to_string_var;
use nvse::commands::Read;
use nvse::table::{Param, Returns};
use nvse::{handled, CommandArgs};

/// `kParamType_String`, `_AnyForm`.
const STRING: u32 = 0x00;
const ANY_FORM: u32 = 0x3d;

const ONE_STRING: &[Param] = &[Param {
    name: "string",
    kind: STRING,
    optional: false,
}];
const ONE_OPTIONAL_FORM: &[Param] = &[Param {
    name: "form",
    kind: ANY_FORM,
    optional: true,
}];

/// The commands, in the order of their opcodes.
pub static COMMANDS: [Cmd; 2] = [
    Cmd {
        opcode: OPCODE_BASE + 0x14,
        name: "GetEditorID",
        short: "GetEdID",
        params: ONE_OPTIONAL_FORM,
        execute: get_editor_id,
        eval: None,
        returns: Returns::String,
    },
    Cmd {
        opcode: OPCODE_BASE + 0x28,
        name: "EditorIDToFormID",
        short: "",
        params: ONE_STRING,
        execute: editor_id_to_form_id,
        eval: None,
        returns: Returns::Form,
    },
];

/// `TESForm::GetFormByEditorID` (`cdecl`: the name, the form or 0).
const FORM_BY_EDITOR_ID: u32 = 0x0048_3a00;
/// `TESForm`'s `refID`, and its virtual `GetName` that answers the editor id.
const FORM_ID: u32 = 0xc;
const GET_EDITOR_ID_SLOT: u32 = 0x130;

/// The form's editor id as the game answers it; empty for none.
fn editor_id_of(e: &mut Engine, form: u32) -> Vec<u8> {
    if form == 0 {
        return Vec::new();
    }
    let at = e.vcall(form, GET_EDITOR_ID_SLOT, &args![]).u32();
    if at == 0 {
        Vec::new()
    } else {
        nvse::table::guest_text(e, at)
    }
}

/// The form named exactly `name`: 0 for none, an empty name, or a name another case spells.
fn form_by_exact_name(e: &mut Engine, name: &[u8]) -> u32 {
    if name.is_empty() {
        return 0;
    }
    let size = (name.len() as u32 + 1).next_multiple_of(4);
    let form = e.with_stack(size, |e, buffer| {
        e.mem.write(buffer.addr(), name);
        e.mem.set_u8(buffer.addr() + name.len() as u32, 0);
        e.call(FORM_BY_EDITOR_ID, &args![buffer.addr()]).u32()
    });
    if form != 0 && editor_id_of(e, form) == name {
        form
    } else {
        0
    }
}

fn editor_id_to_form_id(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_form_result(e, 0);
    let read = Read::of(e, &a);
    if read.ok {
        let name = read.text(0);
        let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
        let form = form_by_exact_name(e, &name[..end]);
        if form != 0 {
            let id = e.mem.u32(form + FORM_ID);
            a.set_form_result(e, id);
        }
    }
    handled(true)
}

fn get_editor_id(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    let form = if read.ok && read.form(0) != 0 {
        read.form(0)
    } else {
        a.this_obj
    };
    let name = editor_id_of(e, form);
    assign_to_string_var(e, &a, &name);
    handled(true)
}

#[cfg(test)]
#[path = "lookups_tests.rs"]
mod tests;
