// SPDX-License-Identifier: GPL-3.0-only
//! The two commands through their records: their opcodes and parameters, each command's
//! answers, and the plugin left unlisted.

use super::*;
use fork_catalogue::{covers, ids};

const DATA: u32 = 0x0300_0000;
const OFFSET: u32 = 0x0300_0f00;
const RESULT: u32 = 0x0300_0f08;
const SCRIPT: u32 = 0x0300_1000;
const LIST: u32 = 0x0300_2000;
const QUEST_FORM: u32 = 0x0300_8000;

fn engine() -> Engine {
    let mut e = Engine::new();
    e.map(0x0118_e000, 0x9000);
    e.map(0x0300_0000, 0x10000);
    nvse::table::install(&mut e);
    crate::install(&mut e);
    e
}

fn int(v: i32) -> Vec<u8> {
    let mut b = vec![b'n'];
    b.extend_from_slice(&v.to_le_bytes());
    b
}
fn s(text: &str) -> Vec<u8> {
    let mut b = (text.len() as u16).to_le_bytes().to_vec();
    b.extend_from_slice(text.as_bytes());
    b
}
/// Script reference 1 is `form`.
fn form_arg(e: &mut Engine, form: u32) -> Vec<u8> {
    let ref_var = 0x0300_a010;
    e.mem.set_u32(ref_var + 8, form);
    e.mem.set_u32(ref_var + 0xc, 0);
    e.mem.set_u32(SCRIPT + 0x44, ref_var);
    e.mem.set_u32(SCRIPT + 0x48, 0);
    let mut b = vec![b'r'];
    b.extend_from_slice(&1u16.to_le_bytes());
    b
}

/// Calls `opcode` through its record with `args`: the handler's answer and the result.
fn call(e: &mut Engine, opcode: u32, args: &[Vec<u8>]) -> (u32, f64) {
    e.mem.set_u16(DATA, opcode as u16);
    let bytes: Vec<u8> = args.concat();
    e.mem.set_u16(DATA + 2, bytes.len() as u16 + 2);
    e.mem.set_u16(DATA + 4, args.len() as u16);
    for (i, b) in bytes.iter().enumerate() {
        e.mem.set_u8(DATA + 6 + i as u32, *b);
    }
    e.mem.set_u32(OFFSET, 4);
    e.mem.set_f64(RESULT, -1.0);
    let record = nvse::table::record_of(e, opcode);
    assert_ne!(record, 0, "{opcode:04x}");
    let params = e.mem.u32(record + 0x14);
    let execute = e.mem.u32(record + 0x18);
    let ok = e
        .call(execute, &[params, DATA, 0, 0, SCRIPT, LIST, RESULT, OFFSET])
        .eax
        & 0xff;
    (ok, e.mem.f64(RESULT))
}

fn eval(e: &mut Engine, quest: u32) -> f64 {
    let record = nvse::table::record_of(e, GET_QUEST_COMPLETED_ALT);
    let eval = e.mem.u32(record + 0x20);
    assert_ne!(eval, 0);
    e.mem.set_f64(RESULT, 99.0);
    e.call(eval, &[0, quest, 0, RESULT]);
    e.mem.f64(RESULT)
}

fn string_of(e: &Engine, id: f64) -> Vec<u8> {
    nvse::vars::string(e, id as u32).expect("a string")
}

/// The records sit at the opcodes the compiled scripts carry, with the commands' names and
/// parameter kinds; the quest one is also a condition. The plugin is not listed, so
/// `IsPluginInstalled "AnhNVSE"` stays 0.
#[test]
fn the_commands_are_registered_at_their_opcodes_and_the_plugin_is_not_listed() {
    let mut e = engine();
    for (opcode, name, kinds, condition) in [
        (
            GET_QUEST_COMPLETED_ALT,
            "GetQuestCompletedAlt",
            vec![QUEST],
            true,
        ),
        (
            SV_PAD_START,
            "Sv_PadStart",
            vec![STRING, INT, STRING],
            false,
        ),
    ] {
        let record = nvse::table::record_of(&e, opcode);
        assert_ne!(record, 0, "{name}");
        assert_eq!(e.mem.cstr(e.mem.u32(record)), name.as_bytes());
        let params = e.mem.u32(record + 0x14);
        let got: Vec<u32> = (0..kinds.len() as u32)
            .map(|i| e.mem.u32(params + 12 * i + 4))
            .collect();
        assert_eq!(got, kinds, "{name}");
        assert_eq!(e.mem.u32(record + 0x20) != 0, condition, "{name}");
    }
    assert_eq!(
        crate::COMMAND_NAMES,
        ["GetQuestCompletedAlt", "Sv_PadStart"]
    );
    assert_eq!(crate::KNOWN_SLOTS, 28);
    assert!(nvse::plugins::built_ins(&e).is_empty());
    assert_eq!(nvse::plugins::listed(&e, crate::NAME.as_bytes()), None);
    crate::install(&mut e);
    assert_ne!(
        nvse::table::record_of(&e, SV_PAD_START),
        0,
        "a second install changes nothing"
    );
}

/// Completed and not failed answers 1, as a command and as a condition; completed and failed,
/// not completed, other bits alone, or no quest answer 0.
#[test]
fn get_quest_completed_alt_wants_completed_and_not_failed() {
    covers(&[ids::extenders::anh_nvse::GETQUESTCOMPLETEDALT]);
    let mut e = engine();
    // Form type byte: a quest (`QUST`).
    e.mem.set_u8(QUEST_FORM + 4, 0x47);
    let arg = form_arg(&mut e, QUEST_FORM);
    for (flags, want) in [
        (COMPLETED, 1.0),
        (COMPLETED | 0x01 | 0x08 | 0x10, 1.0),
        (COMPLETED | FAILED, 0.0),
        (FAILED, 0.0),
        (0x01 | 0x08 | 0x10 | 0x80, 0.0),
        (0, 0.0),
    ] {
        e.mem.set_u8(QUEST_FORM + QUEST_FLAGS, flags);
        assert_eq!(
            call(&mut e, GET_QUEST_COMPLETED_ALT, std::slice::from_ref(&arg)),
            (1, want),
            "{flags:#04x}"
        );
        assert_eq!(
            eval(&mut e, QUEST_FORM),
            want,
            "{flags:#04x} as a condition"
        );
    }
    assert_eq!(eval(&mut e, 0), 0.0);
}

/// The page's two examples, a source as long as the target or longer, a target of 0 or less and
/// an empty pad (the source unchanged), the mods' own use (two digits with a leading zero), and a
/// target past the longest string xNVSE keeps (cut to it).
#[test]
fn sv_pad_start_pads_from_the_start() {
    covers(&[ids::extenders::anh_nvse::SV_PADSTART]);
    let mut e = engine();
    let mut pad = |source: &str, length: i32, with: &str| {
        let (ok, id) = call(&mut e, SV_PAD_START, &[s(source), int(length), s(with)]);
        assert_eq!(ok, 1);
        String::from_utf8(string_of(&e, id)).unwrap()
    };
    assert_eq!(pad("Hello", 10, "*"), "*****Hello");
    assert_eq!(pad("Hello", 10, "foo"), "foofoHello");
    assert_eq!(pad("7", 2, "0"), "07");
    assert_eq!(pad("12", 2, "0"), "12");
    assert_eq!(pad("Hello", 3, "*"), "Hello");
    assert_eq!(pad("Hello", 0, "*"), "Hello");
    assert_eq!(pad("Hello", -4, "*"), "Hello");
    assert_eq!(pad("Hello", 10, ""), "Hello");
    assert_eq!(pad("", 3, "ab"), "aba");
    let long = pad("x", i32::MAX, "ab");
    assert_eq!(long.len(), 0x3fff);
    assert!(long.ends_with("bx") || long.ends_with("ax"));
}
