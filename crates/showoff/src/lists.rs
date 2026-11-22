// SPDX-License-Identifier: GPL-3.0-only
//! ShowOff xNVSE's form-list commands: `ListAddList` and `ListAddArray` (group 3 of the
//! profile's missing commands; `ListAddList` is called 264 times in 3 plugins). Written from the
//! commands' GECK Wiki pages (see `NOTICE`, `provenance/01-lists.md`); no plugin code was read.
//!
//! - `ListAddList destination source [index]` (short name `AddFormListToFormList`): "adds the
//!   forms from sourceList to destinationList at the given index (or at the end if not
//!   provided). The contents are appended in the order they appear in sourceList. Returns 1 for
//!   success, 0 if an error is encountered", and "does the same changes to the formlist as
//!   multiple ListAddForms would", so the changes are not savebaked.
//! - `ListAddArray list array [index]` (short name `AddArrayToFormList`): the same for the forms
//!   of an array, "appended in the array key order"; "non-forms and invalid forms are skipped".
//!
//! Both add each form as xNVSE's `ListAddForm` adds one without its duplicate check
//! ([`nvse::commands::list::script_add_form`]). What the pages leave open is decided in
//! `provenance/01-lists.md`: the order is kept at an index (the first form at the index, the next
//! after it), a negative index is the end, an empty source or array is a success, a list added to
//! itself is read before it is added to (so it doubles), and a failed add stops the run and
//! answers 0, the forms before it staying.

use crate::commands::Cmd;
use crate::identity::OPCODE_BASE;
use engine_core::prelude::*;
use nvse::commands::Read;
use nvse::expr::{self, Eval, Ty};
use nvse::table::{Param, Returns};
use nvse::vars::{self, Container, Data};
use nvse::{handled, CommandArgs};

/// `kParamType_Integer`, `_AnyForm`; `kNVSEParamType_Number`, `_Form`, `_Array`.
const INT: u32 = 0x01;
const ANY_FORM: u32 = 0x3d;
const NV_NUMBER: u32 = 0x8001;
const NV_FORM: u32 = 0x8008;
const NV_ARRAY: u32 = 0x8040;

const fn p(name: &'static str, kind: u32, optional: bool) -> Param {
    Param {
        name,
        kind,
        optional,
    }
}

/// The parameters as the page's syntax lines give them: two base forms and an optional integer;
/// a base form, an array and an optional number.
const LIST_LIST_INDEX: &[Param] = &[
    p("destinationList", ANY_FORM, false),
    p("sourceList", ANY_FORM, false),
    p("index", INT, true),
];
const LIST_ARRAY_INDEX: &[Param] = &[
    p("formlist", NV_FORM, false),
    p("array", NV_ARRAY, false),
    p("index", NV_NUMBER, true),
];

/// The commands, in the order of their opcodes.
pub static COMMANDS: [Cmd; 2] = [
    Cmd {
        opcode: OPCODE_BASE + 0x1a,
        name: "ListAddList",
        short: "AddFormListToFormList",
        params: LIST_LIST_INDEX,
        execute: list_add_list,
        eval: None,
        needs_ref: false,
        expression: false,
        returns: Returns::Default,
    },
    Cmd {
        opcode: OPCODE_BASE + 0x89,
        name: "ListAddArray",
        short: "AddArrayToFormList",
        params: LIST_ARRAY_INDEX,
        execute: list_add_array,
        eval: None,
        needs_ref: false,
        expression: true,
        returns: Returns::Default,
    },
];

/// `eListEnd`: the index that means the end of the list.
const LIST_END: i32 = -2;
/// `BGSListForm`: its type byte (`kFormType_BGSListForm`), and its forms (a `tList` at `+0x18`).
const FORM_TYPE: u32 = 4;
const TYPE_LIST: u8 = 0x55;
const LIST_FORMS: u32 = 0x18;
/// `LookupFormByID` (`cdecl`: the id, the form or 0).
const LOOKUP_FORM: u32 = 0x0048_39c0;

fn is_list(e: &Engine, form: u32) -> bool {
    form != 0 && e.mem.u8(form + FORM_TYPE) == TYPE_LIST
}

/// `forms` added to `list` one by one, at the end or from `index` on (a negative index is the
/// end). False when an add fails (no such place); the forms before it stay.
fn add_forms(e: &mut Engine, list: u32, forms: &[u32], index: i32) -> bool {
    let mut at = if index < 0 { LIST_END } else { index };
    for &form in forms {
        let went = nvse::commands::list::script_add_form(e, list, form, at);
        if went < 0 {
            return false;
        }
        if at != LIST_END {
            at = went.saturating_add(1);
        }
    }
    true
}

/// `ListAddList`: the forms of `source` (as they are when the call starts) added to
/// `destination`; false unless both are form lists.
fn add_list(e: &mut Engine, destination: u32, source: u32, index: i32) -> bool {
    if !is_list(e, destination) || !is_list(e, source) {
        return false;
    }
    let forms = nvse::invref::list_items(e, source + LIST_FORMS);
    add_forms(e, destination, &forms, index)
}

fn list_add_list(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if read.ok {
        let index = read.int(2, LIST_END as u32) as i32;
        let done = add_list(e, read.form(0), read.form(1), index);
        a.set_result(e, f64::from(u8::from(done)));
    }
    handled(true)
}

/// The array's elements in key order.
fn elements(e: &Engine, array: u32) -> Option<Vec<Data>> {
    let a = vars::array(e, array)?;
    Some(match &a.elements {
        Container::Array(v) => v.iter().map(|x| x.data.clone()).collect(),
        Container::NumMap(v) => v.iter().map(|(_, x)| x.data.clone()).collect(),
        Container::StrMap(v) => v.iter().map(|(_, x)| x.data.clone()).collect(),
    })
}

/// `ListAddArray`'s arguments evaluated: the list, the array and the index; false for anything
/// the page does not allow (a first argument that is not a form list, an array that does not
/// exist, an index that is not a number).
fn add_array(e: &mut Engine, ev: &Eval, toks: &[expr::Tok]) -> bool {
    if !(2..=3).contains(&toks.len()) {
        return false;
    }
    if !ev.can_convert(e, &toks[0], Ty::Form) || !ev.can_convert(e, &toks[1], Ty::Array) {
        return false;
    }
    let list = ev.get_form(e, &toks[0]);
    let array = ev.get_array_id(e, &toks[1]);
    let index = match toks.get(2) {
        None => LIST_END,
        Some(t) if ev.can_convert(e, t, Ty::Number) => ev.get_number(e, t) as i32,
        Some(_) => return false,
    };
    let Some(items) = elements(e, array) else {
        return false;
    };
    if !is_list(e, list) {
        return false;
    }
    let mut forms = Vec::new();
    for item in items {
        if let Data::Form(id) = item {
            let form = e.call(LOOKUP_FORM, &args![id]).u32();
            if form != 0 {
                forms.push(form);
            }
        }
    }
    add_forms(e, list, &forms, index)
}

fn list_add_array(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let mut ev = Eval::new(e, &a);
    expr::with_evaluator(e, |e| {
        if let Some(toks) = ev.extract_args(e) {
            let done = add_array(e, &ev, &toks);
            a.set_result(e, f64::from(u8::from(done)));
        }
    });
    expr::report(e, &ev);
    handled(true)
}

#[cfg(test)]
#[path = "lists_tests.rs"]
mod tests;
