// SPDX-License-Identifier: GPL-3.0-only
//! Script-extender plugin DLLs, loaded (ADR-0018). A plugin's 32-bit x86 code runs in the
//! interpreter of [`x86`] over the guest memory; its imports and the `NVSEInterface` it is
//! handed are host functions behind thunk addresses. A command it registers is a record in
//! xNVSE's table whose execute word is an address in the plugin's image; the engine's call of a
//! handler by address ([`Engine::call`]) lands in a double registered at that address, which
//! runs the interpreter.
//!
//! **Off by default** ([`set_enabled`], `NV_PLUGIN_DLLS=1`): with it off [`load`] does nothing and
//! the built-in extenders answer as before.
//!
//! **A plugin that fails never stops the engine.** Every call into a plugin is guarded: an
//! unmapped access, an instruction the interpreter lacks, an import or game address the host does
//! not provide, or a call that runs too long stops that call, marks the plugin failed, takes its
//! commands out of the table and writes one report line saying which plugin wanted what
//! ([`reports`]).
//!
//! What this first step has: the PE mapper ([`pe`]), the integer core ([`x86`]), the import
//! binding with a small set of kernel32 functions, the `NVSEInterface` (`RegisterCommand`,
//! `SetOpcodeBase`, `QueryInterface`, `GetPluginHandle`, `RegisterTypedCommand`,
//! `GetRuntimeDirectory`), the messaging interface and the others `QueryInterface` hands out
//! ([`interfaces`]: serialization, console, command table, string and array variables, script,
//! data, event manager, logging), plugin commands in the table, and the plugin's place in xNVSE's
//! plugin list. What is not built yet is listed in ADR-0018 and in the
//! report of [`x86`].

pub mod patches;
pub mod pe;
pub mod verdicts;
pub mod x86;

mod crt;
mod interfaces;
mod printf;
pub mod thread;
mod winapi;

use engine_core::prelude::*;
use nvse::plugins::{self, BuiltIn, Listener, Message};
use nvse::table::{self, PluginRecord, Returns};
use std::collections::HashMap;
use x86::{Bridge, Cpu, Stop, EAX, EDX, ESP};

/// Thunk addresses: 16 bytes a thunk, never mapped (the interpreter asks the host at an address
/// before it fetches), in the extender range's unused middle (`EXTENDER_CODE` is
/// `0x7E800000..0x7F000000`; handlers use `+0` to `+0xCCCC` of `0x7E800000` and `0x7EC00000`, named
/// routines start at `0x7EFF0000`).
pub const THUNK_BASE: u32 = 0x7ED0_0000;
pub const THUNK_END: u32 = 0x7EE0_0000;
/// The address a call from the host returns to: the first thunk, which stops the run.
const RETURN_MAGIC: u32 = THUNK_BASE;
/// The most instructions one call into a plugin may run.
pub const CALL_BUDGET: u64 = 200_000_000;
/// xNVSE 6.4.9's packed version and the runtime's (1.4.0.525), as ADR-0016 answers.
const NVSE_VERSION: u32 = 0x0604_0090;
const RUNTIME_VERSION: u32 = 0x0400_20D0;
/// The most plugin DLLs one run loads.
pub const MAX_PLUGINS: usize = 64;

/// Whether the loader is on for this process (`NV_PLUGIN_DLLS`, any value but `0` or empty).
pub fn wanted(var: Option<&str>) -> bool {
    matches!(var, Some(v) if !v.is_empty() && v != "0")
}

type HostFn = fn(&mut Engine, &[u32]) -> Result<u32, Stop>;

#[derive(Clone)]
struct Thunk {
    name: String,
    f: Option<HostFn>,
    argc: u32,
    stdcall: bool,
    /// Words a variadic function may get past its `argc`: read while they are mapped, 0 past
    /// the stack's end.
    extra: u32,
}

/// A plugin as loaded.
#[derive(Clone, Debug, Default)]
pub struct Plugin {
    /// The DLL's file name.
    pub file: String,
    /// `PluginInfo::name`.
    pub name: String,
    pub version: u32,
    pub base: u32,
    pub opcode_base: u32,
    next_opcode: u32,
    /// (opcode, name) of each command it registered.
    pub commands: Vec<(u32, String)>,
    /// Listeners it registered: (sender name, guest address of the callback).
    pub listeners: Vec<(String, u32)>,
    /// Imports the host does not provide ("dll!name"); a call to one stops that call.
    pub unresolved: Vec<String>,
    /// Why it stopped working, once it has.
    pub failed: Option<String>,
    /// A runtime DLL the user supplied (msvcp140), not a plugin.
    pub runtime: bool,
    /// The guest addresses of its `NVSEPlugin_Query` and `NVSEPlugin_Load`.
    pub query_at: u32,
    pub load_at: u32,
    /// `NVSEPlugin_Query` answered yes / `NVSEPlugin_Load` answered yes.
    pub queried: bool,
    pub loaded: bool,
    /// The co-save callbacks it set through the serialization interface.
    pub callbacks: interfaces::Callbacks,
}

#[derive(Default)]
pub struct State {
    on: bool,
    thunks: Vec<Thunk>,
    by_name: HashMap<String, u32>,
    stack_top: u32,
    sp: u32,
    budget: u64,
    nvse: u32,
    msg: u32,
    pub plugins: Vec<Plugin>,
    /// The plugin whose code runs (the innermost).
    running: Vec<usize>,
    /// Host-side counters the tests read.
    pub reports: Vec<String>,
    pub(crate) rt: crt::Rt,
    /// Whether a plugin's entry point runs (`DllMain`) before its `NVSEPlugin_Query`.
    entry_points: bool,
    /// Writes into the game's code, kept ([`patches`]).
    shadow: patches::Shared,
    /// Exports of the runtime DLLs the user supplied: (DLL file name lower case, name) -> address.
    runtime_exports: HashMap<(String, String), u32>,
    /// The thread block `fs` points at ([`thread`]).
    pub(crate) thread: thread::Thread,
    /// The interfaces other than messaging ([`interfaces`]).
    ifaces: interfaces::Ifaces,
}

fn state(e: &mut Engine) -> &mut State {
    e.ext::<State>()
}

pub fn set_enabled(e: &mut Engine, on: bool) {
    state(e).on = on;
}

/// Sets the most instructions one call into a plugin may run (0: the default, [`CALL_BUDGET`]).
pub fn set_budget(e: &mut Engine, n: u64) {
    state(e).budget = n;
}

/// Runs each plugin's entry point (`DllMain`, which makes its C++ globals) before its query.
/// Off until the interpreter has the instructions the C runtime's start-up code uses.
pub fn set_entry_points(e: &mut Engine, on: bool) {
    state(e).entry_points = on;
}

pub fn enabled(e: &mut Engine) -> bool {
    state(e).on
}

/// The report lines so far: one for each plugin that failed or wanted something missing.
pub fn reports(e: &mut Engine) -> Vec<String> {
    state(e).reports.clone()
}

/// Supplies the executable's code: the sections of an unpacked FalloutNV.exe (1.4.0.525), which
/// a plugin that checks the bytes it is about to patch then reads as the game has them. The
/// Steam executable's code is encrypted and is refused (the unpacked file is the user's own, a
/// GOG executable or one unpacked on their machine; nothing of it is kept or shipped). Without
/// it the code reads as zeros and such a plugin refuses to patch. Returns the bytes supplied.
pub fn supply_code_from_exe(e: &mut Engine, bytes: &[u8]) -> Result<usize, String> {
    let image = pe::Pe::parse(bytes).map_err(|x| x.to_string())?;
    if image.image_base != 0x0040_0000 {
        return Err(format!(
            "not the game's executable (image base {:#x})",
            image.image_base
        ));
    }
    if image.sections.iter().any(|s| s.name == ".bind") {
        return Err("a packed executable (its code is encrypted): unpack it first".into());
    }
    let mut total = 0;
    for s in image.sections.iter().filter(|s| s.name == ".text") {
        let from = s.raw as usize;
        let len = s.raw_size.min(s.vsize) as usize;
        let data = bytes
            .get(from..from + len)
            .ok_or("the code section is cut short")?;
        state(e)
            .shadow
            .borrow_mut()
            .image
            .push((0x0040_0000 + s.va, data.to_vec()));
        total += len;
    }
    if total == 0 {
        return Err("no .text section".into());
    }
    Ok(total)
}

/// The patch sites of every plugin so far, as it wrote them (merged where one write goes on
/// where the last ended), each with whether it is the entry of a function the engine has.
pub fn patch_sites(e: &mut Engine) -> Vec<patches::Site> {
    let writes = state(e).shadow.borrow().writes.clone();
    writes
        .into_iter()
        .map(|w| patches::Site {
            plugin: state(e)
                .plugins
                .get(w.plugin)
                .map_or_else(String::new, |p| p.file.clone()),
            function_entry: e.is_translated(w.addr),
            addr: w.addr,
            bytes: w.bytes,
        })
        .collect()
}

/// How many distinct bytes of the game's code each plugin read (the file name, the count): the
/// bytes are not available, so a plugin that checks them before it patches sees zeros.
pub fn code_reads(e: &mut Engine) -> Vec<(String, usize)> {
    let reads = state(e).shadow.borrow().reads.clone();
    let mut by: std::collections::BTreeMap<usize, usize> = Default::default();
    for (p, _) in reads {
        *by.entry(p).or_default() += 1;
    }
    by.into_iter()
        .map(|(p, n)| {
            (
                state(e)
                    .plugins
                    .get(p)
                    .map_or_else(String::new, |p| p.file.clone()),
                n,
            )
        })
        .collect()
}

pub fn plugins_loaded(e: &mut Engine) -> Vec<Plugin> {
    state(e).plugins.clone()
}

fn report(e: &mut Engine, line: String) {
    eprintln!("plugin-host: {line}");
    state(e).reports.push(line);
}

// ---------------------------------------------------------------------------------------------
// Thunks
// ---------------------------------------------------------------------------------------------

fn thunk(e: &mut Engine, key: &str, f: Option<HostFn>, argc: u32, stdcall: bool) -> u32 {
    let st = state(e);
    if let Some(&a) = st.by_name.get(key) {
        return a;
    }
    if st.thunks.is_empty() {
        st.thunks.push(Thunk {
            name: "return".into(),
            f: None,
            argc: 0,
            stdcall: false,
            extra: 0,
        });
    }
    let addr = THUNK_BASE + 16 * st.thunks.len() as u32;
    assert!(addr < THUNK_END, "thunk range full");
    st.thunks.push(Thunk {
        name: key.to_string(),
        f,
        argc,
        stdcall,
        extra: 0,
    });
    st.by_name.insert(key.to_string(), addr);
    addr
}

/// Makes the thunk at `addr` read up to `extra` more argument words (a variadic function).
fn set_variadic(e: &mut Engine, addr: u32, extra: u32) {
    let idx = ((addr - THUNK_BASE) / 16) as usize;
    if let Some(t) = state(e).thunks.get_mut(idx) {
        t.extra = extra;
    }
}

struct Host;

impl Bridge for Host {
    fn at(&mut self, e: &mut Engine, cpu: &mut Cpu, eip: u32) -> Option<Result<(), Stop>> {
        if (0x0040_0000..0x0140_9000).contains(&eip) {
            return Some(Err(Stop::Missing(format!(
                "the game's function {eip:#010x}"
            ))));
        }
        if !(THUNK_BASE..THUNK_END).contains(&eip) {
            return None;
        }
        let idx = ((eip - THUNK_BASE) / 16) as usize;
        let t = state(e).thunks.get(idx)?.clone();
        let Some(f) = t.f else {
            return Some(Err(Stop::Missing(t.name)));
        };
        let sp = cpu.r[ESP];
        let mut args = Vec::with_capacity((t.argc + t.extra) as usize);
        for i in 0..t.argc + t.extra {
            match e.mem.try_read(sp.wrapping_add(4 + 4 * i), &mut [0u8; 4]) {
                Ok(()) => args.push(e.mem.u32(sp.wrapping_add(4 + 4 * i))),
                // A variadic function's words past the fixed ones: what the caller did not
                // push may lie past the stack's end.
                Err(_) if i >= t.argc => args.push(0),
                Err(_) => {
                    return Some(Err(Stop::Fault {
                        addr: sp.wrapping_add(4 + 4 * i),
                        write: false,
                    }))
                }
            }
        }
        let ret = match e.mem.try_read(sp, &mut [0u8; 4]) {
            Ok(()) => e.mem.u32(sp),
            Err(_) => {
                return Some(Err(Stop::Fault {
                    addr: sp,
                    write: false,
                }))
            }
        };
        match f(e, &args) {
            Ok(v) => {
                cpu.r[EAX] = v;
                if let Some(hi) = state(e).rt.hi.take() {
                    cpu.r[EDX] = hi;
                }
                cpu.eip = ret;
                cpu.r[ESP] =
                    sp.wrapping_add(4)
                        .wrapping_add(if t.stdcall { 4 * t.argc } else { 0 });
                Some(Ok(()))
            }
            Err(Stop::Fault { addr, write }) => Some(Err(Stop::HostFault {
                func: t.name,
                addr,
                write,
            })),
            Err(stop) => Some(Err(stop)),
        }
    }
}

/// Runs the plugin function at `addr` with cdecl `args`, returning `eax`. The caller says which
/// plugin owns the code (for the report); nested calls (a host function that calls a plugin
/// again) go on the same stack below the outer frame.
pub(crate) fn call_guest(
    e: &mut Engine,
    plugin: usize,
    addr: u32,
    args: &[u32],
) -> Result<u32, Stop> {
    let teb = thread::thread_block(e);
    // The SEH chain as the call found it: a frame the plugin pushed and never popped (it stopped
    // between the two) is not left in the chain.
    let chain = e.mem.u32(teb + thread::EXCEPTION_LIST);
    let saved = state(e).sp;
    let mut cpu = Cpu {
        r: [0; 8],
        fs_base: teb,
        ..Cpu::default()
    };
    cpu.r[ESP] = saved;
    for &a in args.iter().rev() {
        cpu.push(e, a)?;
    }
    cpu.push(e, RETURN_MAGIC)?;
    cpu.eip = addr;
    let st = state(e);
    cpu.shadow = Some(st.shadow.clone());
    let prior = std::mem::replace(&mut st.shadow.borrow_mut().who, plugin);
    st.sp = cpu.r[ESP].wrapping_sub(256);
    st.running.push(plugin);
    let budget = match state(e).budget {
        0 => CALL_BUDGET,
        n => n,
    };
    let r = cpu.run(e, &mut Host, RETURN_MAGIC, budget);
    let st = state(e);
    st.shadow.borrow_mut().who = prior;
    st.running.pop();
    st.sp = saved;
    e.mem.set_u32(teb + thread::EXCEPTION_LIST, chain);
    r.map(|()| cpu.r[EAX])
}

/// The guest stack all plugin calls share, made on first use: (its top, its bottom).
pub(crate) fn guest_stack(e: &mut Engine) -> (u32, u32) {
    const SIZE: u32 = 1 << 20;
    if state(e).stack_top == 0 {
        let base = e.mem.alloc(SIZE);
        let st = state(e);
        st.stack_top = base + SIZE - 64;
        st.sp = st.stack_top;
    }
    let top = state(e).stack_top + 64;
    (top, top - SIZE)
}

/// Marks a plugin failed, takes its commands out of the table (a script that calls one then
/// fails at that line, as without the DLL) and writes the report line.
fn fail(e: &mut Engine, plugin: usize, doing: &str, stop: &Stop) {
    let (name, commands) = {
        let p = &mut state(e).plugins[plugin];
        let line = format!("{doing}: {stop}");
        if p.failed.is_none() {
            p.failed = Some(line);
        }
        (p.file.clone(), p.commands.clone())
    };
    for (opcode, _) in commands {
        let record = table::record_of(e, opcode);
        if record != 0 {
            e.mem.set_u32(record + 0x18, 0);
            e.mem.set_u32(record + 0x20, 0);
        }
    }
    report(e, format!("{name}: {doing}: {stop}"));
}

fn guarded(e: &mut Engine, plugin: usize, doing: &str, addr: u32, args: &[u32]) -> Option<u32> {
    if state(e).plugins[plugin].failed.is_some() {
        return None;
    }
    match call_guest(e, plugin, addr, args) {
        Ok(v) => Some(v),
        Err(stop) => {
            fail(e, plugin, doing, &stop);
            None
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Imports the host provides
// ---------------------------------------------------------------------------------------------

fn k32_tick(_: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u32))
}
fn k32_zero(_: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    Ok(0)
}
fn k32_one(_: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    Ok(1)
}
fn k32_pid(_: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    Ok(1)
}
fn k32_pseudo_handle(_: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    Ok(u32::MAX)
}
fn k32_virtual_protect(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    // `VirtualProtect(addr, size, new, *old)`: the model has no page rights to change.
    if a[3] != 0 {
        let wrote = e.mem.try_write(a[3], &0x40u32.to_le_bytes()).is_ok();
        // A plugin that hands the game's own code as the place for the old rights is not
        // patching it: nothing is kept.
        if !wrote && !patches::in_image(a[3], 4) {
            return Err(Stop::Fault {
                addr: a[3],
                write: true,
            });
        }
    }
    Ok(1)
}
fn k32_qpc(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    e.mem
        .try_write(a[0], &t.to_le_bytes())
        .map_err(|_| Stop::Fault {
            addr: a[0],
            write: true,
        })?;
    Ok(1)
}
fn k32_lstrlen(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    Ok(e.mem.cstr(a[0]).len() as u32)
}
fn k32_debug_string(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let s = String::from_utf8_lossy(&e.mem.cstr(a[0])).into_owned();
    let who = state(e).running.last().copied();
    let name = who.map_or(String::new(), |i| state(e).plugins[i].file.clone());
    report(e, format!("{name}: OutputDebugString: {s}"));
    Ok(0)
}
fn k32_terminate(_: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    Err(Stop::Exit)
}

/// (dll, name, function, argument words, stdcall). `argc` words are read for each call.
const SHIMS: &[(&str, &str, HostFn, u32, bool)] = &[
    ("kernel32.dll", "GetTickCount", k32_tick, 0, true),
    ("kernel32.dll", "GetCurrentProcessId", k32_pid, 0, true),
    ("kernel32.dll", "GetCurrentThreadId", k32_pid, 0, true),
    (
        "kernel32.dll",
        "GetCurrentProcess",
        k32_pseudo_handle,
        0,
        true,
    ),
    (
        "kernel32.dll",
        "VirtualProtect",
        k32_virtual_protect,
        4,
        true,
    ),
    ("kernel32.dll", "FlushInstructionCache", k32_one, 3, true),
    (
        "kernel32.dll",
        "IsProcessorFeaturePresent",
        k32_zero,
        1,
        true,
    ),
    (
        "kernel32.dll",
        "DisableThreadLibraryCalls",
        k32_one,
        1,
        true,
    ),
    ("kernel32.dll", "IsDebuggerPresent", k32_zero, 0, true),
    ("kernel32.dll", "QueryPerformanceCounter", k32_qpc, 1, true),
    ("kernel32.dll", "lstrlenA", k32_lstrlen, 1, true),
    (
        "kernel32.dll",
        "OutputDebugStringA",
        k32_debug_string,
        1,
        true,
    ),
    ("kernel32.dll", "TerminateProcess", k32_terminate, 2, true),
];

fn bind_import(e: &mut Engine, dll: &str, name: &str) -> (u32, bool) {
    let key = format!("{dll}!{name}");
    if let Some(&addr) = state(e)
        .runtime_exports
        .get(&(dll.to_ascii_lowercase(), name.to_string()))
    {
        return (addr, true);
    }
    match SHIMS.iter().find(|s| s.0 == dll && s.1 == name) {
        Some(&(_, _, f, argc, stdcall)) => (thunk(e, &key, Some(f), argc, stdcall), true),
        None => {
            if let Some((f, argc)) = winapi::lookup(dll, name) {
                return (thunk(e, &key, Some(f), argc, true), true);
            }
            if let Some((f, argc)) = crt::lookup(dll, name) {
                return (thunk(e, &key, Some(f), argc, false), true);
            }
            (thunk(e, &key, None, 0, true), false)
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The NVSEInterface and the messaging interface
// ---------------------------------------------------------------------------------------------

fn current(e: &mut Engine) -> Result<usize, Stop> {
    state(e)
        .running
        .last()
        .copied()
        .ok_or_else(|| Stop::Missing("an interface call outside a plugin".into()))
}

fn nvse_set_opcode_base(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let p = current(e)?;
    let plugin = &mut state(e).plugins[p];
    plugin.opcode_base = a[0];
    plugin.next_opcode = a[0];
    Ok(0)
}

fn register(e: &mut Engine, info: u32, returns: Returns) -> Result<u32, Stop> {
    let p = current(e)?;
    let read = |e: &Engine, off: u32| e.mem.u32(info + off);
    for off in 0..0x28 {
        if e.mem.try_read(info + off, &mut [0u8; 1]).is_err() {
            return Err(Stop::Fault {
                addr: info + off,
                write: false,
            });
        }
    }
    let name = String::from_utf8_lossy(&e.mem.cstr(read(e, 0))).into_owned();
    let short = String::from_utf8_lossy(&e.mem.cstr(read(e, 4))).into_owned();
    let (needs_ref, param_count, params) = (
        e.mem.u16(info + 0x10) != 0,
        e.mem.u16(info + 0x12),
        read(e, 0x14),
    );
    let (execute, eval, flags) = (read(e, 0x18), read(e, 0x20), read(e, 0x24));
    let plugin = &mut state(e).plugins[p];
    if plugin.opcode_base == 0 {
        // A plugin that never called `SetOpcodeBase` has no opcodes (xNVSE refuses it).
        return Ok(0);
    }
    let opcode = plugin.next_opcode;
    if opcode >= table::ROOM_END {
        return Ok(0);
    }
    plugin.next_opcode += 1;
    plugin.commands.push((opcode, name.clone()));
    e.mem.set_u32(info + 8, opcode);
    table::grow_to(e, opcode + 1);
    for handler in [execute, eval] {
        if handler != 0 {
            register_handler(e, p, handler);
        }
    }
    let record = PluginRecord {
        name: &name,
        short: &short,
        needs_ref,
        param_count,
        params,
        execute,
        eval,
        flags,
        expression: false,
        returns,
    };
    table::add_plugin_record(e, opcode, &record);
    Ok(1)
}

/// A plugin's handler at a guest address is called by the engine through [`Engine::call`]: a
/// double at that address runs the interpreter.
fn register_handler(e: &mut Engine, plugin: usize, addr: u32) {
    e.register_double(addr, move |e, args| {
        let r = guarded(e, plugin, "command", addr, args).unwrap_or(0);
        Ret {
            eax: r,
            ..Ret::default()
        }
    });
}

fn nvse_register_command(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    register(e, a[0], Returns::Default)
}

fn nvse_register_typed(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let returns = match a[1] {
        1 => Returns::Form,
        2 => Returns::String,
        3 => Returns::Array,
        4 => Returns::ArrayIndex,
        5 => Returns::Ambiguous,
        _ => Returns::Default,
    };
    register(e, a[0], returns)
}

fn nvse_handle(e: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    current(e).map(|p| p as u32)
}

fn nvse_runtime_dir(e: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    Ok(table::guest_str(e, b".\\"))
}

fn nvse_query_interface(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    match a[0] {
        2 => Ok(state(e).msg),
        id if interfaces::query(e, id) != 0 => Ok(interfaces::query(e, id)),
        id => {
            let p = current(e)?;
            let file = state(e).plugins[p].file.clone();
            report(e, format!("{file}: QueryInterface({id}) is not provided"));
            Ok(0)
        }
    }
}

fn nvse_expr_utils(_: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    Ok(0)
}

fn msg_register_listener(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let (handle, sender, handler) = (a[0] as usize, a[1], a[2]);
    let sender = String::from_utf8_lossy(&e.mem.cstr(sender)).into_owned();
    let plugins = &mut state(e).plugins;
    let Some(p) = plugins.get_mut(handle) else {
        return Ok(0);
    };
    p.listeners.push((sender, handler));
    register_handler(e, handle, handler);
    Ok(1)
}

fn msg_dispatch(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    // `Dispatch(sender, type, data, len, receiver)`: to the listeners that named the sender.
    let (sender, kind, data, len) = (a[0] as usize, a[1], a[2], a[3]);
    let Some(name) = state(e).plugins.get(sender).map(|p| p.name.clone()) else {
        return Ok(0);
    };
    deliver(e, Some(&name), kind, data, len);
    Ok(1)
}

/// Sends a message to the plugins' listeners: those that registered for `from` (`None`: for
/// "NVSE", xNVSE's own messages).
fn deliver(e: &mut Engine, from: Option<&str>, kind: u32, data: u32, len: u32) {
    let want = from.unwrap_or("NVSE");
    let n = state(e).plugins.len();
    for i in 0..n {
        let callbacks: Vec<u32> = state(e).plugins[i]
            .listeners
            .iter()
            .filter(|(s, _)| s == want)
            .map(|(_, f)| *f)
            .collect();
        for f in callbacks {
            let sender = table::guest_str(e, want.as_bytes());
            let msg = e.mem.alloc(16);
            e.mem.set_u32(msg, sender);
            e.mem.set_u32(msg + 4, kind);
            e.mem.set_u32(msg + 8, len);
            e.mem.set_u32(msg + 12, data);
            guarded(e, i, "message", f, &[msg]);
            e.mem.free(msg);
        }
    }
}

/// xNVSE's message to listeners, as [`nvse::plugins::dispatch`] sends it to the built-ins: one
/// of these is each plugin's listener in xNVSE's list, in the order of the plugin file names.
fn forward(e: &mut Engine, plugin: usize, m: &Message) {
    // Only this plugin's own listeners: the others get theirs from their own entry.
    let callbacks: Vec<u32> = state(e).plugins[plugin]
        .listeners
        .iter()
        .filter(|(s, _)| s == "NVSE")
        .map(|(_, f)| *f)
        .collect();
    for f in callbacks {
        let sender = table::guest_str(e, b"NVSE");
        let msg = e.mem.alloc(16);
        e.mem.set_u32(msg, sender);
        e.mem.set_u32(msg + 4, m.kind);
        e.mem.set_u32(msg + 8, m.len);
        e.mem.set_u32(msg + 12, m.data);
        guarded(e, plugin, "message", f, &[msg]);
        e.mem.free(msg);
    }
}

fn listen<const N: usize>(e: &mut Engine, m: &Message) {
    forward(e, N, m);
}

macro_rules! listeners {
    ($($n:literal)*) => { [$(listen::<$n> as Listener),*] };
}

const LISTENERS: [Listener; MAX_PLUGINS] = listeners!(
    0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31
    32 33 34 35 36 37 38 39 40 41 42 43 44 45 46 47 48 49 50 51 52 53 54 55 56 57 58 59 60 61 62 63
);

fn build_interfaces(e: &mut Engine) {
    if state(e).nvse != 0 {
        return;
    }
    let slots: [(&str, HostFn, u32); 9] = [
        ("RegisterCommand", nvse_register_command, 1),
        ("SetOpcodeBase", nvse_set_opcode_base, 1),
        ("QueryInterface", nvse_query_interface, 1),
        ("GetPluginHandle", nvse_handle, 0),
        ("RegisterTypedCommand", nvse_register_typed, 2),
        ("GetRuntimeDirectory", nvse_runtime_dir, 0),
        ("InitExpressionEvaluatorUtils", nvse_expr_utils, 1),
        ("RegisterTypedCommandVersion", nvse_register_typed, 3),
        ("(unused)", nvse_expr_utils, 0),
    ];
    let words: Vec<u32> = slots
        .iter()
        .map(|(n, f, argc)| thunk(e, &format!("NVSEInterface.{n}"), Some(*f), *argc, false))
        .collect();
    let nvse = e.mem.alloc(64);
    e.mem.set_u32(nvse, NVSE_VERSION);
    e.mem.set_u32(nvse + 4, RUNTIME_VERSION);
    e.mem.set_u32(nvse + 8, 0);
    e.mem.set_u32(nvse + 12, 0);
    // RegisterCommand, SetOpcodeBase, QueryInterface, GetPluginHandle, RegisterTypedCommand,
    // GetRuntimeDirectory, then isNogore, InitExpressionEvaluatorUtils, RegisterTypedCommandVersion.
    for (i, w) in [words[0], words[1], words[2], words[3], words[4], words[5]]
        .iter()
        .enumerate()
    {
        e.mem.set_u32(nvse + 16 + 4 * i as u32, *w);
    }
    e.mem.set_u32(nvse + 40, 0);
    e.mem.set_u32(nvse + 44, words[6]);
    e.mem.set_u32(nvse + 48, words[7]);
    let msg = e.mem.alloc(16);
    e.mem.set_u32(msg, 4);
    let listen = thunk(
        e,
        "NVSEMessagingInterface.RegisterListener",
        Some(msg_register_listener),
        3,
        false,
    );
    let dispatch = thunk(
        e,
        "NVSEMessagingInterface.Dispatch",
        Some(msg_dispatch),
        5,
        false,
    );
    e.mem.set_u32(msg + 4, listen);
    e.mem.set_u32(msg + 8, dispatch);
    let st = state(e);
    st.nvse = nvse;
    st.msg = msg;
    interfaces::build(e);
}

// ---------------------------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------------------------

/// Why a DLL was not loaded.
#[derive(Debug, PartialEq, Eq)]
pub enum LoadError {
    /// The loader is off (the default).
    Off,
    Pe(String),
    NoQuery,
    TooMany,
    /// `NVSEPlugin_Query` or `NVSEPlugin_Load` said no, or stopped; the report has the line.
    Refused(String),
}

/// The image's implicit thread-local storage, when it has any ([`thread::attach_tls`]).
fn attach_tls(e: &mut Engine, image: &pe::Pe, base: u32) -> Result<(), LoadError> {
    if image.tls == 0 {
        return Ok(());
    }
    thread::attach_tls(e, base, image.size_of_image, image.tls)
        .map_err(|m| LoadError::Pe(format!("TLS: {m}")))
}

/// Maps a runtime DLL the user supplied (`msvcp140.dll`: the C++ library the plugins import),
/// binds its own imports, runs its entry point and makes its exports answer a plugin's imports of
/// that DLL name. Nothing of Microsoft's is shipped: the file is the user's own (or fetched on
/// request), and a test supplies a made-up one. With the loader off nothing happens.
pub fn supply_runtime(e: &mut Engine, file: &str, bytes: &[u8]) -> Result<(), LoadError> {
    if !state(e).on {
        return Err(LoadError::Off);
    }
    let image = pe::Pe::parse(bytes).map_err(|x| LoadError::Pe(x.to_string()))?;
    build_interfaces(e);
    let mut unresolved = Vec::new();
    let mut bound = Vec::new();
    for i in &image.imports {
        bound.push(bind_import(e, &i.dll, &i.name));
    }
    let mut it = bound.iter();
    let base = image
        .map(e, bytes, |i| {
            let (addr, ok) = it.next().expect("one binding for each import");
            if !ok {
                unresolved.push(format!("{}!{}", i.dll, i.name));
            }
            *addr
        })
        .map_err(|x| LoadError::Pe(x.to_string()))?;
    attach_tls(e, &image, base)?;
    let idx = state(e).plugins.len();
    state(e).plugins.push(Plugin {
        file: file.to_string(),
        name: file.to_string(),
        base,
        unresolved: unresolved.clone(),
        runtime: true,
        ..Plugin::default()
    });
    if !unresolved.is_empty() {
        report(
            e,
            format!(
                "{file}: runtime DLL imports the host does not provide: {}",
                unresolved.join(", ")
            ),
        );
    }
    let key = file.to_ascii_lowercase();
    for (name, rva) in &image.exports {
        state(e)
            .runtime_exports
            .insert((key.clone(), name.clone()), base + rva);
    }
    if image.entry != 0 {
        let r = guarded(e, idx, "DllMain", base + image.entry, &[base, 1, 0]);
        if r.is_some_and(|v| v & 0xff == 0) {
            let why = "DllMain answered no".to_string();
            state(e).plugins[idx].failed = Some(why.clone());
            return Err(LoadError::Refused(why));
        }
        if let Some(why) = state(e).plugins[idx].failed.clone() {
            return Err(LoadError::Refused(why));
        }
    }
    Ok(())
}

/// Maps a plugin DLL's bytes, binds its imports, runs `NVSEPlugin_Query` and `NVSEPlugin_Load`
/// and puts it in xNVSE's plugin list. `file` is the DLL's file name (xNVSE's order of plugins
/// is by it). Returns the plugin's index. With the loader off nothing happens ([`LoadError::Off`]).
/// A plugin that asks for another by its file name (`GetModuleHandle`) needs [`load_many`].
pub fn load(e: &mut Engine, file: &str, bytes: &[u8]) -> Result<usize, LoadError> {
    load_many(e, &[(file.to_string(), bytes.to_vec())])
        .pop()
        .unwrap_or(Err(LoadError::Off))
}

/// Loads plugins as xNVSE does: every DLL is mapped first (so a plugin can find another by its
/// file name), then each is queried in the given order, then each is loaded. The answers are in
/// the order of `files`.
pub fn load_many(e: &mut Engine, files: &[(String, Vec<u8>)]) -> Vec<Result<usize, LoadError>> {
    let mut mapped: Vec<Result<usize, LoadError>> = files
        .iter()
        .map(|(file, bytes)| map_one(e, file, bytes))
        .collect();
    for r in &mut mapped {
        if let Ok(idx) = *r {
            if let Err(x) = query_one(e, idx) {
                *r = Err(x);
            }
        }
    }
    let mut queried: Vec<Option<usize>> = mapped.iter().map(|r| r.as_ref().ok().copied()).collect();
    for (r, q) in mapped.iter_mut().zip(queried.iter_mut()) {
        if let Some(idx) = *q {
            if let Err(x) = load_one(e, idx) {
                *r = Err(x);
            }
        }
    }
    // What each wrote into the game's code is reported once, as a count; the sites are in
    // `patch_sites`.
    for p in 0..state(e).plugins.len() {
        report_patches(e, p);
    }
    mapped
}

fn report_patches(e: &mut Engine, idx: usize) {
    let (writes, bytes) = {
        let sh = state(e).shadow.borrow();
        let mine = sh.writes.iter().filter(|w| w.plugin == idx);
        (
            mine.clone().count(),
            mine.map(|w| w.bytes.len()).sum::<usize>(),
        )
    };
    let file = state(e).plugins[idx].file.clone();
    let reads = code_reads(e)
        .into_iter()
        .find(|(f, _)| *f == file)
        .map_or(0, |r| r.1);
    if writes > 0 || reads > 0 {
        report(
            e,
            format!(
                "{file}: patches the game's code: {writes} write(s), {bytes} byte(s), dropped (the code runs as translated Rust); read {reads} byte(s) of it"
            ),
        );
    }
}

/// Phase 1: the file mapped, relocated, its imports bound, its entry point run when asked.
fn map_one(e: &mut Engine, file: &str, bytes: &[u8]) -> Result<usize, LoadError> {
    if !state(e).on {
        return Err(LoadError::Off);
    }
    if state(e).plugins.len() >= MAX_PLUGINS {
        return Err(LoadError::TooMany);
    }
    let image = pe::Pe::parse(bytes).map_err(|x| LoadError::Pe(x.to_string()))?;
    build_interfaces(e);
    let mut unresolved = Vec::new();
    let mut shims = Vec::new();
    for i in &image.imports {
        shims.push((i.clone(), bind_import(e, &i.dll, &i.name)));
    }
    let mut it = shims.iter();
    let base = image
        .map(e, bytes, |_| {
            let (import, (addr, ok)) = it.next().expect("one binding for each import");
            if !ok {
                unresolved.push(format!("{}!{}", import.dll, import.name));
            }
            *addr
        })
        .map_err(|x| LoadError::Pe(x.to_string()))?;
    attach_tls(e, &image, base)?;
    let idx = state(e).plugins.len();
    let query = image.export("NVSEPlugin_Query").map(|r| base + r);
    let load_fn = image.export("NVSEPlugin_Load").map(|r| base + r);
    state(e).plugins.push(Plugin {
        file: file.to_string(),
        name: file.to_string(),
        base,
        unresolved: unresolved.clone(),
        query_at: query.unwrap_or(0),
        load_at: load_fn.unwrap_or(0),
        ..Plugin::default()
    });
    if !unresolved.is_empty() {
        let line = format!(
            "{file}: imports the host does not provide: {}",
            unresolved.join(", ")
        );
        report(e, line);
    }
    if query.is_none() || load_fn.is_none() {
        let p = &mut state(e).plugins[idx];
        p.failed = Some("no NVSEPlugin_Query or NVSEPlugin_Load export".into());
        return Err(LoadError::NoQuery);
    }
    // The DLL's entry point, as the loader runs it: C++ globals are made here.
    if image.entry != 0 && state(e).entry_points {
        let r = guarded(e, idx, "DllMain", base + image.entry, &[base, 1, 0]);
        if r.is_some_and(|v| v & 0xff == 0) {
            let p = &mut state(e).plugins[idx];
            p.failed = Some("DllMain answered no".into());
        }
        if let Some(why) = state(e).plugins[idx].failed.clone() {
            return Err(LoadError::Refused(why));
        }
    }
    Ok(idx)
}

/// Phase 2: `NVSEPlugin_Query(nvse, info)`.
fn query_one(e: &mut Engine, idx: usize) -> Result<(), LoadError> {
    let (file, query) = {
        let p = &state(e).plugins[idx];
        (p.file.clone(), p.query_at)
    };
    let nvse = state(e).nvse;
    let info = e.mem.alloc(16);
    e.mem.zero(info, 16);
    let ok = guarded(e, idx, "NVSEPlugin_Query", query, &[nvse, info]);
    if ok.map(|v| v & 0xff) != Some(1) {
        e.mem.free(info);
        let why = state(e).plugins[idx]
            .failed
            .clone()
            .unwrap_or_else(|| "NVSEPlugin_Query answered no".into());
        return Err(LoadError::Refused(why));
    }
    let name = e.mem.u32(info + 4);
    let name = if name != 0 {
        String::from_utf8_lossy(&e.mem.cstr(name)).into_owned()
    } else {
        file
    };
    let version = e.mem.u32(info + 8);
    e.mem.free(info);
    let p = &mut state(e).plugins[idx];
    p.name = name;
    p.version = version;
    p.queried = true;
    Ok(())
}

/// Phase 3: `NVSEPlugin_Load(nvse)`, then the plugin's place in xNVSE's list.
fn load_one(e: &mut Engine, idx: usize) -> Result<(), LoadError> {
    let (file, load_fn) = {
        let p = &state(e).plugins[idx];
        (p.file.clone(), p.load_at)
    };
    let nvse = state(e).nvse;
    let ok = guarded(e, idx, "NVSEPlugin_Load", load_fn, &[nvse]);
    if ok.map(|v| v & 0xff) != Some(1) {
        let why = state(e).plugins[idx]
            .failed
            .clone()
            .unwrap_or_else(|| "NVSEPlugin_Load answered no".into());
        return Err(LoadError::Refused(why));
    }
    state(e).plugins[idx].loaded = true;
    // Its place in xNVSE's list (IsPluginInstalled, GetPluginVersion, its messages).
    let (pname, version, opcode_base) = {
        let p = &state(e).plugins[idx];
        (p.name.clone(), p.version, p.opcode_base)
    };
    // Its co-save callbacks: its section's header is its opcode base, so a plugin with none has
    // no section (xNVSE's `GetInfoFromBase` refuses base 0).
    let (save, load, pre_load, new_game) = interfaces::hooks(e, idx);
    let (save, load, pre_load) = if opcode_base == 0 {
        if save.is_some() || load.is_some() || pre_load.is_some() {
            report(
                e,
                format!("{file}: co-save callbacks of a plugin with no opcode base are not called"),
            );
        }
        (None, None, None)
    } else {
        (save, load, pre_load)
    };
    plugins::register(
        e,
        BuiltIn {
            file: Box::leak(file.into_boxed_str()),
            name: Box::leak(pname.into_boxed_str()),
            version,
            // A plugin with no commands has no opcode base: a base no opcode reaches.
            opcode_base: if opcode_base != 0 {
                opcode_base
            } else {
                0xffff_0000 + idx as u32
            },
            listed: true,
            listener: Some(LISTENERS[idx]),
            save,
            load,
            pre_load,
            new_game,
        },
    );
    Ok(())
}

#[cfg(test)]
engine_core::assemble!();

#[cfg(test)]
mod testdll;
#[cfg(test)]
mod tests;
