// SPDX-License-Identifier: GPL-3.0-only
//! kNVSE's commands built so far, from the plugin's public name and parameter list and the GECK
//! Wiki's description (`NOTICE`, `provenance/02-set-actor-animation-path.md`).
//!
//! - `SetActorAnimationPath firstPerson enable path [pollCondition] [condition]
//!   [matchBaseAnimGroupId]` (`0x3922`), on an actor reference or on none (every actor): with
//!   `enable`, loads the `.kf` file at `path` (`ModelLoader::LoadKF`) and puts it on the store's
//!   stack for the actor (or every actor), the person and the animation group the file names; the
//!   condition function, when one is given, must return true for the actor at each start of the
//!   group. Without `enable`, takes the calling mod's override of that path off again. Answers 1
//!   when an override was set or taken off, else 0.
//!
//! No other kNVSE command is built; a script that calls one fails at that line, as without the
//! plugin.

use crate::store::{Key, Override, Target};
use engine_core::prelude::*;
use nvse::commands::Read;
use nvse::table::{self, Param, PluginRecord, Returns};
use nvse::{handled, CommandArgs};

/// The command names the capabilities file lists, in opcode order.
pub const COMMAND_NAMES: [&str; 1] = ["SetActorAnimationPath"];

/// `SetActorAnimationPath`'s opcode: the plugin's base `0x3920` plus 2, its third registration.
pub const SET_ACTOR_ANIMATION_PATH: u32 = 0x3922;

/// `kParamType_String`, `kParamType_Integer`, `kParamType_AnyForm`.
const STRING: u32 = 0x00;
const INT: u32 = 0x01;
const ANY_FORM: u32 = 0x3d;

/// The parameter list as the plugin declares it (names, types and which are optional).
const PARAMS: &[Param] = &[
    Param {
        name: "first person",
        kind: INT,
        optional: false,
    },
    Param {
        name: "enable",
        kind: INT,
        optional: false,
    },
    Param {
        name: "animation path",
        kind: STRING,
        optional: false,
    },
    Param {
        name: "poll condition",
        kind: INT,
        optional: true,
    },
    Param {
        name: "condition",
        kind: ANY_FORM,
        optional: true,
    },
    Param {
        name: "match base anim group id",
        kind: INT,
        optional: true,
    },
];

/// The `ModelLoader` singleton pointer and `ModelLoader::LoadKF(name)` (one manual reference on
/// the model it answers).
const MODEL_LOADER: u32 = 0x011c_3b3c;
const MODEL_LOADER_LOAD_KF: u32 = 0x0044_71c0;
/// The id of a `TESAnimGroup` (group and type bytes).
const ANIM_GROUP_ID: u32 = 0x0047_d3b0;
/// `TESForm +0xC`: the form id.
const FORM_ID: u32 = 0xc;
/// The longest path handed to the loader (the game's path buffers hold 0x104 bytes).
const MAX_PATH: usize = 0x103;

/// The mod index of the last file in the form's list of files (`TESForm`'s `+0x10`, the file's
/// index at `+0x40C`), 0xFF for none: the mod an override belongs to.
fn owning_mod(e: &Engine, form: u32) -> u32 {
    if form == 0 {
        return 0xff;
    }
    let mut node = form + 0x10;
    while e.mem.u32(node + 4) != 0 {
        node = e.mem.u32(node + 4);
    }
    let file = e.mem.u32(node);
    if file == 0 {
        0xff
    } else {
        u32::from(e.mem.u8(file + 0x40c))
    }
}

/// Whether `path` names one `.kf` file (a folder of them is a later step).
fn is_kf_file(path: &[u8]) -> bool {
    path.len() > 3 && path[path.len() - 3..].eq_ignore_ascii_case(b".kf")
}

/// The `KFModel` of the file at `path`, with the manual reference the load takes; 0 when there is
/// none or the path is too long.
fn load_kf(e: &mut Engine, path: &[u8]) -> u32 {
    if path.is_empty() || path.len() > MAX_PATH || path.contains(&0) {
        return 0;
    }
    let loader: u32 = e.global(MODEL_LOADER);
    if loader == 0 {
        return 0;
    }
    let size = (path.len() as u32 + 4) & !3;
    e.with_stack(size, |e, name| {
        e.mem.write(name.addr(), path);
        e.mem.set_u8(name.addr() + path.len() as u32, 0);
        e.call(MODEL_LOADER_LOAD_KF, &args![loader, name]).u32()
    })
}

/// Sets the override of `path` for `target` and person: loaded, its group read, put on top of its
/// stack. Whether it was set.
fn enable_path(
    e: &mut Engine,
    target: Target,
    first_person: bool,
    owner: u32,
    path: &[u8],
    condition: u32,
    poll: bool,
) -> bool {
    if !is_kf_file(path) {
        return false;
    }
    // The loader is handed the path as the store keeps it (lower case, `\` separators).
    let path = &crate::store::normal_path(path);
    let kf = load_kf(e, path);
    if kf == 0 {
        return false;
    }
    let group = e.call(crate::seam::KF_MODEL_ANIM_GROUP, &args![kf]).u32();
    if group == 0 {
        e.call(crate::KF_MODEL_RELEASE, &args![kf]);
        return false;
    }
    let id = e.call(ANIM_GROUP_ID, &args![group]).u16();
    if condition != 0 {
        crate::save_condition(e, condition);
    }
    let key = Key {
        target,
        first_person,
        group: id,
    };
    crate::enable(
        e,
        key,
        Override {
            owner,
            path: path.to_vec(),
            kf,
            condition,
            poll,
        },
    );
    true
}

/// `SetActorAnimationPath` on a script line.
fn set_actor_animation_path(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if !read.ok {
        return handled(true);
    }
    let first_person = read.int(0, 0) != 0;
    let enable = read.int(1, 0) != 0;
    let path = read.text(2);
    let poll = read.int(3, 0) != 0;
    let condition = read.form(4);
    let target = if a.this_obj == 0 {
        Target::Every
    } else {
        Target::Actor(e.mem.u32(a.this_obj + FORM_ID))
    };
    let owner = owning_mod(e, a.script);
    let done = if enable {
        enable_path(e, target, first_person, owner, &path, condition, poll)
    } else {
        crate::disable_path(e, target, first_person, owner, &path) > 0
    };
    a.set_result(e, f64::from(u8::from(done)));
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

/// Registers the built commands at their opcodes: the table grown to reach them, each record with
/// its name, parameters and execute.
pub fn install(e: &mut Engine) {
    table::grow_to(e, SET_ACTOR_ANIMATION_PATH + 1);
    let execute = table::execute_address(SET_ACTOR_ANIMATION_PATH);
    e.register(execute, set_actor_animation_path);
    let params = guest_params(e, PARAMS);
    table::add_plugin_record(
        e,
        SET_ACTOR_ANIMATION_PATH,
        &PluginRecord {
            name: COMMAND_NAMES[0],
            short: "",
            needs_ref: false,
            param_count: PARAMS.len() as u16,
            params,
            execute,
            eval: 0,
            flags: 0,
            expression: false,
            returns: Returns::Default,
        },
    );
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
