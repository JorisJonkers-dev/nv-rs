// SPDX-License-Identifier: GPL-3.0-only
//! What the group tests share: an engine with xNVSE's table and the plugin installed, compiled
//! arguments, and a call through a command's record as `EvaluateLine` makes it.

use engine_core::prelude::*;

pub const DATA: u32 = 0x0300_0000;
pub const OFFSET: u32 = 0x0300_0f00;
pub const RESULT: u32 = 0x0300_0f08;
pub const SCRIPT: u32 = 0x0300_1000;
pub const LIST: u32 = 0x0300_2000;

/// An engine with xNVSE's table and the plugin answering "installed".
pub fn engine() -> Engine {
    let mut e = Engine::new();
    e.map(0x0118_e000, 0x9000);
    e.map(0x0300_0000, 0x10000);
    nvse::table::install(&mut e);
    crate::install_as(&mut e, true);
    e
}

/// Calls `opcode` through its record with `this_obj` and `args`: the handler's answer (`al`) and
/// the result double (it starts at 0, as the seams zero it).
pub fn call(e: &mut Engine, opcode: u32, this_obj: u32, args: &[Vec<u8>]) -> (u32, f64) {
    e.mem.set_u16(DATA, opcode as u16);
    let bytes: Vec<u8> = args.concat();
    e.mem.set_u16(DATA + 2, bytes.len() as u16 + 2);
    e.mem.set_u16(DATA + 4, args.len() as u16);
    for (i, b) in bytes.iter().enumerate() {
        e.mem.set_u8(DATA + 6 + i as u32, *b);
    }
    e.mem.set_u32(OFFSET, 4);
    e.mem.set_f64(RESULT, 0.0);
    let record = nvse::table::record_of(e, opcode);
    assert_ne!(record, 0, "{opcode:04x}");
    let params = e.mem.u32(record + 0x14);
    let execute = e.mem.u32(record + 0x18);
    let ok = e
        .call(
            execute,
            &[params, DATA, this_obj, 0, SCRIPT, LIST, RESULT, OFFSET],
        )
        .eax;
    (ok & 0xff, e.mem.f64(RESULT))
}

/// Compiled arguments: an immediate integer (`n`), a reference of the script's list (`r` and its
/// index).
pub fn n(v: i32) -> Vec<u8> {
    let mut b = vec![b'n'];
    b.extend_from_slice(&v.to_le_bytes());
    b
}
pub fn r(index: u16) -> Vec<u8> {
    let mut b = vec![b'r'];
    b.extend_from_slice(&index.to_le_bytes());
    b
}

/// Makes `form` reference `index` (from 1) of the script's reference list.
pub fn set_script_ref(e: &mut Engine, index: u16, form: u32) {
    let i = u32::from(index);
    let ref_var = 0x0300_a000 + i * 0x10;
    e.mem.set_u32(ref_var + 8, form);
    e.mem.set_u32(ref_var + 0xc, 0);
    let mut node = SCRIPT + 0x44;
    for at in 1..i {
        let next = e.mem.u32(node + 4);
        node = if next == 0 {
            let made = 0x0300_b000 + (at + 1) * 8;
            e.mem.set_u32(node + 4, made);
            made
        } else {
            next
        };
    }
    e.mem.set_u32(node, ref_var);
}

/// Appends `node` to a list whose head is at `head`: a pointer to the first node, or (with
/// `embedded`) the first node itself.
fn append(e: &mut Engine, head: u32, node: u32, embedded: bool) {
    let mut at = if embedded {
        if e.mem.u32(head) == 0 {
            e.mem.set_u32(head, e.mem.u32(node));
            return;
        }
        head
    } else {
        let first = e.mem.u32(head);
        if first == 0 {
            e.mem.set_u32(head, node);
            return;
        }
        first
    };
    while e.mem.u32(at + 4) != 0 {
        at = e.mem.u32(at + 4);
    }
    e.mem.set_u32(at + 4, node);
}

/// Adds local `id` named `name` to the script's variable list and the event list, holding
/// `value`.
pub fn add_local(e: &mut Engine, id: u16, name: &str, value: f64) {
    let id32 = u32::from(id);
    let local = 0x0300_6000 + id32 * 0x20;
    e.mem.set_u32(local, id32);
    e.mem.set_f64(local + 8, value);
    let node = 0x0300_4000 + id32 * 0x10;
    e.mem.set_u32(node, local);
    e.mem.set_u32(node + 4, 0);
    append(e, LIST + 0xc, node, false);
    let info = 0x0300_8000 + id32 * 0x40;
    e.mem.set_u32(info, id32);
    e.mem.set_u32(info + 0x18, info + 0x20);
    for (i, b) in name.bytes().enumerate() {
        e.mem.set_u8(info + 0x20 + i as u32, b);
    }
    e.mem.set_u8(info + 0x20 + name.len() as u32, 0);
    let var_node = 0x0300_c000 + id32 * 8;
    e.mem.set_u32(var_node, info);
    e.mem.set_u32(var_node + 4, 0);
    append(e, SCRIPT + 0x4c, var_node, true);
}

/// Runs an expression-format command: each argument is the bytes of one expression.
pub fn call_expr(e: &mut Engine, opcode: u32, exprs: &[Vec<u8>]) -> (u32, f64) {
    let mut bytes = vec![exprs.len() as u8];
    for x in exprs {
        bytes.extend_from_slice(&(x.len() as u16 + 2).to_le_bytes());
        bytes.extend_from_slice(x);
    }
    e.mem.set_u16(DATA, opcode as u16);
    e.mem.set_u16(DATA + 2, bytes.len() as u16);
    e.mem.write(DATA + 4, &bytes);
    e.mem.set_u32(OFFSET, 4);
    e.mem.set_f64(RESULT, 0.0);
    let record = nvse::table::record_of(e, opcode);
    assert_ne!(record, 0, "{opcode:04x}");
    let params = e.mem.u32(record + 0x14);
    let execute = e.mem.u32(record + 0x18);
    let ok = e
        .call(execute, &[params, DATA, 0, 0, SCRIPT, LIST, RESULT, OFFSET])
        .eax;
    (ok & 0xff, e.mem.f64(RESULT))
}
