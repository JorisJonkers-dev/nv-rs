//! `functions_ln/ln_fn_utility.h` (JIP LN NVSE): the commands built so far, `GetLNVersion`,
//! `ListToArray`, `GetFormCountType` and `SortFormsByType`.

use super::arrays::{create_array, Element, TempElements};
use super::forms::ListNode;
use super::{type_of, Handlers};
use crate::cosave::lookup_form;
use crate::game::BGSListForm;
use engine_core::prelude::*;
use nvse::commands::Read;
use nvse::vars::{self, Container, Data};
use nvse::{handled, CommandArgs};

#[rustfmt::skip]
pub static COMMANDS: Handlers = &[
    (0x2200, get_ln_version),
    (0x2212, list_to_array),
    (0x22ff, sort_forms_by_type),
    (0x2300, get_form_count_type),
];

/// `ADDR_ReturnTrue`: the vtable slot of a reference's +0xF0 (`IS_REFERENCE`).
const RETURN_TRUE: u32 = 0x008d_0360;

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_ln/ln_fn_utility.h:
// Cmd_GetLNVersion_Execute.
/// `JIP_LN_VERSION` (internal/version.h): 57 + 30 * 0.01, echoed to an open console.
fn get_ln_version(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 57.0 + 30.0 * 0.01);
    crate::console::print_result(e, &a);
    handled(true)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_ln/ln_fn_utility.h:
// Cmd_ListToArray_Execute.
/// The list's non-empty items as an array of forms (an empty list: an empty array).
fn list_to_array(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    let read = Read::of(e, &a);
    if read.ok {
        let list = read.form(0);
        let mut items = TempElements::new();
        let mut node = Ptr::<BGSListForm>::new(list).at(BGSListForm::list);
        while !node.is_null() {
            let form = e.get(node, ListNode::data);
            if form != 0 {
                items.append(Element::Form(form));
            }
            node = Ptr::new(e.get(node, ListNode::next));
        }
        let id = create_array(e, a.script, &items.0);
        a.set_result(e, f64::from(id));
    }
    handled(true)
}

/// The values of array `id` in order, `None` for no array.
fn values(e: &Engine, id: u32) -> Option<(bool, Vec<Data>)> {
    let array = vars::array(e, id)?;
    let packed = matches!(array.elements, Container::Array(_));
    let data = match &array.elements {
        Container::Array(v) => v.iter().map(|el| el.data.clone()).collect(),
        Container::NumMap(v) => v.iter().map(|(_, el)| el.data.clone()).collect(),
        Container::StrMap(v) => v.iter().map(|(_, el)| el.data.clone()).collect(),
    };
    Some((packed, data))
}

/// An element's form (`ArrayElementR::Form`): the form of a form element, 0 for the rest.
fn form_of(e: &Engine, d: &Data) -> u32 {
    match d {
        Data::Form(id) => lookup_form(e, *id),
        _ => 0,
    }
}

/// `IS_REFERENCE(form)`.
fn is_reference(e: &Engine, form: u32) -> bool {
    e.mem.u32(e.mem.u32(form) + 0xf0) == RETURN_TRUE
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_ln/ln_fn_utility.h:
// Cmd_GetFormCountType_Execute.
/// How many forms of a plain array have the form type (a reference counts by its own type).
fn get_form_count_type(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    let read = Read::of(e, &a);
    if read.ok {
        let (id, type_id) = (read.int(0, 0), read.int(1, 0));
        if let Some((true, data)) = values(e, id) {
            if !data.is_empty() {
                let count = data
                    .iter()
                    .map(|d| form_of(e, d))
                    .filter(|&f| f != 0 && u32::from(type_of(e, f)) == type_id)
                    .count();
                a.set_result(e, count as f64);
            }
        }
    }
    handled(true)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_ln/ln_fn_utility.h:
// Cmd_SortFormsByType_Execute.
/// The forms grouped by the order of the types array: for each type (its number's low byte),
/// the forms of that type or whose base form is of it, taken from the end of what is left (so
/// each group comes out in reverse), then the rest in their order.
fn sort_forms_by_type(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    let read = Read::of(e, &a);
    if !read.ok {
        return handled(true);
    }
    let (Some((_, forms)), Some((_, types))) =
        (values(e, read.int(0, 0)), values(e, read.int(1, 0)))
    else {
        return handled(true);
    };
    if forms.is_empty() || types.is_empty() {
        return handled(true);
    }
    let mut left: Vec<u32> = forms
        .iter()
        .map(|d| form_of(e, d))
        .filter(|&f| f != 0)
        .collect();
    let mut out = TempElements::new();
    for t in &types {
        let type_id = match t {
            Data::Number(n) => super::cvttsd2si(*n) as u8,
            _ => 0,
        };
        let mut i = left.len();
        while i > 0 {
            i -= 1;
            let form = left[i];
            let same = type_of(e, form) == type_id
                || (is_reference(e, form) && {
                    let base = e.mem.u32(form + 0x20);
                    type_of(e, base) == type_id
                });
            if same {
                out.append(Element::Form(form));
                left.remove(i);
            }
        }
        if left.is_empty() {
            break;
        }
    }
    for form in left {
        out.append(Element::Form(form));
    }
    let id = create_array(e, a.script, &out.0);
    a.set_result(e, f64::from(id));
    handled(true)
}

#[cfg(test)]
#[path = "ln_fn_utility_tests.rs"]
mod tests;
