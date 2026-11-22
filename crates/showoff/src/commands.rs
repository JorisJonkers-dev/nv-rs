// SPDX-License-Identifier: GPL-3.0-only
//! ShowOff xNVSE's commands built so far, from the plugin's public name and parameter list and
//! from how Tale of Two Wastelands' scripts call them (`NOTICE`).
//!
//! - `ActorHasBaseFlag` (long name `GetActorHasBaseFlag`), called on an actor reference with one
//!   integer: 1 when bit `n` of the actor base's flags word (`TESActorBaseData::iActorBaseFlags`,
//!   the `+0x34` of a `TESActorBase`) is set, 0 otherwise. A condition can use it.
//!
//! - The commands built since for the mods of a large modded profile, group by group, each in its
//!   own file with a provenance note in `provenance/`: [`crate::lists`] (`ListAddList`,
//!   `ListAddArray`), [`crate::ini`] (the cached INI commands), [`crate::lookups`] (`IsBaseForm`), [`crate::weapons`] (weapon type and flags).
//!
//! No other ShowOff command is built; a script that calls one fails at that line, as without the
//! plugin.

use engine_core::prelude::*;
use nvse::commands::{dynamic_cast, rtti, Read};
use nvse::table::{self, Param, PluginRecord, Returns};
use nvse::{handled, CommandArgs, EvalArgs};

/// The command names the capabilities file lists, long names: the first is built here, the rest
/// are the groups' tables in the order of [`all`].
pub const COMMAND_NAMES: [&str; 15] = [
    "GetActorHasBaseFlag",
    "ListAddList",
    "ListAddArray",
    "SetINIValue_Cached",
    "GetINIFloatOrCreate_Cached",
    "GetINIFloatOrDefault_Cached",
    "GetINIStringOrDefault_Cached",
    "ClearFileCacheShowOff",
    "SaveCachedIniFile",
    "IsBaseForm",
    "IsWeaponMelee",
    "IsWeaponRanged",
    "IsEquippedWeaponMelee",
    "GetWeaponHasFlag",
    "SetWeaponFlag",
];

/// One command as the plugin registers it, for the groups' tables (the first command above is
/// registered by hand).
pub struct Cmd {
    pub opcode: u32,
    pub name: &'static str,
    pub short: &'static str,
    pub params: &'static [Param],
    pub execute: AbiFn,
    /// The condition eval, for a command that is also a condition function.
    pub eval: Option<AbiFn>,
    /// Whether a calling reference is required.
    pub needs_ref: bool,
    /// Asks xNVSE's expression parser (`kNVSEParams_*` parameter lists).
    pub expression: bool,
    /// What the command answers.
    pub returns: Returns,
}

/// Every command of the groups' tables, in the order of the groups' files.
pub fn all() -> impl Iterator<Item = &'static Cmd> {
    crate::lists::COMMANDS
        .iter()
        .chain(crate::ini::COMMANDS.iter())
        .chain(crate::lookups::COMMANDS.iter())
        .chain(crate::weapons::COMMANDS.iter())
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

/// `ActorHasBaseFlag`'s opcode: the one TTW's compiled scripts call (`0x3CE0`, which is
/// `identity::OPCODE_BASE` plus `0x4D`).
pub const ACTOR_HAS_BASE_FLAG: u32 = 0x3ce0;

/// `kParamType_Integer`.
const INT: u32 = 0x01;

/// `TESObjectREFR::baseForm` and `TESActorBaseData::iActorBaseFlags` as seen from a
/// `TESActorBase`.
const REFR_BASE_FORM: u32 = 0x20;
const ACTOR_BASE_FLAGS: u32 = 0x34;

/// The record's flags word: 1 for a command a condition can use.
const CONDITION: u32 = 1;

/// Whether bit `bit` of the flags word of the actor base of `reference` is set; false for no
/// reference, a base that is not an actor base, or a bit outside `0..32`.
fn has_base_flag(e: &mut Engine, reference: u32, bit: u32) -> bool {
    if reference == 0 || bit >= 32 {
        return false;
    }
    let form = e.mem.u32(reference + REFR_BASE_FORM);
    let base = dynamic_cast(e, form, rtti::TES_FORM, rtti::TES_ACTOR_BASE);
    base != 0 && e.mem.u32(base + ACTOR_BASE_FLAGS) & (1 << bit) != 0
}

/// `ActorHasBaseFlag bit` on a script line.
fn actor_has_base_flag(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if read.ok {
        let on = has_base_flag(e, a.this_obj, read.int(0, 0));
        a.set_result(e, f64::from(u8::from(on)));
    }
    handled(true)
}

/// `ActorHasBaseFlag bit` as a condition.
fn actor_has_base_flag_eval(e: &mut Engine, w: &[u32]) -> Ret {
    let a = EvalArgs::new(w);
    let on = has_base_flag(e, a.this_obj, a.arg1);
    e.mem.set_f64(a.result, f64::from(u8::from(on)));
    handled(true)
}

/// Registers the built commands at their opcodes and returns nothing else: the table grown to
/// reach them, each record with its names, parameter, execute and eval.
pub fn install(e: &mut Engine) {
    let last = all().map(|c| c.opcode).fold(ACTOR_HAS_BASE_FLAG, u32::max);
    table::grow_to(e, last + 1);
    let param = Param {
        name: "int",
        kind: INT,
        optional: false,
    };
    let params = table::alloc(e, 12);
    let name = table::guest_str(e, param.name.as_bytes());
    e.mem.set_u32(params, name);
    e.mem.set_u32(params + 4, param.kind);
    e.mem.set_u32(params + 8, 0);
    let execute = table::execute_address(ACTOR_HAS_BASE_FLAG);
    let eval = table::eval_address(ACTOR_HAS_BASE_FLAG);
    e.register(execute, actor_has_base_flag);
    e.register(eval, actor_has_base_flag_eval);
    table::add_plugin_record(
        e,
        ACTOR_HAS_BASE_FLAG,
        &PluginRecord {
            name: COMMAND_NAMES[0],
            short: "ActorHasBaseFlag",
            needs_ref: true,
            param_count: 1,
            params,
            execute,
            eval,
            flags: CONDITION,
            expression: false,
            returns: Returns::Default,
        },
    );
    for c in all() {
        let execute = table::execute_address(c.opcode);
        e.register(execute, c.execute);
        let eval = match c.eval {
            Some(f) => {
                let at = table::eval_address(c.opcode);
                e.register(at, f);
                at
            }
            None => 0,
        };
        let record = PluginRecord {
            name: c.name,
            short: c.short,
            needs_ref: c.needs_ref,
            param_count: c.params.len() as u16,
            params: guest_params(e, c.params),
            execute,
            eval,
            flags: if c.eval.is_some() { CONDITION } else { 0 },
            expression: c.expression,
            returns: c.returns,
        };
        table::add_plugin_record(e, c.opcode, &record);
    }
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
