// SPDX-License-Identifier: GPL-3.0-only
//! ShowOff xNVSE's actor and item reads (group 7 of the profile's missing commands). Written from
//! the commands' GECK Wiki pages (see `NOTICE`, `provenance/07-actor-reads.md`); no plugin code
//! was read.
//!
//! - `actor.IsLimbCrippled [limb] [threshold]`: 1 when the condition actor value of that limb (0
//!   head to 6 brain; -1 or none: any of them) is at or below the threshold (default 0).
//! - `actor.IsAiming`: the game's own `Actor::GetIronSights` (`008bbc10`): aiming or zoomed in.
//! - `actor.IsBlockingOrAiming`: the same, or the game's `Actor::GetBlocked` (`00894d60`: the
//!   animation action is block).
//! - `[ref.]GetIngestibleConsumeSound [ingestible]`: the consumption sound of an ingestible
//!   (`AlchemyItem` +0xC8); a reference stands for its base form.
//!
//! The first three are condition functions. Another kind of reference answers 0.

use crate::commands::Cmd;
use crate::identity::OPCODE_BASE;
use engine_core::prelude::*;
use nvse::commands::{dynamic_cast, form_or_caller, rtti, Read};
use nvse::table::{Param, Returns};
use nvse::{handled, CommandArgs, EvalArgs};

/// `kParamType_Integer`, `_AnyForm`.
const INT: u32 = 0x01;
const ANY_FORM: u32 = 0x3d;

const TWO_OPTIONAL_INTS: &[Param] = &[
    Param {
        name: "int",
        kind: INT,
        optional: true,
    },
    Param {
        name: "int",
        kind: INT,
        optional: true,
    },
];
const ONE_OPTIONAL_FORM: &[Param] = &[Param {
    name: "form",
    kind: ANY_FORM,
    optional: true,
}];

const fn cmd(
    offset: u32,
    name: &'static str,
    params: &'static [Param],
    execute: AbiFn,
    eval: Option<AbiFn>,
    needs_ref: bool,
    returns: Returns,
) -> Cmd {
    Cmd {
        opcode: OPCODE_BASE + offset,
        name,
        short: "",
        params,
        execute,
        eval,
        needs_ref,
        expression: false,
        returns,
    }
}

/// The commands, in the order of their opcodes.
pub static COMMANDS: [Cmd; 4] = [
    cmd(
        0x34,
        "IsLimbCrippled",
        TWO_OPTIONAL_INTS,
        is_limb_crippled,
        Some(is_limb_crippled_eval),
        true,
        Returns::Default,
    ),
    cmd(
        0x68,
        "GetIngestibleConsumeSound",
        ONE_OPTIONAL_FORM,
        get_ingestible_consume_sound,
        None,
        false,
        Returns::Form,
    ),
    cmd(
        0xb7,
        "IsAiming",
        &[],
        is_aiming,
        Some(is_aiming_eval),
        true,
        Returns::Default,
    ),
    cmd(
        0xb8,
        "IsBlockingOrAiming",
        &[],
        is_blocking_or_aiming,
        Some(is_blocking_or_aiming_eval),
        true,
        Returns::Default,
    ),
];

/// The actor's `ActorValueOwner` (+0xA4) and its current-value virtual (+0xC, `float` in ST0).
pub const ACTOR_VALUE_OWNER: u32 = 0xa4;
pub const OWNER_GET_VALUE: u32 = 0xc;
/// `kAVCode_PerceptionCondition`: the head; the six limbs after it in the page's order, to the
/// brain (`kAVCode_BrainCondition`, 0x1F).
const FIRST_LIMB_CONDITION: u32 = 0x19;
const LIMBS: i32 = 7;

/// `Actor::GetIronSights` and `Actor::GetBlocked` (`thiscall`, a byte and a bool).
pub const ACTOR_GET_IRON_SIGHTS: u32 = 0x008b_bc10;
pub const ACTOR_GET_BLOCKED: u32 = 0x0089_4d60;

/// `TESForm::typeID` of an `AlchemyItem`, and its `data.pConsumptionSound`; `TESForm::refID`.
const FORM_TYPE: u32 = 4;
const TYPE_ALCHEMY_ITEM: u8 = 0x2f;
const CONSUMPTION_SOUND: u32 = 0xc8;
const FORM_ID: u32 = 0x0c;

fn actor(e: &mut Engine, reference: u32) -> u32 {
    if reference == 0 {
        return 0;
    }
    dynamic_cast(e, reference, rtti::TES_FORM, rtti::ACTOR)
}

fn as_f64(on: bool) -> f64 {
    f64::from(u8::from(on))
}

/// Whether limb `limb` (any for -1) of `reference` has a condition at or below `threshold`.
fn limb_crippled(e: &mut Engine, reference: u32, limb: i32, threshold: i32) -> bool {
    let actor = actor(e, reference);
    if actor == 0 {
        return false;
    }
    let limbs = match limb {
        -1 => 0..LIMBS,
        0..LIMBS => limb..limb + 1,
        _ => return false,
    };
    limbs.into_iter().any(|l| {
        let code = FIRST_LIMB_CONDITION + l as u32;
        let value = e
            .vcall(actor + ACTOR_VALUE_OWNER, OWNER_GET_VALUE, &args![code])
            .f64();
        value <= f64::from(threshold)
    })
}

/// `actor.IsLimbCrippled [limb] [threshold]`.
fn is_limb_crippled(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if read.ok {
        let limb = read.int(0, u32::MAX) as i32;
        let threshold = read.int(1, 0) as i32;
        let on = limb_crippled(e, a.this_obj, limb, threshold);
        a.set_result(e, as_f64(on));
    }
    handled(true)
}

/// `IsLimbCrippled limb threshold` as a condition.
fn is_limb_crippled_eval(e: &mut Engine, w: &[u32]) -> Ret {
    let a = EvalArgs::new(w);
    let on = limb_crippled(e, a.this_obj, a.arg1 as i32, a.arg2 as i32);
    e.mem.set_f64(a.result, as_f64(on));
    handled(true)
}

fn aiming(e: &mut Engine, actor: u32) -> bool {
    e.call(ACTOR_GET_IRON_SIGHTS, &args![actor]).u8() != 0
}

fn blocking_or_aiming(e: &mut Engine, reference: u32) -> bool {
    let actor = actor(e, reference);
    actor != 0 && (aiming(e, actor) || e.call(ACTOR_GET_BLOCKED, &args![actor]).bool())
}

fn is_aiming_of(e: &mut Engine, reference: u32) -> bool {
    let actor = actor(e, reference);
    actor != 0 && aiming(e, actor)
}

/// `actor.IsAiming`.
fn is_aiming(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    let on = is_aiming_of(e, a.this_obj);
    a.set_result(e, as_f64(on));
    handled(true)
}

fn is_aiming_eval(e: &mut Engine, w: &[u32]) -> Ret {
    let a = EvalArgs::new(w);
    let on = is_aiming_of(e, a.this_obj);
    e.mem.set_f64(a.result, as_f64(on));
    handled(true)
}

/// `actor.IsBlockingOrAiming`.
fn is_blocking_or_aiming(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    let on = blocking_or_aiming(e, a.this_obj);
    a.set_result(e, as_f64(on));
    handled(true)
}

fn is_blocking_or_aiming_eval(e: &mut Engine, w: &[u32]) -> Ret {
    let a = EvalArgs::new(w);
    let on = blocking_or_aiming(e, a.this_obj);
    e.mem.set_f64(a.result, as_f64(on));
    handled(true)
}

/// `[ref.]GetIngestibleConsumeSound [ingestible]`: the sound's form id; 0 for another form or
/// an ingestible without a sound.
fn get_ingestible_consume_sound(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_form_result(e, 0);
    let read = Read::of(e, &a);
    if !read.ok {
        return handled(true);
    }
    let Some(item) = form_or_caller(e, read.form(0), a.this_obj) else {
        return handled(true);
    };
    if item == 0 || e.mem.u8(item + FORM_TYPE) != TYPE_ALCHEMY_ITEM {
        return handled(true);
    }
    let sound = e.mem.u32(item + CONSUMPTION_SOUND);
    if sound != 0 {
        let id = e.mem.u32(sound + FORM_ID);
        a.set_form_result(e, id);
    }
    handled(true)
}

#[cfg(test)]
#[path = "actor_reads_tests.rs"]
mod tests;
