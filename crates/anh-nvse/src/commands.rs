// SPDX-License-Identifier: GPL-3.0-only
//! The two commands, at the opcodes the mods' compiled scripts carry (`NOTICE`,
//! `provenance/01-quest-completed-and-pad-start.md`).

use engine_core::prelude::*;
use nvse::commands::string::assign_to_string_var;
use nvse::commands::Read;
use nvse::table::{self, Param, PluginRecord, Returns};
use nvse::{handled, CommandArgs, EvalArgs};

/// The opcodes.
pub const GET_QUEST_COMPLETED_ALT: u32 = 0x3604;
pub const SV_PAD_START: u32 = 0x361b;

/// `kParamType_String`, `_Integer`, `_Quest`.
const STRING: u32 = 0x00;
const INT: u32 = 0x01;
const QUEST: u32 = 0x0e;

const fn p(name: &'static str, kind: u32) -> Param {
    Param {
        name,
        kind,
        optional: false,
    }
}

const ONE_QUEST: &[Param] = &[p("quest", QUEST)];
const PAD: &[Param] = &[
    p("source", STRING),
    p("target length", INT),
    p("pad", STRING),
];

/// `TESQuest +0x3C`: the quest's flag byte. Completed `0x02` (the bit the game's own
/// `GetQuestCompleted` reads, `0059e400`), failed `0x40` (the bit `SetFailed` sets).
pub const QUEST_FLAGS: u32 = 0x3c;
pub const COMPLETED: u8 = 0x02;
pub const FAILED: u8 = 0x40;

/// The longest string xNVSE keeps in a string variable (a longer one is stored empty).
const LONGEST: usize = 0x3fff;

/// 1 when the quest is completed and not failed; 0 for no quest.
fn completed_not_failed(e: &Engine, quest: u32) -> f64 {
    if quest == 0 {
        return 0.0;
    }
    let flags = e.mem.u8(quest + QUEST_FLAGS);
    f64::from(u8::from(flags & COMPLETED != 0 && flags & FAILED == 0))
}

/// `GetQuestCompletedAlt quest` as a script line.
fn get_quest_completed_alt(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if read.ok {
        let done = completed_not_failed(e, read.form(0));
        a.set_result(e, done);
    }
    handled(true)
}

/// `GetQuestCompletedAlt quest` as a condition.
fn get_quest_completed_alt_eval(e: &mut Engine, w: &[u32]) -> Ret {
    let a = EvalArgs::new(w);
    let done = completed_not_failed(e, a.arg1);
    e.mem.set_f64(a.result, done);
    handled(true)
}

/// `source` padded from its start with `pad`, repeated and cut, to `length` bytes; `source`
/// itself when it is that long already, the length is 0 or less, or the pad is empty. The length
/// is at most [`LONGEST`].
pub fn pad_start(source: &[u8], length: i32, pad: &[u8]) -> Vec<u8> {
    let length = usize::try_from(length).unwrap_or(0).min(LONGEST);
    if pad.is_empty() || source.len() >= length {
        return source.to_vec();
    }
    let mut out: Vec<u8> = pad
        .iter()
        .copied()
        .cycle()
        .take(length - source.len())
        .collect();
    out.extend_from_slice(source);
    out
}

/// `Sv_PadStart source length pad`: a new string variable with the padded text.
fn sv_pad_start(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if read.ok {
        // The integer as the script stores it (a negative length is a large unsigned one).
        let length = read.int(1, 0) as i32;
        let text = pad_start(&read.text(0), length, &read.text(2));
        assign_to_string_var(e, &a, &text);
    }
    handled(true)
}

/// A parameter list in the built-ins' data (12 bytes a parameter: name, type, optional).
fn guest_params(e: &mut Engine, params: &[Param]) -> u32 {
    let at = table::alloc(e, 12 * params.len() as u32);
    for (i, param) in params.iter().enumerate() {
        let name = table::guest_str(e, param.name.as_bytes());
        let slot = at + 12 * i as u32;
        e.mem.set_u32(slot, name);
        e.mem.set_u32(slot + 4, param.kind);
        e.mem.set_u32(slot + 8, u32::from(param.optional));
    }
    at
}

/// One command as the plugin registers it.
struct Cmd {
    opcode: u32,
    name: &'static str,
    params: &'static [Param],
    execute: AbiFn,
    /// The condition eval, for a command that is also a condition function.
    eval: Option<AbiFn>,
    returns: Returns,
}

/// The commands, in the order of their opcodes.
static COMMANDS: [Cmd; 2] = [
    Cmd {
        opcode: GET_QUEST_COMPLETED_ALT,
        name: crate::COMMAND_NAMES[0],
        params: ONE_QUEST,
        execute: get_quest_completed_alt,
        eval: Some(get_quest_completed_alt_eval),
        returns: Returns::Default,
    },
    Cmd {
        opcode: SV_PAD_START,
        name: crate::COMMAND_NAMES[1],
        params: PAD,
        execute: sv_pad_start,
        eval: None,
        returns: Returns::String,
    },
];

/// Registers the two commands: the table grown to reach them, each record with its name,
/// parameters and execute; `GetQuestCompletedAlt` also as a condition.
pub fn install(e: &mut Engine) {
    table::grow_to(e, SV_PAD_START + 1);
    for c in &COMMANDS {
        let execute = table::execute_address(c.opcode);
        e.register(execute, c.execute);
        let eval = c.eval.map_or(0, |f| {
            e.register(table::eval_address(c.opcode), f);
            table::eval_address(c.opcode)
        });
        let record = PluginRecord {
            name: c.name,
            short: "",
            needs_ref: false,
            param_count: c.params.len() as u16,
            params: guest_params(e, c.params),
            execute,
            eval,
            // A condition-capable command is marked with 1.
            flags: u32::from(c.eval.is_some()),
            expression: false,
            returns: c.returns,
        };
        table::add_plugin_record(e, c.opcode, &record);
    }
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
