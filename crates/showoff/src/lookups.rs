// SPDX-License-Identifier: GPL-3.0-only
//! ShowOff xNVSE's `IsBaseForm` (the rest of group 2 of the profile's missing commands; 5 calls
//! in 4 plugins). Written from the command's GECK Wiki page (see `NOTICE`,
//! `provenance/03-is-base-form.md`); no plugin code was read.
//!
//! `IsBaseForm form`: "returns True (1) if the passed ref variable stores a valid BaseForm, False
//! (0) otherwise". A valid form that is not a reference (`TESObjectREFR` or a class below it) is
//! a base form; no form, a form id nothing has, and a reference all answer 0.

use crate::commands::Cmd;
use crate::identity::OPCODE_BASE;
use engine_core::prelude::*;
use nvse::commands::{dynamic_cast, rtti, Read};
use nvse::table::{Param, Returns};
use nvse::{handled, CommandArgs};

/// `kParamType_AnyForm`.
const ANY_FORM: u32 = 0x3d;

const ONE_FORM: &[Param] = &[Param {
    name: "refVar",
    kind: ANY_FORM,
    optional: false,
}];

/// The commands, in the order of their opcodes.
pub static COMMANDS: [Cmd; 1] = [Cmd {
    opcode: OPCODE_BASE + 0x41,
    name: "IsBaseForm",
    short: "IsBasedForm",
    params: ONE_FORM,
    execute: is_base_form,
    expression: false,
    returns: Returns::Default,
}];

fn is_base_form(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if read.ok && read.form(0) != 0 {
        let form = read.form(0);
        let refr = dynamic_cast(e, form, rtti::TES_FORM, rtti::TES_OBJECT_REFR);
        a.set_result(e, if refr == 0 { 1.0 } else { 0.0 });
    }
    handled(true)
}

#[cfg(test)]
#[path = "lookups_tests.rs"]
mod tests;
