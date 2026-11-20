// SPDX-License-Identifier: GPL-3.0-only
//! The five JohnnyGuitar NVSE commands Tale of Two Wastelands 3.4's scripts call, registered at
//! the opcodes its compiled scripts use. Written from the commands' public names and parameter
//! lists and from how TTW's scripts call them (see `NOTICE`); no plugin code was read.
//!
//! The opcode of each command is its position in the plugin's registration order from
//! [`OPCODE_BASE`]. That order is not in any public document read, so the opcodes here are the
//! ones TTW's own compiled scripts carry (the `SCDA` of a script whose `SCTX` calls the command):
//!
//! | command | opcode | evidence |
//! |---|---|---|
//! | `GetQuestFailed` (`GetQF`) | `0x3165` | `if GetQuestFailed MS14 == 0` |
//! | `IsDLLLoaded` | `0x317E` | `if IsDLLLoaded "client" && ...` |
//! | `GetExtraMiscStat` | `0x31E4` | `if GetExtraMiscStat "Captives Rescued" < 11` |
//! | `ModExtraMiscStat` | `0x31E5` | `ModExtraMiscStat "Bobbleheads Found" 1` |
//! | `InitExtraMiscStat` | `0x31EE` | `InitExtraMiscStat "Bobbleheads Found"` |
//!
//! Every other opcode of the block stays an empty record (a call fails, as without the plugin).
//!
//! What is NOT built: the extra miscellaneous statistics are kept in the engine for the session
//! only. They are not written to the co-save and not shown in the Stats menu; no record format
//! is invented for them, so a plugin section of a real install is carried untouched
//! ([`crate::cosave`]) and never collides with one of ours. `IsDLLLoaded`'s optional integer is
//! accepted and ignored (its meaning is not in the sources read).

use crate::identity::OPCODE_BASE;
use crate::State;
use engine_core::prelude::*;
use nvse::commands::Read;
use nvse::table::{self, Param, PluginRecord, Returns};
use nvse::{handled, CommandArgs, EvalArgs};

const fn p(name: &'static str, kind: u32, optional: bool) -> Param {
    Param {
        name,
        kind,
        optional,
    }
}

/// `kParamType_String`, `_Integer`, `_Quest`.
const STRING: u32 = 0x00;
const INT: u32 = 0x01;
const QUEST: u32 = 0x0e;

const ONE_STRING: &[Param] = &[p("string", STRING, false)];
const ONE_STRING_ONE_INT: &[Param] = &[p("string", STRING, false), p("int", INT, false)];
const IS_DLL_LOADED: &[Param] = &[p("string", STRING, false), p("int", INT, true)];
const ONE_QUEST: &[Param] = &[p("quest", QUEST, false)];

/// One command as the plugin registers it.
pub struct Cmd {
    pub opcode: u32,
    pub name: &'static str,
    pub short: &'static str,
    pub params: &'static [Param],
    pub execute: AbiFn,
    /// The condition eval, for the one command that is also a condition function.
    pub eval: Option<AbiFn>,
}

/// The commands, in the order of their opcodes.
pub static COMMANDS: [Cmd; 5] = [
    Cmd {
        opcode: OPCODE_BASE + 0x65,
        name: "GetQuestFailed",
        short: "GetQF",
        params: ONE_QUEST,
        execute: get_quest_failed,
        eval: Some(get_quest_failed_eval),
    },
    Cmd {
        opcode: OPCODE_BASE + 0x7e,
        name: "IsDLLLoaded",
        short: "",
        params: IS_DLL_LOADED,
        execute: is_dll_loaded,
        eval: None,
    },
    Cmd {
        opcode: OPCODE_BASE + 0xe4,
        name: "GetExtraMiscStat",
        short: "",
        params: ONE_STRING,
        execute: get_extra_misc_stat,
        eval: None,
    },
    Cmd {
        opcode: OPCODE_BASE + 0xe5,
        name: "ModExtraMiscStat",
        short: "",
        params: ONE_STRING_ONE_INT,
        execute: mod_extra_misc_stat,
        eval: None,
    },
    Cmd {
        opcode: OPCODE_BASE + 0xee,
        name: "InitExtraMiscStat",
        short: "",
        params: ONE_STRING,
        execute: init_extra_misc_stat,
        eval: None,
    },
];

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

/// Registers the five commands at their opcodes, the table grown to the last of them.
pub fn install(e: &mut Engine) {
    let last = COMMANDS
        .iter()
        .map(|c| c.opcode)
        .max()
        .unwrap_or(OPCODE_BASE);
    table::grow_to(e, last + 1);
    for c in &COMMANDS {
        let execute = table::execute_address(c.opcode);
        e.register(execute, c.execute);
        let eval = c.eval.map_or(0, |f| {
            e.register(table::eval_address(c.opcode), f);
            table::eval_address(c.opcode)
        });
        let record = PluginRecord {
            name: c.name,
            short: c.short,
            needs_ref: false,
            param_count: c.params.len() as u16,
            params: guest_params(e, c.params),
            execute,
            eval,
            // A condition-capable command is marked with 1.
            flags: u32::from(c.eval.is_some()),
            expression: false,
            returns: Returns::Default,
        };
        table::add_plugin_record(e, c.opcode, &record);
    }
}

/// A name as a stat key: the text up to its first 0.
fn key(mut text: Vec<u8>) -> Vec<u8> {
    let end = text.iter().position(|&b| b == 0).unwrap_or(text.len());
    text.truncate(end);
    text
}

/// `IsDLLLoaded "name" [int]`: 1 when a plugin DLL of that name (with or without `.dll`, without
/// case) is in the plugin list as xNVSE's built-ins hold it, else 0.
fn is_dll_loaded(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if !read.ok {
        return handled(true);
    }
    let name = String::from_utf8_lossy(&key(read.text(0))).to_ascii_lowercase();
    let name = name.strip_suffix(".dll").unwrap_or(&name);
    let loaded = nvse::plugins::built_ins(e).iter().any(|b| {
        b.listed && {
            let file = b.file.to_ascii_lowercase();
            file.strip_suffix(".dll").unwrap_or(&file) == name
        }
    });
    a.set_result(e, f64::from(u8::from(loaded)));
    handled(true)
}

/// `InitExtraMiscStat "name"`: the statistic exists, 0 when it is new.
fn init_extra_misc_stat(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    let read = Read::of(e, &a);
    if read.ok {
        e.ext::<State>().stats.entry(key(read.text(0))).or_insert(0);
    }
    handled(true)
}

/// `ModExtraMiscStat "name" int`: the integer added (wrapping) to the statistic, created at 0.
fn mod_extra_misc_stat(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    let read = Read::of(e, &a);
    if read.ok {
        let by = read.int(1, 0) as i32;
        let stat = e.ext::<State>().stats.entry(key(read.text(0))).or_insert(0);
        *stat = stat.wrapping_add(by);
    }
    handled(true)
}

/// `GetExtraMiscStat "name"`: the statistic's value, 0 for an unknown name.
fn get_extra_misc_stat(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if read.ok {
        let value = e.ext::<State>().stats.get(&key(read.text(0))).copied();
        a.set_result(e, f64::from(value.unwrap_or(0)));
    }
    handled(true)
}

/// `TESQuest`'s quest data sits at `+0x3C` (the engine's `tesquest.rs`: the data's first byte
/// holds the flags) and bit `0x40` is the failed bit `SetFailed` sets.
const QUEST_FLAGS: u32 = 0x3c;
const FAILED: u8 = 0x40;

/// 1 when the quest's failed flag is set; 0 for no quest.
fn quest_failed(e: &Engine, quest: u32) -> f64 {
    f64::from(u8::from(
        quest != 0 && e.mem.u8(quest + QUEST_FLAGS) & FAILED != 0,
    ))
}

/// `GetQuestFailed quest` (alias `GetQF`) as a script line.
fn get_quest_failed(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if read.ok {
        let failed = quest_failed(e, read.form(0));
        a.set_result(e, failed);
    }
    handled(true)
}

/// `GetQuestFailed quest` as a condition.
fn get_quest_failed_eval(e: &mut Engine, w: &[u32]) -> Ret {
    let a = EvalArgs::new(w);
    let failed = quest_failed(e, a.arg1);
    e.mem.set_f64(a.result, failed);
    handled(true)
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
