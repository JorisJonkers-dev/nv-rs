// SPDX-License-Identifier: GPL-3.0-only
//! What the tests share: an engine with xNVSE's table and the plugin installed, stand-ins for the
//! game functions the plugin calls (the model loader, `KFModel`s with a manual reference count,
//! animation groups, the sequence constructor and the manager), and a call through a command's
//! record as `EvaluateLine` makes it.

use engine_core::prelude::*;
use std::cell::RefCell;

pub const DATA: u32 = 0x0300_0000;
pub const OFFSET: u32 = 0x0300_0f00;
pub const RESULT: u32 = 0x0300_0f08;
pub const SCRIPT: u32 = 0x0300_1000;
pub const LIST: u32 = 0x0300_2000;

/// The model loader's singleton, and where the stand-ins put their objects.
const LOADER: u32 = 0x0301_0000;
pub const MODELS: u32 = 0x0302_0000;
/// The `KFModel` stand-in: `+4` its sequence, `+8` its group, `+0x10` the manual references.
pub const KF_REFS: u32 = 0x10;

/// The `.kf` files the loader finds (lower-case path, model), and every
/// `(manager, sequence, name, store targets)` the manager was given, and every
/// `(block, group, source)` the constructor was asked for.
#[derive(Default)]
pub struct World {
    pub files: Vec<(Vec<u8>, u32)>,
    pub added: Vec<[u32; 4]>,
    pub built: Vec<[u32; 3]>,
    pub next_block: u32,
    pub refuse_add: bool,
    /// Extra names the file finder lists (as archives hold them: any case, maybe twice).
    pub listed: Vec<Vec<u8>>,
    /// Every `(pattern, path)` the file finder was asked for.
    pub searched: Vec<(Vec<u8>, Vec<u8>)>,
    /// Every `(animation, group, slot)` `Animation::StartGroup` was called with.
    pub started: Vec<[u32; 3]>,
    /// Names freed, lists deleted.
    pub freed: Vec<u32>,
    pub deleted: Vec<u32>,
}

/// Where the file finder stand-in puts its lists and names.
const LISTS: u32 = 0x0301_8000;

/// The file finder stand-in (`FileFinder::BuildFileList`): the files of [`World::files`] directly
/// in the pattern's folder and the [`World::listed`] names in it, each named by the directory
/// part of `path` (as the game names them), in a list of nodes `[item, next]`; 0 for none.
fn build_file_list(e: &mut Engine, a: &[u32]) -> Ret {
    let pattern = e.mem.cstr(a[0]).to_vec();
    let path = e.mem.cstr(a[1]).to_vec();
    let dir_end = path.iter().rposition(|&b| b == b'\\').map_or(0, |i| i + 1);
    let dir = path[..dir_end].to_ascii_lowercase();
    let names: Vec<Vec<u8>> = WORLD.with(|w| {
        let mut w = w.borrow_mut();
        w.searched.push((pattern, path.clone()));
        let mut names: Vec<Vec<u8>> = w
            .files
            .iter()
            .map(|(n, _)| n.clone())
            .chain(w.listed.iter().cloned())
            .filter(|n| {
                n.to_ascii_lowercase().starts_with(&dir) && !n[dir.len()..].contains(&b'\\')
            })
            .collect();
        names.reverse();
        names
    });
    if names.is_empty() {
        return Ret::default();
    }
    let list = LISTS;
    let mut node = list;
    let mut text = LISTS + 0x1000;
    for (i, name) in names.iter().enumerate() {
        e.mem.write(text, name);
        e.mem.set_u8(text + name.len() as u32, 0);
        e.mem.set_u32(node, text);
        let next = if i + 1 < names.len() {
            LISTS + 0x10 * (i as u32 + 1)
        } else {
            0
        };
        e.mem.set_u32(node + 4, next);
        node = next;
        text += 0x110;
    }
    Ret {
        eax: list,
        ..Ret::default()
    }
}

thread_local! {
    pub static WORLD: RefCell<World> = RefCell::new(World::default());
}

/// An engine with xNVSE's table, the stand-ins and the plugin answering "installed" as `on` says.
pub fn engine_as(on: bool) -> Engine {
    WORLD.with(|w| *w.borrow_mut() = World::default());
    crate::SAVED.with(|s| s.take());
    crate::seam::CONDITIONS.with(|c| *c.borrow_mut() = Default::default());
    let mut e = Engine::new();
    e.map(0x0118_e000, 0x9000);
    e.map(0x011c_3000, 0x1000);
    e.map(0x011d_e000, 0x1000);
    e.map(0x0300_0000, 0x40000);
    e.set_global(0x011c_3b3c, LOADER);
    e.register(0x0044_71c0, |e, a| {
        let name = e.mem.cstr(a[1]).to_ascii_lowercase();
        let kf = WORLD.with(|w| {
            w.borrow()
                .files
                .iter()
                .find(|(n, _)| *n == name)
                .map_or(0, |(_, kf)| *kf)
        });
        if kf != 0 {
            let refs = e.mem.u32(kf + KF_REFS);
            e.mem.set_u32(kf + KF_REFS, refs + 1);
        }
        Ret {
            eax: kf,
            ..Ret::default()
        }
    });
    e.register(crate::KF_MODEL_ADD_REF, |e, a| {
        let refs = e.mem.u32(a[0] + KF_REFS);
        e.mem.set_u32(a[0] + KF_REFS, refs + 1);
        Ret::default()
    });
    e.register(crate::KF_MODEL_RELEASE, |e, a| {
        let refs = e.mem.u32(a[0] + KF_REFS);
        assert!(refs > 0, "{:08x} released more often than referenced", a[0]);
        e.mem.set_u32(a[0] + KF_REFS, refs - 1);
        Ret::default()
    });
    e.register(crate::seam::KF_MODEL_SEQUENCE, |e, a| Ret {
        eax: e.mem.u32(a[0] + 4),
        ..Ret::default()
    });
    e.register(crate::seam::KF_MODEL_ANIM_GROUP, |e, a| Ret {
        eax: e.mem.u32(a[0] + 8),
        ..Ret::default()
    });
    e.register(0x0047_d3b0, |e, a| Ret {
        eax: u32::from(e.mem.u16(a[0])),
        ..Ret::default()
    });
    e.register(crate::seam::NI_OPERATOR_NEW, |_, _| {
        let block = WORLD.with(|w| {
            let mut w = w.borrow_mut();
            w.next_block += 1;
            0x0303_0000 + 0x80 * w.next_block
        });
        Ret {
            eax: block,
            ..Ret::default()
        }
    });
    e.register(crate::seam::ANIM_GROUP_SEQUENCE_CONSTRUCT, |_, a| {
        WORLD.with(|w| w.borrow_mut().built.push([a[0], a[1], a[2]]));
        Ret {
            eax: a[0],
            ..Ret::default()
        }
    });
    e.register(crate::seam::MANAGER_ADD_SEQUENCE, |_, a| {
        let refuse = WORLD.with(|w| {
            let mut w = w.borrow_mut();
            w.added.push([a[0], a[1], a[2], a[3]]);
            w.refuse_add
        });
        Ret {
            eax: u32::from(!refuse),
            ..Ret::default()
        }
    });
    e.register(0x00af_e420, build_file_list);
    // `Animation::StartGroup`: asks the seam, then plays its answer, else the game's own sequence
    // ([`GAME_SEQUENCE`]), in the slot.
    e.register(0x0049_48c0, |e, a| {
        WORLD.with(|w| w.borrow_mut().started.push([a[0], a[1], a[2]]));
        let sequence = play(e, a[0], a[1] as u16, a[2]);
        Ret {
            eax: sequence,
            ..Ret::default()
        }
    });
    e.register(0x0040_1030, |_, a| {
        WORLD.with(|w| w.borrow_mut().freed.push(a[0]));
        Ret::default()
    });
    // Takes the head off: the next node's words move up, or the list is emptied.
    e.register(0x0063_f7b0, |e, a| {
        let next = e.mem.u32(a[0] + 4);
        let (item, after) = if next == 0 {
            (0, 0)
        } else {
            (e.mem.u32(next), e.mem.u32(next + 4))
        };
        e.mem.set_u32(a[0], item);
        e.mem.set_u32(a[0] + 4, after);
        Ret::default()
    });
    e.register(0x0047_02f0, |_, a| {
        WORLD.with(|w| w.borrow_mut().deleted.push(a[0]));
        Ret::default()
    });
    nvse::table::install(&mut e);
    crate::install_as(&mut e, on);
    e
}

/// [`engine_as`] answering "installed".
pub fn engine() -> Engine {
    engine_as(true)
}

/// A `.kf` file at `path` the loader finds: its model, with a sequence and a group of id `group`.
/// `n` keeps the objects of several files apart.
pub fn kf_file(e: &mut Engine, n: u32, path: &str, group: u16) -> u32 {
    let kf = MODELS + 0x100 * n;
    let group_object = kf + 0x40;
    e.mem.set_u32(kf + 4, kf + 0x80);
    e.mem.set_u32(kf + 8, group_object);
    e.mem.set_u32(kf + KF_REFS, 0);
    e.mem.set_u16(group_object, group);
    WORLD.with(|w| {
        w.borrow_mut()
            .files
            .push((path.as_bytes().to_ascii_lowercase(), kf))
    });
    kf
}

/// The manual references on `kf`.
pub fn refs(e: &Engine, kf: u32) -> u32 {
    e.mem.u32(kf + KF_REFS)
}

/// Calls `opcode` through its record with `this_obj` and `args`: the handler's answer (`al`) and
/// the result double (it starts at 0, as the seams zero it).
pub fn call(e: &mut Engine, opcode: u32, this_obj: u32, args: &[Vec<u8>]) -> (u32, f64) {
    e.mem.set_u16(DATA, opcode as u16);
    let bytes: Vec<u8> = args.concat();
    e.mem.set_u16(DATA + 2, bytes.len() as u16 + 2);
    e.mem.set_u16(DATA + 4, args.len() as u16);
    for (i, b) in bytes.iter().enumerate() {
        e.mem.set_u8(DATA + 6 + i as u32, *b);
    }
    e.mem.set_u32(OFFSET, 4);
    e.mem.set_f64(RESULT, 0.0);
    let record = nvse::table::record_of(e, opcode);
    assert_ne!(record, 0, "{opcode:04x}");
    let params = e.mem.u32(record + 0x14);
    let execute = e.mem.u32(record + 0x18);
    let ok = e
        .call(
            execute,
            &[params, DATA, this_obj, 0, SCRIPT, LIST, RESULT, OFFSET],
        )
        .eax;
    (ok & 0xff, e.mem.f64(RESULT))
}

/// Compiled arguments: an immediate integer (`n`), a string (its length and bytes), a reference
/// of the script's list (`r` and its index).
pub fn n(v: i32) -> Vec<u8> {
    let mut b = vec![b'n'];
    b.extend_from_slice(&v.to_le_bytes());
    b
}
pub fn s(text: &str) -> Vec<u8> {
    let mut b = (text.len() as u16).to_le_bytes().to_vec();
    b.extend_from_slice(text.as_bytes());
    b
}
pub fn r(index: u16) -> Vec<u8> {
    let mut b = vec![b'r'];
    b.extend_from_slice(&index.to_le_bytes());
    b
}

/// Makes `form` reference `index` (from 1) of the script's reference list.
pub fn set_script_ref(e: &mut Engine, index: u16, form: u32) {
    let i = u32::from(index);
    let ref_var = 0x0300_a000 + i * 0x10;
    e.mem.set_u32(ref_var + 8, form);
    e.mem.set_u32(ref_var + 0xc, 0);
    let mut node = SCRIPT + 0x44;
    for at in 1..i {
        let next = e.mem.u32(node + 4);
        node = if next == 0 {
            let made = 0x0300_b000 + (at + 1) * 8;
            e.mem.set_u32(node + 4, made);
            made
        } else {
            next
        };
    }
    e.mem.set_u32(node, ref_var);
}

/// Makes the calling script belong to mod `index` (its form's file list holds one file).
pub fn set_script_mod(e: &mut Engine, index: u8) {
    let file = 0x0300_e000;
    e.mem.set_u32(SCRIPT + 0x10, file);
    e.mem.set_u32(SCRIPT + 0x14, 0);
    e.mem.set_u8(file + 0x40c, index);
}

/// The sequence the game's own path starts (the stand-in's sequence map).
pub const GAME_SEQUENCE: u32 = 0x0303_f000;

/// A group started in `slot` as `Animation::StartGroup` starts it: the seam's answer, else
/// [`GAME_SEQUENCE`], put in the slot with the group (`+0x4c`, `+0xe0`).
pub fn play(e: &mut Engine, animation: u32, group: u16, slot: u32) -> u32 {
    let sequence = e
        .site(0x0049_48c0, &[animation, u32::from(group), slot])
        .unwrap_or(GAME_SEQUENCE);
    e.mem.set_u16(animation + 0x4c + 2 * slot, group);
    e.mem.set_u32(animation + 0xe0 + 4 * slot, sequence);
    sequence
}

/// The sequence playing in `animation`'s `slot`.
pub fn playing(e: &Engine, animation: u32, slot: u32) -> u32 {
    e.mem.u32(animation + 0xe0 + 4 * slot)
}

/// The `StartGroup` calls so far, taken.
pub fn started() -> Vec<[u32; 3]> {
    WORLD.with(|w| std::mem::take(&mut w.borrow_mut().started))
}
