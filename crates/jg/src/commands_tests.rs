// SPDX-License-Identifier: GPL-3.0-only
//! The five commands: their opcodes (TTW's compiled scripts), each command's answers, and the
//! install switch.

use super::*;
use crate::identity::{self, OPCODE_BASE};

const DATA: u32 = 0x0300_0000;
const OFFSET: u32 = 0x0300_0f00;
const RESULT: u32 = 0x0300_0f08;
const SCRIPT: u32 = 0x0300_1000;
const LIST: u32 = 0x0300_2000;
const QUEST: u32 = 0x0300_8000;

const GET_QUEST_FAILED: u32 = 0x3165;
const IS_DLL_LOADED: u32 = 0x317e;
const GET_STAT: u32 = 0x31e4;
const MOD_STAT: u32 = 0x31e5;
const INIT_STAT: u32 = 0x31ee;

fn engine() -> Engine {
    let mut e = Engine::new();
    e.map(0x0118_e000, 0x9000);
    e.map(0x0300_0000, 0x10000);
    nvse::table::install(&mut e);
    crate::install_as(&mut e, true);
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

fn text(e: &Engine, at: u32) -> Vec<u8> {
    nvse::table::guest_text(e, e.mem.u32(at))
}

/// The opcodes are the ones TTW's compiled scripts carry, and the records have the names,
/// parameter kinds and (for the quest one) a condition eval.
#[test]
fn commands_sit_at_the_opcodes_ttw_uses() {
    let e = engine();
    let at = |name: &str| COMMANDS.iter().find(|c| c.name == name).unwrap().opcode;
    assert_eq!(at("GetQuestFailed"), GET_QUEST_FAILED);
    assert_eq!(at("IsDLLLoaded"), IS_DLL_LOADED);
    assert_eq!(at("GetExtraMiscStat"), GET_STAT);
    assert_eq!(at("ModExtraMiscStat"), MOD_STAT);
    assert_eq!(at("InitExtraMiscStat"), INIT_STAT);
    for (c, name) in COMMANDS.iter().zip(crate::COMMAND_NAMES) {
        assert_eq!(c.name, name);
        assert!(c.opcode >= OPCODE_BASE);
        let record = nvse::table::record_of(&e, c.opcode);
        assert_eq!(text(&e, record), c.name.as_bytes());
        assert_eq!(text(&e, record + 4), c.short.as_bytes());
        assert_eq!(e.mem.u16(record + 0x12) as usize, c.params.len());
        assert_eq!(e.mem.u32(record + 0x20) != 0, c.eval.is_some());
        let params = e.mem.u32(record + 0x14);
        for (k, p) in c.params.iter().enumerate() {
            let slot = params + 12 * k as u32;
            assert_eq!(e.mem.u32(slot + 4), p.kind);
            assert_eq!(e.mem.u32(slot + 8), u32::from(p.optional));
        }
    }
    let quest = nvse::table::record_of(&e, GET_QUEST_FAILED);
    assert_eq!(text(&e, quest + 4), b"GetQF");
    assert_eq!(e.mem.u32(quest + 0x24), 1);
    // An opcode of the block no command owns stays empty.
    let other = nvse::table::record_of(&e, OPCODE_BASE + 1);
    assert_eq!(text(&e, other), b"");
}

/// `IsDLLLoaded "client"` is 0 (TTW's check); a built-in's DLL, with or without `.dll` and
/// without case, is 1; the optional integer changes nothing.
#[test]
fn is_dll_loaded_checks_the_plugin_list() {
    let mut e = engine();
    assert_eq!(call(&mut e, IS_DLL_LOADED, &[s("client")]), (1, 0.0));
    assert_eq!(call(&mut e, IS_DLL_LOADED, &[s("")]), (1, 0.0));
    assert_eq!(
        call(&mut e, IS_DLL_LOADED, &[s("JohnnyGuitar.dll")]),
        (1, 1.0)
    );
    assert_eq!(call(&mut e, IS_DLL_LOADED, &[s("JOHNNYGUITAR")]), (1, 1.0));
    assert_eq!(
        call(&mut e, IS_DLL_LOADED, &[s("johnnyguitar"), int(1)]),
        (1, 1.0)
    );
    // The name is not the DLL's file name, and xNVSE itself is not in the list.
    assert_eq!(
        call(&mut e, IS_DLL_LOADED, &[s("JohnnyGuitarNVSE")]),
        (1, 0.0)
    );
    assert_eq!(
        call(&mut e, IS_DLL_LOADED, &[s("nvse_steam_loader")]),
        (1, 0.0)
    );
}

/// Init adds a name at 0 and leaves a value alone; Mod adds (and creates); Get answers 0 for an
/// unknown name; names are kept apart.
#[test]
fn extra_misc_stats_are_kept_by_name() {
    let mut e = engine();
    assert_eq!(call(&mut e, GET_STAT, &[s("Bobbleheads Found")]), (1, 0.0));
    assert_eq!(call(&mut e, INIT_STAT, &[s("Bobbleheads Found")]).0, 1);
    assert!(e
        .ext::<State>()
        .stats
        .contains_key(&b"Bobbleheads Found"[..]));
    assert_eq!(call(&mut e, GET_STAT, &[s("Bobbleheads Found")]), (1, 0.0));
    assert_eq!(
        call(&mut e, MOD_STAT, &[s("Bobbleheads Found"), int(1)]).0,
        1
    );
    assert_eq!(
        call(&mut e, MOD_STAT, &[s("Bobbleheads Found"), int(4)]).0,
        1
    );
    assert_eq!(call(&mut e, GET_STAT, &[s("Bobbleheads Found")]), (1, 5.0));
    // Init again keeps the value.
    call(&mut e, INIT_STAT, &[s("Bobbleheads Found")]);
    assert_eq!(call(&mut e, GET_STAT, &[s("Bobbleheads Found")]), (1, 5.0));
    // Mod creates a missing name, and a negative integer subtracts.
    call(&mut e, MOD_STAT, &[s("Contracts Completed"), int(3)]);
    call(&mut e, MOD_STAT, &[s("Contracts Completed"), int(-5)]);
    assert_eq!(
        call(&mut e, GET_STAT, &[s("Contracts Completed")]),
        (1, -2.0)
    );
    assert_eq!(call(&mut e, GET_STAT, &[s("Bobbleheads Found")]), (1, 5.0));
    // Wrapping, not a panic.
    call(&mut e, MOD_STAT, &[s("Big"), int(i32::MAX)]);
    call(&mut e, MOD_STAT, &[s("Big"), int(1)]);
    assert_eq!(
        call(&mut e, GET_STAT, &[s("Big")]),
        (1, f64::from(i32::MIN))
    );
}

/// A quest with the failed bit set answers 1 as a command and as a condition; without it, or
/// with no quest, 0; other flag bits do not count.
#[test]
fn get_quest_failed_reads_the_flag() {
    let mut e = engine();
    // Form type byte: a quest (`QUST`).
    e.mem.set_u8(QUEST + 4, 0x47);
    e.mem.set_u8(QUEST + QUEST_FLAGS, 0x3f | 0x80);
    let arg = form_arg(&mut e, QUEST);
    assert_eq!(
        call(&mut e, GET_QUEST_FAILED, std::slice::from_ref(&arg)),
        (1, 0.0)
    );
    e.mem.set_u8(QUEST + QUEST_FLAGS, 0x01 | FAILED);
    assert_eq!(
        call(&mut e, GET_QUEST_FAILED, std::slice::from_ref(&arg)),
        (1, 1.0)
    );
    assert_eq!(eval(&mut e, QUEST), 1.0);
    e.mem.set_u8(QUEST + QUEST_FLAGS, 0x01);
    assert_eq!(eval(&mut e, QUEST), 0.0);
    assert_eq!(eval(&mut e, 0), 0.0);
}

fn eval(e: &mut Engine, quest: u32) -> f64 {
    let record = nvse::table::record_of(e, GET_QUEST_FAILED);
    let eval = e.mem.u32(record + 0x20);
    assert_ne!(eval, 0);
    e.mem.set_f64(RESULT, 99.0);
    e.call(eval, &[0, quest, 0, RESULT]);
    e.mem.f64(RESULT)
}

/// The commands are registered with the plugin when it answers installed, and not otherwise;
/// `set_answers_installed` is what `install` reads.
#[test]
fn install_follows_the_switch() {
    let base = || {
        let mut e = Engine::new();
        e.map(0x0118_e000, 0x9000);
        nvse::table::install(&mut e);
        e
    };
    // Default: off.
    let mut e = base();
    crate::install(&mut e);
    assert!(nvse::plugins::built_ins(&e).is_empty());
    assert_eq!(nvse::plugins::listed(&e, identity::NAME.as_bytes()), None);
    assert_eq!(nvse::table::record_of(&e, GET_QUEST_FAILED), 0);
    // Switched on: plugin and commands.
    let mut e = base();
    crate::set_answers_installed(&mut e, true);
    crate::install(&mut e);
    assert_eq!(
        nvse::plugins::listed(&e, identity::NAME.as_bytes()),
        Some((identity::NAME, identity::VERSION_INT))
    );
    let record = nvse::table::record_of(&e, IS_DLL_LOADED);
    assert_eq!(text(&e, record), b"IsDLLLoaded");
    // Switched off explicitly.
    let mut e = base();
    crate::set_answers_installed(&mut e, false);
    crate::install(&mut e);
    assert!(nvse::plugins::built_ins(&e).is_empty());
}
