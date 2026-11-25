// SPDX-License-Identifier: GPL-3.0-only
//! JohnnyGuitar NVSE's game-state reads (group 4 of the profile's missing commands: 15 calls, most
//! of them in Enhanced Movement). Written from the commands' GECK Wiki pages (see `NOTICE`,
//! `provenance/04-game-reads.md`); no plugin code was read.
//!
//! - `GetPipBoyMode`: the Pip-Boy's state, 0 closed, 1 or 2 opening, 3 open, 4 or 5 closing: the
//!   word the game's own Pip-Boy update keeps at `InterfaceManager` +0x4BC. Also a condition.
//! - `actor.GetThresholdedActorValue av` (`GetThresholdAV`): the actor value held within its
//!   limits, as the game reads it for its own formulas
//!   (`ActorValueOwner::GetClampedActorFloatValue`, `0066ef50`, on the actor's owner at +0xA4).
//!   Also a condition.
//! - `actor.GetActorValueAlt code` (`GetAVAlt`): the current value of the actor value with that
//!   number, through the game's own `GetActorValue` condition function (`0059c4f0`).
//!
//! A reference that is not an actor, or an actor value number outside 0 to 76, answers 0.

use crate::commands::Cmd;
use crate::identity::OPCODE_BASE;
use engine_core::prelude::*;
use nvse::commands::{dynamic_cast, rtti, Read};
use nvse::table::{Param, Returns};
use nvse::{handled, CommandArgs, EvalArgs};

/// `kParamType_Integer`, `_ActorValue`.
const INT: u32 = 0x01;
const ACTOR_VALUE: u32 = 0x05;

const ONE_INT: &[Param] = &[Param {
    name: "int",
    kind: INT,
    optional: false,
}];
const ONE_ACTOR_VALUE: &[Param] = &[Param {
    name: "actor value",
    kind: ACTOR_VALUE,
    optional: false,
}];

/// The commands, in the order of their opcodes.
pub static COMMANDS: [Cmd; 3] = [
    Cmd {
        opcode: OPCODE_BASE + 0x68,
        name: "GetPipBoyMode",
        short: "",
        params: &[],
        execute: get_pip_boy_mode,
        eval: Some(get_pip_boy_mode_eval),
        needs_ref: false,
        returns: Returns::Default,
    },
    Cmd {
        opcode: OPCODE_BASE + 0x74,
        name: "GetActorValueAlt",
        short: "GetAVAlt",
        params: ONE_INT,
        execute: get_actor_value_alt,
        eval: None,
        needs_ref: true,
        returns: Returns::Default,
    },
    Cmd {
        opcode: OPCODE_BASE + 0xa7,
        name: "GetThresholdedActorValue",
        short: "GetThresholdAV",
        params: ONE_ACTOR_VALUE,
        execute: get_thresholded_actor_value,
        eval: Some(get_thresholded_actor_value_eval),
        needs_ref: true,
        returns: Returns::Default,
    },
];

/// `InterfaceManager::pInstance` and the Pip-Boy state word of `InterfaceManager::UpdatePipboy`
/// (`0070ee80`; engine-fallout-interface `fopipboymanager.rs` reads 3 as open).
pub const INTERFACE_MANAGER: u32 = 0x011d_8a80;
pub const PIP_BOY_MODE: u32 = 0x4bc;

/// `ActorValueOwner::GetClampedActorFloatValue` (`thiscall`, `float` in ST0), the actor's
/// `ActorValueOwner` at +0xA4, and the `GetActorValue` condition function (`cdecl`: reference,
/// code, unused, `double *result`).
pub const GET_CLAMPED_ACTOR_FLOAT_VALUE: u32 = 0x0066_ef50;
pub const ACTOR_VALUE_OWNER: u32 = 0xa4;
pub const GET_ACTOR_VALUE_CONDITION: u32 = 0x0059_c4f0;

/// The last actor value number (`eActorValue`, 77 values).
const ACTOR_VALUE_MAX: u32 = 76;

fn pip_boy_mode(e: &mut Engine) -> f64 {
    let manager = e.mem.u32(INTERFACE_MANAGER);
    if manager == 0 {
        return 0.0;
    }
    f64::from(e.mem.u32(manager + PIP_BOY_MODE))
}

/// `GetPipBoyMode`.
fn get_pip_boy_mode(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    let mode = pip_boy_mode(e);
    a.set_result(e, mode);
    handled(true)
}

/// `GetPipBoyMode` as a condition.
fn get_pip_boy_mode_eval(e: &mut Engine, w: &[u32]) -> Ret {
    let a = EvalArgs::new(w);
    let mode = pip_boy_mode(e);
    e.mem.set_f64(a.result, mode);
    handled(true)
}

/// The reference as an actor (0 for none or another kind of reference).
fn actor(e: &mut Engine, reference: u32) -> u32 {
    if reference == 0 {
        return 0;
    }
    dynamic_cast(e, reference, rtti::TES_FORM, rtti::ACTOR)
}

/// The actor value `code` of `reference` within its limits; 0 for no actor or a code out of range.
fn thresholded(e: &mut Engine, reference: u32, code: u32) -> f64 {
    let actor = actor(e, reference);
    if actor == 0 || code > ACTOR_VALUE_MAX {
        return 0.0;
    }
    e.call(
        GET_CLAMPED_ACTOR_FLOAT_VALUE,
        &args![actor + ACTOR_VALUE_OWNER, code],
    )
    .f64()
}

/// `actor.GetThresholdedActorValue av`.
fn get_thresholded_actor_value(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if read.ok {
        let value = thresholded(e, a.this_obj, read.short(0, u32::MAX));
        a.set_result(e, value);
    }
    handled(true)
}

/// `GetThresholdedActorValue av` as a condition: the actor value is the first parameter.
fn get_thresholded_actor_value_eval(e: &mut Engine, w: &[u32]) -> Ret {
    let a = EvalArgs::new(w);
    let value = thresholded(e, a.this_obj, a.arg1);
    e.mem.set_f64(a.result, value);
    handled(true)
}

/// `actor.GetActorValueAlt code`: the game's `GetActorValue` condition with the number for the
/// name. Only an actor is passed on (the condition checks too), so another reference stays 0.
fn get_actor_value_alt(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if !read.ok || actor(e, a.this_obj) == 0 {
        return handled(true);
    }
    let code = read.int(0, u32::MAX);
    if code <= ACTOR_VALUE_MAX {
        e.call(
            GET_ACTOR_VALUE_CONDITION,
            &args![a.this_obj, code, 0u32, a.result],
        );
    }
    handled(true)
}

#[cfg(test)]
#[path = "game_reads_tests.rs"]
mod tests;
