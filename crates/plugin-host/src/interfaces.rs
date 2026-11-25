// SPDX-License-Identifier: GPL-3.0-only
//! The interfaces `NVSEInterface::QueryInterface` hands a plugin, other than messaging: guest
//! tables of function words, each word a thunk whose host function is the `nvse` crate's own
//! (xNVSE built in, ADR-0016). Layouts and ids are those of xNVSE 6.4.9's `PluginAPI.h`, the
//! tables' order that of its `PluginManager.cpp`:
//!
//! | id | interface | what answers |
//! |---:|---|---|
//! | 0 | serialization | co-save records through the built-in co-save ([`nvse::saveload`]) |
//! | 1 | console | `RunScriptLine`, `RunScriptLine2` ([`nvse::compile_script`]) |
//! | 3 | command table | xNVSE's table in guest memory ([`nvse::table`], [`nvse::plugins`]) |
//! | 4 | string variables | [`nvse::vars`], `Assign` as xNVSE's string commands assign |
//! | 5 | array variables | [`nvse::vars`] |
//! | 6 | script | user functions ([`nvse::functions`]), the argument reader ([`nvse::args`]), the run-time compile |
//! | 7 | data | the lambda functions and the script data cache; xNVSE's C++ singletons are not given |
//! | 8 | event manager | `DispatchEvent` ([`nvse::events`]) |
//! | 9 | logging | the plugin log folder: the game folder |
//!
//! A slot with nothing behind it yet is a thunk that stops the call and names itself
//! (`NVSEScriptInterface.DecompileToBuffer`), so the report says what the plugin wanted, never a
//! read through a null word.

use crate::crt::{rd, wr};
use crate::x86::Stop;
use crate::{current, guarded, report, state, thunk, HostFn};
use engine_core::prelude::*;
use nvse::cosave::{Chunk, OutOfRange, Reader};
use nvse::plugins::Records;
use nvse::vars::{self, Data, Key, Value};

/// `LookupFormByID` (`004839c0`): a form id to its address, as xNVSE gives forms to plugins.
pub const LOOKUP_FORM: u32 = 0x0048_39c0;
/// The most arguments a variadic call reads: xNVSE's user functions take at most 15.
const VARIADIC: u32 = 15;
/// `PluginInfo::kInfoVersion`.
const INFO_VERSION: u32 = 1;
/// The longest text a plugin passes that is read (xNVSE's longest string is `kMaxMessageLength`,
/// 0x4000; a text with no NUL within this stops the call instead of reading on).
const MAX_TEXT: u32 = 0x10_0000;
/// The most bytes one co-save record write reads from the plugin.
const MAX_RECORD: u32 = 0x100_0000;
/// The most elements one `Create*` call reads.
const MAX_ELEMENTS: u32 = 0x10_0000;

/// One function word of a table: its name in the report, the host function (none: the call
/// stops and names it), its fixed argument words and how many more a variadic call may pass.
struct Slot(&'static str, Option<HostFn>, u32, u32);

/// The serialization state of one plugin: its callbacks' guest addresses.
#[derive(Clone, Copy, Debug, Default)]
pub struct Callbacks {
    pub save: u32,
    pub load: u32,
    pub pre_load: u32,
    pub new_game: u32,
}

/// The records of a co-save section being read: the chunks, the next one, the open one and how
/// far into it the plugin has read.
#[derive(Default)]
pub struct Reading {
    chunks: Vec<Chunk>,
    next: usize,
    open: Option<(usize, usize)>,
}

/// What the interfaces keep.
#[derive(Default)]
pub struct Ifaces {
    /// The table of each id, 0 for one not given.
    pub tables: [u32; 11],
    /// A `PluginInfo` in guest memory for each plugin name asked for.
    infos: std::collections::HashMap<String, u32>,
    /// The guest copy of each string variable's text `GetString` answered, and its text.
    texts: std::collections::HashMap<u32, (u32, Vec<u8>)>,
    /// The records a save callback is writing, while it runs.
    pub writing: Option<Records>,
    /// The section a load callback is reading, while it runs.
    pub reading: Option<Reading>,
    /// The word `GetData(kNVSEData_NumPreloadMods)` points at.
    preload_word: u32,
    /// The empty text the log path is.
    log_path: u32,
}

fn ifaces(e: &mut Engine) -> &mut Ifaces {
    &mut state(e).ifaces
}

fn fault(addr: u32, write: bool) -> Stop {
    Stop::Fault { addr, write }
}

/// A NUL-ended text a plugin passed (0: none), read a page at a time: one that runs into
/// unmapped memory or past [`MAX_TEXT`] stops the call.
fn text(e: &Engine, at: u32) -> Result<Vec<u8>, Stop> {
    const PAGE: u32 = 0x1000;
    let mut out = Vec::new();
    if at == 0 {
        return Ok(out);
    }
    let mut p = at;
    loop {
        let n = PAGE - (p & (PAGE - 1));
        let mut chunk = vec![0u8; n as usize];
        e.mem.try_read(p, &mut chunk).map_err(|_| fault(p, false))?;
        if let Some(end) = chunk.iter().position(|&b| b == 0) {
            out.extend_from_slice(&chunk[..end]);
            return Ok(out);
        }
        out.extend_from_slice(&chunk);
        if out.len() as u32 >= MAX_TEXT {
            return Err(fault(p, false));
        }
        p = p.checked_add(n).ok_or_else(|| fault(p, false))?;
    }
}

/// `len` bytes a plugin passed for a co-save record; more than [`MAX_RECORD`] stops the call.
fn record_bytes(e: &mut Engine, at: u32, len: u32) -> Result<Vec<u8>, Stop> {
    if len > MAX_RECORD {
        return Err(fault(at, false));
    }
    rd(e, at, len)
}

/// The address of element `i` of a plugin's list at `base`, `size` bytes each: past the end of
/// the address space stops the call.
fn nth(base: u32, size: u32, i: u32) -> Result<u32, Stop> {
    size.checked_mul(i)
        .and_then(|off| base.checked_add(off))
        .ok_or_else(|| fault(base, false))
}

/// A plugin's element count: more than [`MAX_ELEMENTS`] stops the call.
fn count(n: u32, at: u32) -> Result<u32, Stop> {
    if n > MAX_ELEMENTS {
        return Err(fault(at, false));
    }
    Ok(n)
}

fn put32(e: &mut Engine, at: u32, v: u32) -> Result<(), Stop> {
    wr(e, at, &v.to_le_bytes())
}

fn get32(e: &mut Engine, at: u32) -> Result<u32, Stop> {
    let b = rd(e, at, 4)?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// A text copied into guest memory for a plugin to keep (NUL-ended).
fn guest_text(e: &mut Engine, bytes: &[u8]) -> u32 {
    let at = e.mem.alloc(bytes.len() as u32 + 1);
    e.mem.write(at, bytes);
    e.mem.set_u8(at + bytes.len() as u32, 0);
    at
}

fn table(e: &mut Engine, iface: &str, version: Option<u32>, slots: &[Slot]) -> u32 {
    let head = u32::from(version.is_some());
    let at = e.mem.alloc(4 * (head + slots.len() as u32));
    if let Some(v) = version {
        e.mem.set_u32(at, v);
    }
    for (i, Slot(name, f, argc, extra)) in slots.iter().enumerate() {
        let word = thunk(e, &format!("{iface}.{name}"), *f, *argc, false);
        if *extra > 0 {
            crate::set_variadic(e, word, *extra);
        }
        e.mem.set_u32(at + 4 * (head + i as u32), word);
    }
    at
}

/// Builds every table once (the first plugin's mapping).
pub(crate) fn build(e: &mut Engine) {
    if ifaces(e).tables[3] != 0 {
        return;
    }
    let t = [
        table(
            e,
            "NVSESerializationInterface",
            Some(2),
            &[
                Slot("SetSaveCallback", Some(ser_set_save), 2, 0),
                Slot("SetLoadCallback", Some(ser_set_load), 2, 0),
                Slot("SetNewGameCallback", Some(ser_set_new_game), 2, 0),
                Slot("WriteRecord", Some(ser_write_record), 4, 0),
                Slot("OpenRecord", Some(ser_open_record), 2, 0),
                Slot("WriteRecordData", Some(ser_write_data), 2, 0),
                Slot("GetNextRecordInfo", Some(ser_next_record), 3, 0),
                Slot("ReadRecordData", Some(ser_read_data), 2, 0),
                Slot("ResolveRefID", Some(ser_resolve), 2, 0),
                Slot("SetPreLoadCallback", Some(ser_set_pre_load), 2, 0),
                Slot("GetSavePath", None, 0, 0),
                Slot("PeekRecordData", Some(ser_peek_data), 2, 0),
                Slot("WriteRecord8", Some(ser_write8), 1, 0),
                Slot("WriteRecord16", Some(ser_write16), 1, 0),
                Slot("WriteRecord32", Some(ser_write32), 1, 0),
                Slot("WriteRecord64", Some(ser_write64), 1, 0),
                Slot("ReadRecord8", Some(ser_read8), 0, 0),
                Slot("ReadRecord16", Some(ser_read16), 0, 0),
                Slot("ReadRecord32", Some(ser_read32), 0, 0),
                Slot("ReadRecord64", Some(ser_read64), 1, 0),
                Slot("SkipNBytes", Some(ser_skip), 1, 0),
            ],
        ),
        table(
            e,
            "NVSEConsoleInterface",
            Some(3),
            &[
                Slot("RunScriptLine", Some(console_run_line), 2, 0),
                Slot("RunScriptLine2", Some(console_run_line2), 3, 0),
            ],
        ),
        0,
        table(
            e,
            "NVSECommandTableInterface",
            Some(2),
            &[
                Slot("Start", Some(cmd_start), 0, 0),
                Slot("End", Some(cmd_end), 0, 0),
                Slot("GetByOpcode", Some(cmd_by_opcode), 1, 0),
                Slot("GetByName", Some(cmd_by_name), 1, 0),
                Slot("GetReturnType", Some(cmd_return_type), 1, 0),
                Slot("GetRequiredNVSEVersion", None, 1, 0),
                Slot("GetParentPlugin", Some(cmd_parent_plugin), 1, 0),
                Slot("GetPluginInfoByName", Some(cmd_info_by_name), 1, 0),
                Slot("GetPluginInfoByDLLName", Some(cmd_info_by_dll), 1, 0),
            ],
        ),
        table(
            e,
            "NVSEStringVarInterface",
            Some(1),
            &[
                Slot("GetString", Some(str_get), 1, 0),
                Slot("SetString", Some(str_set), 2, 0),
                Slot("CreateString", Some(str_create), 2, 0),
                Slot("Register", Some(str_register), 1, 0),
                Slot("Assign", Some(str_assign), 9, 0),
            ],
        ),
        // No version word: the array table starts with its first function (`PluginAPI.h`).
        table(
            e,
            "NVSEArrayVarInterface",
            None,
            &[
                Slot("CreateArray", Some(arr_create), 3, 0),
                Slot("CreateStringMap", Some(arr_create_string_map), 4, 0),
                Slot("CreateMap", Some(arr_create_map), 4, 0),
                Slot("AssignCommandResult", Some(arr_assign_result), 2, 0),
                Slot("SetElement", Some(arr_set_element), 3, 0),
                Slot("AppendElement", Some(arr_append), 2, 0),
                Slot("GetArraySize", Some(arr_size), 1, 0),
                Slot("LookupArrayByID", Some(arr_lookup), 1, 0),
                Slot("GetElement", Some(arr_get_element), 3, 0),
                Slot("GetElements", Some(arr_get_elements), 3, 0),
                Slot("GetArrayPacked", Some(arr_packed), 1, 0),
                Slot("GetContainerType", Some(arr_container_type), 1, 0),
                Slot("ArrayHasKey", Some(arr_has_key), 2, 0),
            ],
        ),
        // No version word either.
        table(
            e,
            "NVSEScriptInterface",
            None,
            &[
                Slot("CallFunction", Some(script_call), 5, VARIADIC),
                Slot("GetFunctionParams", Some(script_params), 2, 0),
                Slot("ExtractArgsEx", Some(script_extract_args), 5, 0x20),
                Slot("ExtractFormatStringArgs", None, 8, 0),
                Slot("CallFunctionAlt", Some(script_call_alt), 3, VARIADIC),
                Slot("CompileScript", Some(script_compile), 1, 0),
                Slot("CompileExpression", Some(script_compile_expression), 1, 0),
                Slot("DecompileToBuffer", None, 3, 0),
            ],
        ),
        table(
            e,
            "NVSEDataInterface",
            Some(4),
            &[
                Slot("GetSingleton", Some(data_singleton), 1, 0),
                Slot("GetFunc", Some(data_func), 1, 0),
                Slot("GetData", Some(data_data), 1, 0),
                Slot("ClearScriptDataCache", Some(data_clear_cache), 0, 0),
            ],
        ),
        // No version word: the event manager's table starts with `RegisterEvent`.
        table(
            e,
            "NVSEEventManagerInterface",
            None,
            &[
                Slot("RegisterEvent", None, 4, 0),
                Slot("DispatchEvent", Some(event_dispatch), 2, VARIADIC),
                Slot("DispatchEventAlt", None, 4, 0),
                Slot("SetNativeEventHandler", None, 2, 0),
                Slot("RemoveNativeEventHandler", None, 2, 0),
                Slot("RegisterEventWithAlias", None, 5, 0),
                Slot("DispatchEventThreadSafe", None, 3, 0),
                Slot("DispatchEventAltThreadSafe", None, 5, 0),
                Slot("SetNativeHandlerFunctionValue", None, 1, 0),
                Slot("SetNativeEventHandlerWithPriority", None, 5, 0),
                Slot("RemoveNativeEventHandlerWithPriority", None, 3, 0),
                Slot("IsEventHandlerFirst", None, 5, 0),
                Slot("IsEventHandlerLast", None, 5, 0),
                Slot("GetHigherPriorityEventHandlers", None, 5, 0),
                Slot("GetLowerPriorityEventHandlers", None, 5, 0),
            ],
        ),
        table(
            e,
            "NVSELoggingInterface",
            None,
            &[Slot("GetPluginLogPath", Some(log_path), 0, 0)],
        ),
    ];
    let i = ifaces(e);
    i.tables[..10].copy_from_slice(&t);
}

/// The table `QueryInterface(id)` answers (0 for none; messaging is the caller's).
pub(crate) fn query(e: &mut Engine, id: u32) -> u32 {
    if id == 2 {
        return 0;
    }
    ifaces(e).tables.get(id as usize).copied().unwrap_or(0)
}

// ---- serialization ---------------------------------------------------------------------------

fn handle_plugin(e: &mut Engine, handle: u32) -> Option<usize> {
    let n = state(e).plugins.len();
    ((handle as usize) < n && !state(e).plugins[handle as usize].runtime).then_some(handle as usize)
}

/// A plugin sets only its own callbacks: a handle that is no plugin, or another plugin's, sets
/// nothing (the callback would run in that plugin's name).
fn set_callback(e: &mut Engine, a: &[u32], which: fn(&mut Callbacks) -> &mut u32) -> u32 {
    let Some(p) = handle_plugin(e, a[0]) else {
        return 0;
    };
    let Ok(me) = current(e) else {
        return 0;
    };
    if me != p {
        let file = state(e).plugins[me].file.clone();
        report(
            e,
            format!("{file}: a co-save callback for another plugin's handle is not set"),
        );
        return 0;
    }
    let plugin = &mut state(e).plugins[p];
    *which(&mut plugin.callbacks) = a[1];
    if plugin.loaded {
        // xNVSE takes callbacks at any time; the co-save hooks of a loaded plugin are fixed when
        // it is put in the list, so one set later is not called.
        let file = plugin.file.clone();
        report(
            e,
            format!("{file}: a co-save callback set after NVSEPlugin_Load is not called"),
        );
    }
    0
}

fn ser_set_save(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    Ok(set_callback(e, a, |c| &mut c.save))
}
fn ser_set_load(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    Ok(set_callback(e, a, |c| &mut c.load))
}
fn ser_set_new_game(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    Ok(set_callback(e, a, |c| &mut c.new_game))
}
fn ser_set_pre_load(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    Ok(set_callback(e, a, |c| &mut c.pre_load))
}

fn with_records(e: &mut Engine, f: impl FnOnce(&mut Records)) -> u32 {
    match ifaces(e).writing.as_mut() {
        Some(r) => {
            f(r);
            1
        }
        None => 0,
    }
}

fn ser_write_record(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let bytes = record_bytes(e, a[2], a[3])?;
    Ok(with_records(e, |r| r.record(a[0], a[1], &bytes)))
}
fn ser_open_record(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    Ok(with_records(e, |r| r.open(a[0], a[1])))
}
fn ser_write_data(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let bytes = record_bytes(e, a[0], a[1])?;
    let open = ifaces(e)
        .writing
        .as_ref()
        .is_some_and(|r| !r.chunks.is_empty());
    with_records(e, |r| r.data(&bytes));
    Ok(u32::from(open))
}
fn ser_write8(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    with_records(e, |r| r.u8(a[0] as u8));
    Ok(0)
}
fn ser_write16(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    with_records(e, |r| r.u16(a[0] as u16));
    Ok(0)
}
fn ser_write32(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    with_records(e, |r| r.u32(a[0]));
    Ok(0)
}
fn ser_write64(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let bytes = rd(e, a[0], 8)?;
    with_records(e, |r| r.data(&bytes));
    Ok(0)
}

/// `GetNextRecordInfo(type*, version*, length*)`: the next record of the section, the rest of
/// the open one skipped; false after the last.
fn ser_next_record(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let Some(r) = ifaces(e).reading.as_mut() else {
        return Ok(0);
    };
    let Some(c) = r.chunks.get(r.next) else {
        r.open = None;
        return Ok(0);
    };
    let (kind, version, len) = (c.kind, c.version, c.data.len() as u32);
    r.open = Some((r.next, 0));
    r.next += 1;
    put32(e, a[0], kind)?;
    put32(e, a[1], version)?;
    put32(e, a[2], len)?;
    Ok(1)
}

/// Up to `len` bytes of the open record from where the plugin is (`advance`: moves on).
fn take(e: &mut Engine, len: u32, advance: bool) -> Vec<u8> {
    let Some(r) = ifaces(e).reading.as_mut() else {
        return Vec::new();
    };
    let Some((i, pos)) = r.open else {
        return Vec::new();
    };
    let data = &r.chunks[i].data;
    let end = data.len().min(pos + len as usize);
    let out = data[pos..end].to_vec();
    if advance {
        r.open = Some((i, end));
    }
    out
}

fn ser_read_data(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let bytes = take(e, a[1], true);
    wr(e, a[0], &bytes)?;
    Ok(bytes.len() as u32)
}
fn ser_peek_data(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let bytes = take(e, a[1], false);
    wr(e, a[0], &bytes)?;
    Ok(bytes.len() as u32)
}
fn read_n(e: &mut Engine, n: u32) -> u32 {
    let mut b = take(e, n, true);
    b.resize(4, 0);
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
fn ser_read8(e: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    Ok(read_n(e, 1))
}
fn ser_read16(e: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    Ok(read_n(e, 2))
}
fn ser_read32(e: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    Ok(read_n(e, 4))
}
fn ser_read64(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let mut b = take(e, 8, true);
    b.resize(8, 0);
    wr(e, a[0], &b)?;
    Ok(0)
}
fn ser_skip(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    take(e, a[0], true);
    Ok(0)
}
fn ser_resolve(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    match nvse::saveload::resolve_ref_id(e, a[0]) {
        Some(id) => {
            put32(e, a[1], id)?;
            Ok(1)
        }
        None => Ok(0),
    }
}

/// The co-save hooks of plugin `N`, given to xNVSE's list ([`hooks`]).
fn save_hook<const N: usize>(e: &mut Engine, out: &mut Records) {
    let cb = state(e).plugins[N].callbacks.save;
    if cb == 0 {
        return;
    }
    ifaces(e).writing = Some(std::mem::take(out));
    guarded(e, N, "co-save write", cb, &[0]);
    *out = ifaces(e).writing.take().unwrap_or_default();
}

fn read_hook(e: &mut Engine, n: usize, cb: u32, r: &mut Reader) -> Result<(), OutOfRange> {
    let chunks = nvse::plugins::read_chunks(r)?;
    if cb == 0 {
        return Ok(());
    }
    let outer = ifaces(e).reading.replace(Reading {
        chunks,
        ..Reading::default()
    });
    guarded(e, n, "co-save read", cb, &[0]);
    ifaces(e).reading = outer;
    Ok(())
}

fn load_hook<const N: usize>(e: &mut Engine, r: &mut Reader) -> Result<(), OutOfRange> {
    let cb = state(e).plugins[N].callbacks.load;
    read_hook(e, N, cb, r)
}

fn pre_load_hook<const N: usize>(e: &mut Engine, r: &mut Reader) -> Result<(), OutOfRange> {
    let cb = state(e).plugins[N].callbacks.pre_load;
    read_hook(e, N, cb, r)
}

fn new_game_hook<const N: usize>(e: &mut Engine) {
    let cb = state(e).plugins[N].callbacks.new_game;
    if cb != 0 {
        guarded(e, N, "new-game callback", cb, &[0]);
    }
}

type Hooks = (
    nvse::plugins::SaveCallback,
    nvse::plugins::LoadCallback,
    nvse::plugins::LoadCallback,
    fn(&mut Engine),
);

macro_rules! hooks {
    ($($n:literal)*) => { [$((save_hook::<$n> as nvse::plugins::SaveCallback, load_hook::<$n> as nvse::plugins::LoadCallback, pre_load_hook::<$n> as nvse::plugins::LoadCallback, new_game_hook::<$n> as fn(&mut Engine))),*] };
}

const HOOKS: [Hooks; crate::MAX_PLUGINS] = hooks!(
    0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31
    32 33 34 35 36 37 38 39 40 41 42 43 44 45 46 47 48 49 50 51 52 53 54 55 56 57 58 59 60 61 62 63
);

/// The co-save hooks for plugin `idx`, by the callbacks it set: (save, load, pre-load, new game).
pub(crate) fn hooks(e: &mut Engine, idx: usize) -> Hooks4 {
    let c = state(e).plugins[idx].callbacks;
    let h = HOOKS[idx];
    (
        (c.save != 0).then_some(h.0),
        (c.load != 0).then_some(h.1),
        (c.pre_load != 0).then_some(h.2),
        (c.new_game != 0).then_some(h.3),
    )
}

pub(crate) type Hooks4 = (
    Option<nvse::plugins::SaveCallback>,
    Option<nvse::plugins::LoadCallback>,
    Option<nvse::plugins::LoadCallback>,
    Option<fn(&mut Engine)>,
);

// ---- console ---------------------------------------------------------------------------------

fn console_run_line(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let line = text(e, a[0])?;
    Ok(u32::from(nvse::compile_script::run_script_line(
        e, &line, a[1],
    )))
}
fn console_run_line2(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let line = text(e, a[0])?;
    Ok(u32::from(nvse::compile_script::run_script_line2(
        e,
        &line,
        a[1],
        a[2] & 0xff != 0,
    )))
}

// ---- command table ---------------------------------------------------------------------------

fn cmd_start(e: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    let t = e.script_tables();
    Ok(t.record(engine_core::extend::ScriptTables::FIRST_OPCODE))
}
fn cmd_end(e: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    let t = e.script_tables();
    Ok(t.record(t.max_opcode))
}
fn cmd_by_opcode(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    Ok(nvse::table::record_of(e, a[0]))
}
fn cmd_by_name(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let name = text(e, a[0])?;
    Ok(nvse::table::opcode_by_name(e, &name).map_or(0, |op| nvse::table::record_of(e, op)))
}
/// The opcode word of a command record a plugin holds (`CommandInfo::opcode`, `+8`).
fn opcode_of(e: &mut Engine, record: u32) -> Result<u32, Stop> {
    if record == 0 {
        return Ok(0);
    }
    get32(e, record.wrapping_add(8))
}
fn cmd_return_type(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    use nvse::table::Returns;
    let op = opcode_of(e, a[0])?;
    Ok(match nvse::table::return_type(e, op) {
        Returns::Default => 0,
        Returns::Form => 1,
        Returns::String => 2,
        Returns::Array => 3,
        Returns::ArrayIndex => 4,
        Returns::Ambiguous => 5,
    })
}

/// A `PluginInfo` (`infoVersion`, `name`, `version`) in guest memory, one for each name.
fn info(e: &mut Engine, name: &str, version: u32) -> u32 {
    if let Some(&at) = ifaces(e).infos.get(name) {
        return at;
    }
    let text = guest_text(e, name.as_bytes());
    let at = e.mem.alloc(12);
    e.mem.set_u32(at, INFO_VERSION);
    e.mem.set_u32(at + 4, text);
    e.mem.set_u32(at + 8, version);
    ifaces(e).infos.insert(name.to_string(), at);
    at
}

/// xNVSE's `kNVSEOpcodeStart` and `kNVSEOpcodeTest` (`CommandTable.h`): below the first a
/// command is the game's, below the second xNVSE's own.
const NVSE_OPCODE_START: u32 = 0x1400;
const NVSE_OPCODE_TEST: u32 = 0x2000;

fn cmd_parent_plugin(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let op = opcode_of(e, a[0])?;
    if op < NVSE_OPCODE_START {
        return Ok(0);
    }
    if op < NVSE_OPCODE_TEST {
        // `g_NVSEPluginInfo` (xNVSE `CommandTable.cpp`) carries `NVSE_VERSION_INTEGER`.
        return Ok(info(e, "NVSE", nvse::VERSION.0));
    }
    Ok(match nvse::plugins::parent_of(e, op, false) {
        Some(p) => info(e, p.name, p.version),
        None => 0,
    })
}
fn cmd_info_by_name(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let name = text(e, a[0])?;
    Ok(match nvse::plugins::listed(e, &name) {
        Some((name, version)) => info(e, name, version),
        None => 0,
    })
}
fn cmd_info_by_dll(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let file = text(e, a[0])?;
    let found = nvse::plugins::built_ins(e)
        .into_iter()
        .filter(|p| p.listed)
        .find(|p| p.file.as_bytes().eq_ignore_ascii_case(&file));
    Ok(match found {
        Some(p) => info(e, p.name, p.version),
        None => 0,
    })
}

// ---- string variables ------------------------------------------------------------------------

/// `GetString(id)`: the text, valid while the variable keeps it (one guest copy a variable, made
/// again when its text changed); 0 for no such variable.
fn str_get(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let Some(now) = vars::string(e, a[0]) else {
        return Ok(0);
    };
    if let Some((at, was)) = ifaces(e).texts.get(&a[0]) {
        if *was == now {
            return Ok(*at);
        }
    }
    let at = guest_text(e, &now);
    if let Some((old, _)) = ifaces(e).texts.insert(a[0], (at, now)) {
        e.mem.free(old);
    }
    Ok(at)
}
fn str_set(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let value = text(e, a[1])?;
    vars::set_string(e, a[0], &value);
    Ok(0)
}
/// `CreateString(value, owningScript)`: owned by the script's mod; 0 with no script.
fn str_create(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    if a[1] == 0 {
        return Ok(0);
    }
    let value = text(e, a[0])?;
    let modi = vars::script_mod_index(e, a[1]);
    Ok(vars::add_string(e, modi, &value, false))
}
/// `Register`: xNVSE keeps the interface for its `%z` format; nv-rs's format reads string
/// variables itself.
fn str_register(_: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    Ok(0)
}
/// `Assign(COMMAND_ARGS, value)`: the command's string result.
fn str_assign(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let value = text(e, a[8])?;
    let args = nvse::CommandArgs::new(&a[..8]);
    nvse::commands::string::assign_to_string_var(e, &args, &value);
    Ok(1)
}

// ---- array variables -------------------------------------------------------------------------

/// `NVSEArrayVarInterface::Element` (16 bytes: the value, then the type byte at `+8`).
const ELEMENT: u32 = 16;
const T_NUMBER: u8 = 1;
const T_FORM: u8 = 2;
const T_STRING: u8 = 3;
const T_ARRAY: u8 = 4;

/// A plugin's element, read: `None` for one of no type.
fn read_element(e: &mut Engine, at: u32) -> Result<Option<Value>, Stop> {
    let b = rd(e, at, 9)?;
    let word = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    Ok(match b[8] {
        T_NUMBER => Some(Value::Number(f64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))),
        // A form's address: its id at `+0xC` (`TESForm::refID`).
        T_FORM => Some(Value::Form(if word == 0 {
            0
        } else {
            get32(e, word.wrapping_add(0xc))?
        })),
        T_STRING => Some(Value::Str(text(e, word)?)),
        T_ARRAY => Some(Value::Array(word)),
        _ => None,
    })
}

fn read_key(e: &mut Engine, at: u32) -> Result<Option<Key>, Stop> {
    Ok(match read_element(e, at)? {
        Some(Value::Number(n)) => Some(Key::Number(n)),
        Some(Value::Str(s)) => Some(Key::Str(s)),
        _ => None,
    })
}

/// Writes an element for the plugin (`InternalElemToPluginElem`): a form as its address, a
/// string as a guest copy it owns.
fn write_element(e: &mut Engine, at: u32, data: &Data) -> Result<(), Stop> {
    let (word, kind): (u64, u8) = match data {
        Data::Number(n) => (n.to_bits(), T_NUMBER),
        Data::Form(id) => (u64::from(form_address(e, *id)), T_FORM),
        Data::Str(s) => (u64::from(guest_text(e, s)), T_STRING),
        Data::Array(id) => (u64::from(*id), T_ARRAY),
        Data::Invalid => (0, 0),
    };
    let mut b = [0u8; ELEMENT as usize];
    b[..8].copy_from_slice(&word.to_le_bytes());
    b[8] = kind;
    wr(e, at, &b)
}

fn form_address(e: &mut Engine, id: u32) -> u32 {
    if id == 0 {
        0
    } else {
        e.call(LOOKUP_FORM, &[id]).eax
    }
}

fn key_data(k: &Key) -> Data {
    match k {
        Key::Number(n) => Data::Number(*n),
        Key::Str(s) => Data::Str(s.clone()),
    }
}

/// `SetElementFromAPI`: the value of a plugin element at `key`; one of no type makes the element
/// and sets nothing.
fn set_from(e: &mut Engine, id: u32, key: &Key, value: Option<Value>) -> bool {
    match value {
        Some(v) => vars::set_element(e, id, key, v),
        None => vars::make_element(e, id, key),
    }
}

fn create(e: &mut Engine, key_type: u8, packed: bool, script: u32) -> u32 {
    let modi = vars::script_mod_index(e, script);
    vars::create_array(e, key_type, packed, modi)
}

/// `CreateArray(data, size, script)`: a packed array of the elements.
fn arr_create(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let id = create(e, vars::KEY_NUMBER, true, a[2]);
    for i in 0..count(a[1], a[0])? {
        let v = read_element(e, nth(a[0], ELEMENT, i)?)?;
        set_from(e, id, &Key::Number(f64::from(i)), v);
    }
    Ok(id)
}
fn arr_create_string_map(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let id = create(e, vars::KEY_STRING, false, a[3]);
    for i in 0..count(a[2], a[0])? {
        let key_at = get32(e, nth(a[0], 4, i)?)?;
        let key = text(e, key_at)?;
        let v = read_element(e, nth(a[1], ELEMENT, i)?)?;
        set_from(e, id, &Key::Str(key), v);
    }
    Ok(id)
}
fn arr_create_map(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let id = create(e, vars::KEY_NUMBER, false, a[3]);
    for i in 0..count(a[2], a[0])? {
        let k = rd(e, nth(a[0], 8, i)?, 8)?;
        let key = f64::from_le_bytes([k[0], k[1], k[2], k[3], k[4], k[5], k[6], k[7]]);
        let v = read_element(e, nth(a[1], ELEMENT, i)?)?;
        set_from(e, id, &Key::Number(key), v);
    }
    Ok(id)
}
/// `AssignCommandResult(arr, dest)`: the command's result is the array's id.
fn arr_assign_result(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    if !vars::array_exists(e, a[0]) {
        let who = current(e).ok().map(|p| state(e).plugins[p].file.clone());
        report(
            e,
            format!(
                "{}: returns array {} that does not exist",
                who.unwrap_or_default(),
                a[0]
            ),
        );
        return Ok(0);
    }
    wr(e, a[1], &f64::from(a[0]).to_le_bytes())?;
    Ok(1)
}
fn arr_set_element(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    if vars::array(e, a[0]).is_none() {
        return Ok(0);
    }
    if let Some(key) = read_key(e, a[1])? {
        let v = read_element(e, a[2])?;
        set_from(e, a[0], &key, v);
    }
    Ok(0)
}
/// `AppendElement`: at the end of a packed array; nothing for a map.
fn arr_append(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let at = match vars::array(e, a[0]) {
        Some(v) if v.packed && v.key_type == vars::KEY_NUMBER => v.len(),
        _ => return Ok(0),
    };
    let v = read_element(e, a[1])?;
    set_from(e, a[0], &Key::Number(at as f64), v);
    Ok(0)
}
fn arr_size(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    Ok(vars::array(e, a[0]).map_or(0, |v| v.len() as u32))
}
fn arr_lookup(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    Ok(if vars::array_exists(e, a[0]) { a[0] } else { 0 })
}
fn arr_get_element(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let Some(key_type) = vars::array(e, a[0]).map(|v| v.key_type) else {
        return Ok(0);
    };
    let Some(key) = read_key(e, a[1])? else {
        return Ok(0);
    };
    if key.type_code() != key_type {
        return Ok(0);
    }
    match vars::get_element(e, a[0], &key) {
        Some(el) => {
            write_element(e, a[2], &el.data)?;
            Ok(1)
        }
        None => Ok(0),
    }
}
/// `GetElements(arr, elements, keys)`: every element in order, and its key when `keys` is given.
fn arr_get_elements(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    if a[1] == 0 {
        return Ok(0);
    }
    let Some(keys) = vars::array(e, a[0]).map(|v| v.keys()) else {
        return Ok(0);
    };
    for (i, key) in keys.iter().enumerate() {
        let i = i as u32;
        if a[2] != 0 {
            write_element(e, nth(a[2], ELEMENT, i)?, &key_data(key))?;
        }
        let data = vars::get_element(e, a[0], key)
            .map(|el| el.data)
            .unwrap_or_default();
        write_element(e, nth(a[1], ELEMENT, i)?, &data)?;
    }
    Ok(1)
}
fn arr_packed(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    Ok(vars::array(e, a[0]).map_or(0, |v| u32::from(v.packed)))
}
/// `GetContainerType`: 0 array, 1 map, 2 string map, -1 none.
fn arr_container_type(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    Ok(match vars::array(e, a[0]) {
        None => u32::MAX,
        Some(v) if v.key_type == vars::KEY_STRING => 2,
        Some(v) if v.packed => 0,
        Some(_) => 1,
    })
}
fn arr_has_key(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    if vars::array(e, a[0]).is_none() {
        return Ok(0);
    }
    let Some(key) = read_key(e, a[1])? else {
        return Ok(0);
    };
    Ok(u32::from(vars::get_element(e, a[0], &key).is_some()))
}

// ---- script ----------------------------------------------------------------------------------

/// A user function's result into a plugin's element (`BasicTokenToPluginElem`): false for none
/// or a type an element cannot hold.
fn result_element(e: &mut Engine, at: u32, tok: Option<nvse::expr::Tok>) -> Result<bool, Stop> {
    use nvse::expr::Tok;
    let data = match tok {
        Some(Tok::Number { v, .. }) => Data::Number(v),
        Some(Tok::Bool(b)) => Data::Number(f64::from(u8::from(b))),
        Some(Tok::Form(id)) => Data::Form(id),
        Some(Tok::Str(s)) => Data::Str(s),
        Some(Tok::Array(id)) => Data::Array(id),
        _ => {
            write_element(e, at, &Data::Invalid)?;
            return Ok(false);
        }
    };
    write_element(e, at, &data)?;
    Ok(true)
}

/// `CallFunction(script, callingObj, container, result, numArgs, ...)`.
fn script_call(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let n = a[4] & 0xff;
    if n > VARIADIC {
        return Ok(0);
    }
    let args = a[5..5 + n as usize].to_vec();
    let caller = nvse::functions::Caller::internal(a[0], a[1], a[2], args);
    let tok = nvse::functions::call(e, caller);
    if a[3] == 0 {
        return Ok(1);
    }
    Ok(u32::from(result_element(e, a[3], tok)?))
}
/// `CallFunctionAlt(script, callingObj, numArgs, ...)`: no result asked for.
fn script_call_alt(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let n = a[2] & 0xff;
    if n > VARIADIC {
        return Ok(0);
    }
    let args = a[3..3 + n as usize].to_vec();
    let caller = nvse::functions::Caller::internal(a[0], a[1], 0, args);
    nvse::functions::call(e, caller);
    Ok(1)
}
/// `GetFunctionParams(script, typesOut)`: the parameter count (-1: not a function script).
fn script_params(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    match nvse::functions::param_types(e, a[0]) {
        None => Ok(u32::MAX),
        Some(types) => {
            if a[1] != 0 {
                wr(e, a[1], &types)?;
            }
            Ok(types.len() as u32)
        }
    }
}
/// `ExtractArgsEx(paramInfo, scriptData, offsetPtr, script, eventList, ...)`: each argument read
/// stored through the next pointer, as the game's `ExtractArgs` stores it.
fn script_extract_args(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let read = nvse::args::v_extract_args_ex(e, a[0], a[1], a[2], a[3], a[4], false);
    for (i, arg) in read.args.iter().enumerate() {
        let Some(&out) = a.get(5 + i) else {
            break;
        };
        store_arg(e, out, arg)?;
    }
    Ok(u32::from(read.ok))
}

fn store_arg(e: &mut Engine, out: u32, arg: &nvse::args::Arg) -> Result<(), Stop> {
    use nvse::args::Arg;
    match arg {
        Arg::Str { text, .. } => {
            let mut b = text.clone();
            b.push(0);
            wr(e, out, &b)
        }
        Arg::Int(v) => wr(e, out, &v.to_le_bytes()),
        Arg::Short(v) | Arg::Form(v) | Arg::Var(v) => wr(e, out, &v.to_le_bytes()),
        Arg::Byte(v) => wr(e, out, &[*v]),
        Arg::Float(v) => wr(e, out, &v.to_le_bytes()),
        Arg::Double(v) => wr(e, out, &v.to_le_bytes()),
    }
}

fn script_compile(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let source = text(e, a[0])?;
    Ok(nvse::compile_script::compile_script(e, &source))
}
fn script_compile_expression(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let source = text(e, a[0])?;
    Ok(nvse::compile_script::compile_expression(e, &source))
}

// ---- data ------------------------------------------------------------------------------------

/// `GetSingleton(id)`: xNVSE's C++ objects (the input hook, the array and string maps) have no
/// guest form here; 0, and a report line naming the one asked for.
fn data_singleton(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let who = current(e)?;
    let file = state(e).plugins[who].file.clone();
    report(
        e,
        format!(
            "{file}: NVSEDataInterface.GetSingleton({}) is not provided",
            a[0]
        ),
    );
    Ok(0)
}

fn lambda_save(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    nvse::lambda::save_variables(e, a[0]);
    Ok(0)
}
fn lambda_unsave(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    nvse::lambda::unsave_variables(e, a[0]);
    Ok(0)
}
fn is_lambda(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    Ok(u32::from(nvse::lambda::is_lambda(e, a[0])))
}

/// `GetFunc(id)`: the lambda functions (8, 9, 10); the others are not given (0, reported).
fn data_func(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let f: Option<(&str, HostFn)> = match a[0] {
        8 => Some(("LambdaSaveVariableList", lambda_save)),
        9 => Some(("LambdaUnsaveVariableList", lambda_unsave)),
        10 => Some(("IsScriptLambda", is_lambda)),
        _ => None,
    };
    match f {
        Some((name, f)) => Ok(thunk(
            e,
            &format!("NVSEDataInterface.{name}"),
            Some(f),
            1,
            false,
        )),
        None => {
            let who = current(e)?;
            let file = state(e).plugins[who].file.clone();
            report(
                e,
                format!(
                    "{file}: NVSEDataInterface.GetFunc({}) is not provided",
                    a[0]
                ),
            );
            Ok(0)
        }
    }
}
/// `GetData(kNVSEData_NumPreloadMods)`: a word holding 0 (no mod is preloaded here).
fn data_data(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    if a[0] != 1 {
        return Ok(0);
    }
    if ifaces(e).preload_word == 0 {
        let w = e.mem.alloc(4);
        e.mem.set_u32(w, 0);
        ifaces(e).preload_word = w;
    }
    Ok(ifaces(e).preload_word)
}
/// `ClearScriptDataCache`: the user functions' headers forgotten and the message sent.
fn data_clear_cache(e: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    nvse::functions::clear_infos(e);
    nvse::plugins::dispatch(e, nvse::plugins::msg::CLEAR_SCRIPT_DATA_CACHE, 0, 0);
    Ok(0)
}

// ---- events and logging ----------------------------------------------------------------------

/// `DispatchEvent(name, thisObj, ...)`: the event's own number of arguments is read.
fn event_dispatch(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let name = text(e, a[0])?;
    Ok(u32::from(nvse::events::dispatch(e, &name, a[1], &a[2..])))
}

/// `GetPluginLogPath()`: empty, the game folder (xNVSE's default with no `sPluginLogPath`).
fn log_path(e: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    if ifaces(e).log_path == 0 {
        let at = guest_text(e, b"");
        ifaces(e).log_path = at;
    }
    Ok(ifaces(e).log_path)
}

#[cfg(test)]
#[path = "interfaces_tests.rs"]
mod tests;
