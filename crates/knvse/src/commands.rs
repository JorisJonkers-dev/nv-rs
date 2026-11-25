// SPDX-License-Identifier: GPL-3.0-only
//! kNVSE's commands built so far, from the plugin's public name and parameter list and the GECK
//! Wiki's description (`NOTICE`, `provenance/02-set-actor-animation-path.md`).
//!
//! - `SetActorAnimationPath firstPerson enable path [pollCondition] [condition]
//!   [matchBaseAnimGroupId]` (`0x3922`), on an actor reference or on none (every actor): with
//!   `enable`, loads the `.kf` file at `path` (`ModelLoader::LoadKF`) and puts it on the store's
//!   stack for the actor (or every actor), the person and the animation group the file names. A
//!   `path` that does not end in `.kf` is a folder: every `.kf` file in it (loose or archived)
//!   is set so, one per group. The condition function, when one is given, must return true for
//!   the actor at each start of the group, and with the poll flag also while the group plays
//!   (`seam::poll`). Without `enable`, takes the calling mod's overrides of that path off again.
//!   Answers 1 when an override was set or taken off, else 0.
//!
//! No other kNVSE command is built; a script that calls one fails at that line, as without the
//! plugin.

use crate::store::{Key, Override, Target};
use engine_core::prelude::*;
use nvse::commands::Read;
use nvse::table::{self, Param, PluginRecord, Returns};
use nvse::{handled, CommandArgs};
use std::collections::BTreeSet;

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
/// `FileFinder::BuildFileList(pattern, path, archive types, list)` (cdecl): the loose and the
/// archived files matching `pattern`, each named by the directory part of `path` and its file
/// name, in a new list (0 for none). Type 1: the meshes archives, as `ModelLoader` asks.
const BUILD_FILE_LIST: u32 = 0x00af_e420;
const MESH_ARCHIVES: u32 = 1;
/// What `ModelLoader` gives such a list back with: each name freed (`00401030`), the head taken
/// off (`0063f7b0`), the emptied list deleted (`004702f0`).
const MEMORY_FREE: u32 = 0x0040_1030;
const LIST_REMOVE_HEAD: u32 = 0x0063_f7b0;
const LIST_DELETE: u32 = 0x0047_02f0;

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

/// Whether `path` names one `.kf` file; anything else is a folder of them.
fn is_kf_file(path: &[u8]) -> bool {
    path.len() > 3 && path[path.len() - 3..].eq_ignore_ascii_case(b".kf")
}

/// `path` as the store keeps it ([`crate::store::normal_path`]) without a trailing `\`, so a
/// folder given with and without one is the same override.
fn store_path(path: &[u8]) -> Vec<u8> {
    let mut p = crate::store::normal_path(path);
    while p.last() == Some(&b'\\') {
        p.pop();
    }
    p
}

/// The `.kf` files in the folder `folder` (under `Data\Meshes`, loose and in the meshes
/// archives, as `ModelLoader::BuildKFFileList` searches it, but without handing the folder to the
/// idle manager): their paths as the loader takes them, sorted, each once. None for a folder too
/// long for the game's buffers.
fn folder_files(e: &mut Engine, folder: &[u8]) -> Vec<Vec<u8>> {
    const ROOT: &[u8] = b"Data\\Meshes\\";
    const FILES: &[u8] = b"\\*.kf";
    if folder.is_empty()
        || folder.contains(&0)
        || ROOT.len() + folder.len() + FILES.len() > MAX_PATH
    {
        return Vec::new();
    }
    let pattern = [ROOT, folder, FILES].concat();
    let names_from = [folder, FILES].concat();
    let room = (MAX_PATH as u32 + 1 + 3) & !3;
    let list = e.with_stack(2 * room, |e, at| {
        let (p, n) = (at.addr(), at.addr() + room);
        e.mem.write(p, &pattern);
        e.mem.set_u8(p + pattern.len() as u32, 0);
        e.mem.write(n, &names_from);
        e.mem.set_u8(n + names_from.len() as u32, 0);
        e.call(BUILD_FILE_LIST, &args![p, n, MESH_ARCHIVES, 0u32])
            .u32()
    });
    if list == 0 {
        return Vec::new();
    }
    let mut names = BTreeSet::new();
    let mut node = list;
    let mut nodes = 0usize;
    while node != 0 {
        let item = e.mem.u32(node);
        if item != 0 {
            names.insert(crate::store::normal_path(&e.mem.cstr(item)));
        }
        node = e.mem.u32(node + 4);
        nodes += 1;
    }
    // Given back as the game gives its own lists back; at most once per node, whatever the list.
    for _ in 0..nodes {
        if e.mem.u32(list) == 0 && e.mem.u32(list + 4) == 0 {
            break;
        }
        let item = e.mem.u32(list);
        if item != 0 {
            e.call(MEMORY_FREE, &args![item]);
        }
        e.call(LIST_REMOVE_HEAD, &args![list]);
    }
    e.call(LIST_DELETE, &args![list, 1u32]);
    names.into_iter().filter(|n| is_kf_file(n)).collect()
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

/// The model of the `.kf` file `file` and the id of the group it names, with the load's manual
/// reference; none (and no reference kept) when it cannot be loaded or names no group.
fn load_with_group(e: &mut Engine, file: &[u8]) -> Option<(u32, u16)> {
    let kf = load_kf(e, file);
    if kf == 0 {
        return None;
    }
    let group = e.call(crate::seam::KF_MODEL_ANIM_GROUP, &args![kf]).u32();
    if group == 0 {
        e.call(crate::KF_MODEL_RELEASE, &args![kf]);
        return None;
    }
    Some((kf, e.call(ANIM_GROUP_ID, &args![group]).u16()))
}

/// Sets the overrides of `path` (as [`store_path`] keeps it) for `target` and person: the file,
/// or each file of the folder, loaded, its group read, put on top of its stack under `path`. In
/// a folder, the first file in name order that names a group is that group's; the others of the
/// same group are not set. Whether one was set.
fn enable_path(
    e: &mut Engine,
    target: Target,
    first_person: bool,
    owner: u32,
    path: &[u8],
    condition: u32,
    poll: bool,
) -> bool {
    // The loader is handed the paths as the store keeps them (lower case, `\` separators).
    let files = if is_kf_file(path) {
        vec![path.to_vec()]
    } else {
        folder_files(e, path)
    };
    let mut groups = BTreeSet::new();
    for file in files {
        let Some((kf, id)) = load_with_group(e, &file) else {
            continue;
        };
        if !groups.insert(id) {
            e.call(crate::KF_MODEL_RELEASE, &args![kf]);
            continue;
        }
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
    }
    !groups.is_empty()
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
    let path = store_path(&read.text(2));
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
