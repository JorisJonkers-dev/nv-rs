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
//! The extra miscellaneous statistics are listed in the Pip-Boy's statistics page after the game's
//! own, by name (the three natives of [`engine_core::extend::EXTRA_MISC_STAT_COUNT`] that the
//! Stats menu calls). They are saved in a record of nv-rs's own type in the plugin's co-save section ([`crate::cosave`]);
//! the plugin's own record format is in no public document read, so a plugin section of a real
//! install is carried untouched and never collides with ours. `IsDLLLoaded`'s optional integer is
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
    /// Whether a calling reference is required (the declaration line's flag).
    pub needs_ref: bool,
    /// What the command answers (a form or a string need their own result handling).
    pub returns: Returns,
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
        needs_ref: false,
        returns: Returns::Default,
    },
    Cmd {
        opcode: OPCODE_BASE + 0x7e,
        name: "IsDLLLoaded",
        short: "",
        params: IS_DLL_LOADED,
        execute: is_dll_loaded,
        eval: None,
        needs_ref: false,
        returns: Returns::Default,
    },
    Cmd {
        opcode: OPCODE_BASE + 0xe4,
        name: "GetExtraMiscStat",
        short: "",
        params: ONE_STRING,
        execute: get_extra_misc_stat,
        eval: None,
        needs_ref: false,
        returns: Returns::Default,
    },
    Cmd {
        opcode: OPCODE_BASE + 0xe5,
        name: "ModExtraMiscStat",
        short: "",
        params: ONE_STRING_ONE_INT,
        execute: mod_extra_misc_stat,
        eval: None,
        needs_ref: false,
        returns: Returns::Default,
    },
    Cmd {
        opcode: OPCODE_BASE + 0xee,
        name: "InitExtraMiscStat",
        short: "",
        params: ONE_STRING,
        execute: init_extra_misc_stat,
        eval: None,
        needs_ref: false,
        returns: Returns::Default,
    },
];

/// Every command this crate builds, group by group, in the order of the groups' files.
pub fn all() -> impl Iterator<Item = &'static Cmd> {
    COMMANDS
        .iter()
        .chain(crate::math::COMMANDS.iter())
        .chain(crate::lookups::COMMANDS.iter())
        .chain(crate::form_traits::COMMANDS.iter())
        .chain(crate::game_reads::COMMANDS.iter())
        .chain(crate::body_parts::COMMANDS.iter())
        .chain(crate::cell_visits::COMMANDS.iter())
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

/// Registers the five commands at their opcodes, the table grown to the last of them.
pub fn install(e: &mut Engine) {
    let last = all().map(|c| c.opcode).max().unwrap_or(OPCODE_BASE);
    table::grow_to(e, last + 1);
    for c in all() {
        let execute = table::execute_address(c.opcode);
        e.register(execute, c.execute);
        let eval = c.eval.map_or(0, |f| {
            e.register(table::eval_address(c.opcode), f);
            table::eval_address(c.opcode)
        });
        let record = PluginRecord {
            name: c.name,
            short: c.short,
            needs_ref: c.needs_ref,
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
    install_stat_rows(e);
}

/// The three natives the Stats menu asks for the rows after the game's own (nv-rs#307): how many
/// statistics there are, and the name and value of the one at an index (by name order).
fn install_stat_rows(e: &mut Engine) {
    use engine_core::extend::{EXTRA_MISC_STAT_COUNT, EXTRA_MISC_STAT_NAME, EXTRA_MISC_STAT_VALUE};
    e.register(EXTRA_MISC_STAT_COUNT, stat_row_count);
    e.register(EXTRA_MISC_STAT_NAME, stat_row_name);
    e.register(EXTRA_MISC_STAT_VALUE, stat_row_value);
}

fn stat_row_count(e: &mut Engine, _w: &[u32]) -> Ret {
    (e.ext::<State>().stats.len() as u32).into_ret()
}

/// The name of row `w[0]` as guest text (kept by the built-ins' text table), 0 past the end.
fn stat_row_name(e: &mut Engine, w: &[u32]) -> Ret {
    let name = e
        .ext::<State>()
        .stats
        .keys()
        .nth(w.first().copied().unwrap_or(u32::MAX) as usize)
        .cloned();
    match name {
        Some(name) => table::guest_str(e, &name).into_ret(),
        None => 0u32.into_ret(),
    }
}

/// The value of row `w[0]`, 0 past the end.
fn stat_row_value(e: &mut Engine, w: &[u32]) -> Ret {
    let value = e
        .ext::<State>()
        .stats
        .values()
        .nth(w.first().copied().unwrap_or(u32::MAX) as usize)
        .copied()
        .unwrap_or(0);
    (value as u32).into_ret()
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
