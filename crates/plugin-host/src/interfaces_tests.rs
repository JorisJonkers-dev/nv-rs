// SPDX-License-Identifier: GPL-3.0-only
//! The interfaces `QueryInterface` hands out, against the made-up DLL (`testdll`) and through
//! their own function words, called as a plugin calls them (cdecl, through the interpreter).

use super::*;
use crate::testdll::{self as dll, build_ifaces};
use crate::{call_guest, load, plugins_loaded, reports, set_enabled};

const DATA: u32 = 0x0300_0000;
const OFFSET: u32 = 0x0300_0f00;
const SCRIPT: u32 = 0x0300_1000;
const LIST: u32 = 0x0300_2000;
/// Scratch memory the tests hand the interfaces as the plugin's buffers.
const BUF: u32 = 0x0300_4000;

fn engine() -> Engine {
    let mut e = Engine::new();
    e.map(0x0118_e000, 0x9000);
    e.map(DATA, 0x1_0000);
    nvse::table::install(&mut e);
    e
}

/// An engine with the made-up plugin loaded; its index.
fn loaded() -> (Engine, usize) {
    let mut e = engine();
    set_enabled(&mut e, true);
    let i = load(&mut e, "test.dll", &build_ifaces()).expect("loads");
    (e, i)
}

/// The function word at `slot` of interface `id` (after its version word when it has one).
fn word(e: &mut Engine, id: u32, slot: u32) -> u32 {
    let table = query(e, id);
    assert_ne!(table, 0, "interface {id}");
    let head = u32::from(!matches!(id, 5 | 6 | 8 | 9));
    e.mem.u32(table + 4 * (head + slot))
}

/// Calls slot `slot` of interface `id` as plugin `p` does.
fn call(e: &mut Engine, p: usize, id: u32, slot: u32, args: &[u32]) -> u32 {
    let f = word(e, id, slot);
    call_guest(e, p, f, args).expect("the call returns")
}

fn put_text(e: &mut Engine, at: u32, s: &[u8]) -> u32 {
    e.mem.write(at, s);
    e.mem.set_u8(at + s.len() as u32, 0);
    at
}

fn put_element(e: &mut Engine, at: u32, word: u64, kind: u8) -> u32 {
    e.mem.set_u64(at, word);
    e.mem.set_u8(at + 8, kind);
    at
}

/// The made-up plugin asks for every interface in its `Load`: each id but messaging's has its own
/// table now (none answers 0), and the command table finds the plugin's own command by name.
#[test]
fn a_plugin_gets_every_interface_and_finds_its_command_by_name() {
    let (mut e, i) = loaded();
    let base = plugins_loaded(&mut e)[i].base + 0x1000;
    let mut tables = Vec::new();
    for id in [0, 1, 3, 4, 5, 6, 7, 8, 9] {
        let got = e.mem.u32(base + dll::G_IFACE + 4 * id);
        assert_ne!(got, 0, "interface {id}");
        assert_eq!(got, query(&mut e, id));
        tables.push(got);
    }
    tables.sort_unstable();
    tables.dedup();
    assert_eq!(tables.len(), 9, "one table each");
    let record = nvse::table::record_of(&e, dll::OPCODE_BASE);
    assert_eq!(e.mem.u32(base + dll::G_FOUND), record);
    assert!(
        !reports(&mut e)
            .iter()
            .any(|l| l.contains("is not provided")),
        "{:?}",
        reports(&mut e)
    );
}

/// The versions xNVSE 6.4.9 writes first in the tables that have one.
#[test]
fn the_tables_carry_xnvses_versions() {
    let (mut e, _) = loaded();
    for (id, v) in [(0, 2), (1, 3), (3, 2), (4, 1), (7, 4)] {
        let t = query(&mut e, id);
        assert_eq!(e.mem.u32(t), v, "interface {id}");
    }
    // Messaging stays where it was, and an id past the last answers nothing.
    assert_eq!(query(&mut e, 2), 0);
    assert_eq!(query(&mut e, 10), 0);
    assert_eq!(query(&mut e, 99), 0);
}

/// A slot with nothing behind it stops the call and names itself; the plugin is not read through
/// a null word.
#[test]
fn a_slot_not_built_names_itself_when_called() {
    let (mut e, i) = loaded();
    let f = word(&mut e, 6, 7);
    let r = call_guest(&mut e, i, f, &[0, 0, 0]);
    assert!(
        matches!(&r, Err(Stop::Missing(n)) if n == "NVSEScriptInterface.DecompileToBuffer"),
        "{r:?}"
    );
}

/// The command table: by opcode, by name, the return type and the parent plugin (the game's
/// commands have none, xNVSE's own are "NVSE", a plugin's its `PluginInfo`), and plugins by name
/// and by DLL name without case.
#[test]
fn the_command_table_finds_commands_and_their_plugins() {
    let (mut e, i) = loaded();
    let record = nvse::table::record_of(&e, dll::OPCODE_BASE);
    assert_eq!(call(&mut e, i, 3, 2, &[dll::OPCODE_BASE]), record);
    let name = put_text(&mut e, BUF, b"TestPluginAnswer");
    assert_eq!(call(&mut e, i, 3, 3, &[name]), record);
    let none = put_text(&mut e, BUF, b"NoSuchCommandAnywhere");
    assert_eq!(call(&mut e, i, 3, 3, &[none]), 0);
    assert_eq!(call(&mut e, i, 3, 4, &[record]), 0, "a number");
    // Start and End bound the table, the plugin's record inside.
    let (start, end) = (call(&mut e, i, 3, 0, &[]), call(&mut e, i, 3, 1, &[]));
    assert!(
        start < record && record < end,
        "{start:#x} {record:#x} {end:#x}"
    );

    let info = call(&mut e, i, 3, 6, &[record]);
    assert_ne!(info, 0);
    assert_eq!(e.mem.u32(info), 1);
    assert_eq!(e.mem.cstr(e.mem.u32(info + 4)), b"TestPlugin");
    assert_eq!(e.mem.u32(info + 8), 7);
    // The same PluginInfo by its name and its DLL's, without case.
    let n = put_text(&mut e, BUF, b"testplugin");
    assert_eq!(call(&mut e, i, 3, 7, &[n]), info);
    let f = put_text(&mut e, BUF, b"TEST.DLL");
    assert_eq!(call(&mut e, i, 3, 8, &[f]), info);
    let missing = put_text(&mut e, BUF, b"NotLoaded");
    assert_eq!(call(&mut e, i, 3, 7, &[missing]), 0);

    // A command record of the game's own range and one of xNVSE's.
    e.mem.set_u32(BUF + 0x100 + 8, 0x1100);
    assert_eq!(call(&mut e, i, 3, 6, &[BUF + 0x100]), 0);
    e.mem.set_u32(BUF + 0x100 + 8, 0x1500);
    let nvse_info = call(&mut e, i, 3, 6, &[BUF + 0x100]);
    assert_eq!(e.mem.cstr(e.mem.u32(nvse_info + 4)), b"NVSE");
    assert_eq!(e.mem.u32(nvse_info + 8), nvse::PACKED_VERSION);
}

/// String variables: made by a script's mod, read, set; with no owning script nothing is made.
#[test]
fn string_variables_are_made_read_and_set() {
    let (mut e, i) = loaded();
    e.mem.set_u32(SCRIPT + 0xc, 0x0500_0001);
    let hello = put_text(&mut e, BUF, b"hello");
    let id = call(&mut e, i, 4, 2, &[hello, SCRIPT]);
    assert_ne!(id, 0);
    assert_eq!(vars::string(&e, id).as_deref(), Some(&b"hello"[..]));
    let at = call(&mut e, i, 4, 0, &[id]);
    assert_eq!(e.mem.cstr(at), b"hello");
    // The same copy while the text is the same.
    assert_eq!(call(&mut e, i, 4, 0, &[id]), at);
    let world = put_text(&mut e, BUF + 0x40, b"world");
    call(&mut e, i, 4, 1, &[id, world]);
    let again = call(&mut e, i, 4, 0, &[id]);
    assert_eq!(e.mem.cstr(again), b"world");
    assert_eq!(call(&mut e, i, 4, 0, &[0x7fff_fff0]), 0, "no such string");
    assert_eq!(call(&mut e, i, 4, 2, &[hello, 0]), 0, "no owning script");
}

/// Array variables: an array from elements, a string map and a number map, their elements read,
/// set and appended, the container types and keys, and a result assigned.
#[test]
fn array_variables_are_made_read_and_changed() {
    let (mut e, i) = loaded();
    // CreateArray([2.5, "x"]).
    put_element(&mut e, BUF, 2.5f64.to_bits(), 1);
    let x = put_text(&mut e, BUF + 0x200, b"x");
    put_element(&mut e, BUF + 16, u64::from(x), 3);
    let arr = call(&mut e, i, 5, 0, &[BUF, 2, SCRIPT]);
    assert!(vars::array_exists(&e, arr));
    assert_eq!(call(&mut e, i, 5, 6, &[arr]), 2, "size");
    assert_eq!(call(&mut e, i, 5, 10, &[arr]), 1, "packed");
    assert_eq!(call(&mut e, i, 5, 11, &[arr]), 0, "an array");
    assert_eq!(call(&mut e, i, 5, 7, &[arr]), arr, "looked up");
    // GetElement(arr, 1) -> "x".
    put_element(&mut e, BUF + 0x40, 1f64.to_bits(), 1);
    assert_eq!(call(&mut e, i, 5, 8, &[arr, BUF + 0x40, BUF + 0x60]), 1);
    assert_eq!(e.mem.u8(BUF + 0x68), 3);
    assert_eq!(e.mem.cstr(e.mem.u32(BUF + 0x60)), b"x");
    // AppendElement(arr, 9.0): a third element.
    put_element(&mut e, BUF + 0x80, 9f64.to_bits(), 1);
    call(&mut e, i, 5, 5, &[arr, BUF + 0x80]);
    assert_eq!(call(&mut e, i, 5, 6, &[arr]), 3);
    // GetElements: values and keys in order.
    assert_eq!(call(&mut e, i, 5, 9, &[arr, BUF + 0x400, BUF + 0x500]), 1);
    assert_eq!(e.mem.f64(BUF + 0x400), 2.5);
    assert_eq!(e.mem.f64(BUF + 0x420), 9.0);
    assert_eq!(e.mem.f64(BUF + 0x520), 2.0, "the third key");
    assert_eq!(call(&mut e, i, 5, 12, &[arr, BUF + 0x40]), 1, "has key 1");

    // CreateStringMap(["a"], [4.0]); SetElement(map, "b", 5.0).
    let a = put_text(&mut e, BUF + 0x210, b"a");
    e.mem.set_u32(BUF + 0x100, a);
    put_element(&mut e, BUF + 0x110, 4f64.to_bits(), 1);
    let map = call(&mut e, i, 5, 1, &[BUF + 0x100, BUF + 0x110, 1, SCRIPT]);
    assert_eq!(call(&mut e, i, 5, 11, &[map]), 2, "a string map");
    let b = put_text(&mut e, BUF + 0x220, b"b");
    put_element(&mut e, BUF + 0x140, u64::from(b), 3);
    put_element(&mut e, BUF + 0x150, 5f64.to_bits(), 1);
    call(&mut e, i, 5, 4, &[map, BUF + 0x140, BUF + 0x150]);
    assert_eq!(call(&mut e, i, 5, 6, &[map]), 2);
    assert_eq!(call(&mut e, i, 5, 8, &[map, BUF + 0x140, BUF + 0x160]), 1);
    assert_eq!(e.mem.f64(BUF + 0x160), 5.0);
    // A number key on a string map finds nothing.
    assert_eq!(call(&mut e, i, 5, 8, &[map, BUF + 0x40, BUF + 0x160]), 0);
    assert_eq!(call(&mut e, i, 5, 5, &[map, BUF + 0x80]), 0);
    assert_eq!(call(&mut e, i, 5, 6, &[map]), 2, "no append to a map");

    // CreateMap([10.0], [1.0]): a number map.
    e.mem.set_f64(BUF + 0x180, 10.0);
    put_element(&mut e, BUF + 0x190, 1f64.to_bits(), 1);
    let nmap = call(&mut e, i, 5, 2, &[BUF + 0x180, BUF + 0x190, 1, SCRIPT]);
    assert_eq!(call(&mut e, i, 5, 11, &[nmap]), 1, "a map");
    assert_eq!(call(&mut e, i, 5, 10, &[nmap]), 0);

    // AssignCommandResult: the id as the result double; an array that does not exist is refused.
    assert_eq!(call(&mut e, i, 5, 3, &[arr, BUF + 0x300]), 1);
    assert_eq!(e.mem.f64(BUF + 0x300), f64::from(arr));
    assert_eq!(call(&mut e, i, 5, 3, &[0x7fff_0000, BUF + 0x300]), 0);
    assert!(reports(&mut e)
        .iter()
        .any(|l| l.contains("returns array") && l.contains("does not exist")));
    assert_eq!(call(&mut e, i, 5, 11, &[0x7fff_0000]), u32::MAX);
    assert_eq!(call(&mut e, i, 5, 6, &[0x7fff_0000]), 0);
    assert_eq!(call(&mut e, i, 5, 7, &[0x7fff_0000]), 0);
}

/// `ExtractArgsEx` stores each argument through the next pointer: a string as its text, an
/// integer as four bytes.
#[test]
fn extract_args_stores_each_argument_through_its_pointer() {
    let (mut e, i) = loaded();
    // Parameters: a string, an integer (`ParamInfo`: type text, type id, optional).
    let params = BUF + 0x800;
    e.mem.set_u32(params + 4, 0);
    e.mem.set_u32(params + 12 + 4, 1);
    // The line: opcode, length, then the arguments at offset 4: count 2, "abc", n 42.
    let mut line = vec![0u8, 0, 0, 0, 2, 0, 3, 0];
    line.extend(b"abc");
    line.push(b'n');
    line.extend(42i32.to_le_bytes());
    e.mem.write(DATA, &line);
    e.mem.set_u32(OFFSET, 4);
    let (s, n) = (BUF + 0x900, BUF + 0x980);
    e.mem.set_u32(n, 0xdead_beef);
    let ok = call(&mut e, i, 6, 2, &[params, DATA, OFFSET, SCRIPT, LIST, s, n]);
    assert_eq!(ok, 1);
    assert_eq!(e.mem.cstr(s), b"abc");
    assert_eq!(e.mem.u32(n), 42);
    assert_eq!(e.mem.u32(OFFSET), 4, "the offset word does not move");
}

/// The co-save: a save callback's records go into its section; a load callback reads them back
/// record by record, by peek, by size and skipped; with no callback running nothing is written.
#[test]
fn co_save_records_are_written_and_read_back() {
    let (mut e, i) = loaded();
    // Outside a callback a write answers no.
    let bytes = put_text(&mut e, BUF, b"abcd");
    assert_eq!(call(&mut e, i, 0, 3, &[0x5445_5354, 1, bytes, 4]), 0);
    // While a save callback writes: two records, the second opened and filled.
    ifaces(&mut e).writing = Some(Records::default());
    assert_eq!(call(&mut e, i, 0, 3, &[0x5445_5354, 1, bytes, 4]), 1);
    assert_eq!(call(&mut e, i, 0, 4, &[0x4e55_4d53, 2]), 1);
    call(&mut e, i, 0, 12, &[0x11]);
    call(&mut e, i, 0, 13, &[0x2233]);
    call(&mut e, i, 0, 14, &[0x4455_6677]);
    e.mem.set_u64(BUF + 0x10, 0x0102_0304_0506_0708);
    call(&mut e, i, 0, 15, &[BUF + 0x10]);
    let written = ifaces(&mut e).writing.take().unwrap();
    assert_eq!(written.chunks.len(), 2);
    assert_eq!(written.chunks[0].data, b"abcd");
    assert_eq!(written.chunks[1].kind, 0x4e55_4d53);
    assert_eq!(written.chunks[1].data.len(), 1 + 2 + 4 + 8);

    // Read back.
    ifaces(&mut e).reading = Some(Reading {
        chunks: written.chunks,
        ..Reading::default()
    });
    let (t, v, l) = (BUF + 0x40, BUF + 0x44, BUF + 0x48);
    assert_eq!(call(&mut e, i, 0, 6, &[t, v, l]), 1);
    assert_eq!(
        (e.mem.u32(t), e.mem.u32(v), e.mem.u32(l)),
        (0x5445_5354, 1, 4)
    );
    assert_eq!(call(&mut e, i, 0, 11, &[BUF + 0x60, 2]), 2, "peek");
    assert_eq!(
        call(&mut e, i, 0, 7, &[BUF + 0x70, 8]),
        4,
        "the rest of the record"
    );
    assert_eq!(e.mem.cstr(BUF + 0x70), b"abcd");
    assert_eq!(call(&mut e, i, 0, 6, &[t, v, l]), 1);
    assert_eq!(call(&mut e, i, 0, 16, &[]), 0x11);
    assert_eq!(call(&mut e, i, 0, 17, &[]), 0x2233);
    assert_eq!(call(&mut e, i, 0, 18, &[]), 0x4455_6677);
    call(&mut e, i, 0, 19, &[BUF + 0x80]);
    assert_eq!(e.mem.u64(BUF + 0x80), 0x0102_0304_0506_0708);
    assert_eq!(call(&mut e, i, 0, 6, &[t, v, l]), 0, "no third record");
    ifaces(&mut e).reading = None;
    assert_eq!(call(&mut e, i, 0, 6, &[t, v, l]), 0, "no section open");
}

/// A callback the plugin set during its `Load` is called by xNVSE's save, through the plugin's
/// entry in the list; one set later is reported, not called.
#[test]
fn a_save_callback_set_in_load_is_called_by_the_save() {
    let mut e = engine();
    set_enabled(&mut e, true);
    let i = crate::map_one(&mut e, "test.dll", &build_ifaces()).unwrap();
    crate::query_one(&mut e, i).unwrap();
    // The callback: WriteRecord('TEST', 3, "xy", 2); ret. Assembled in scratch memory.
    let write = word(&mut e, 0, 3);
    let data = put_text(&mut e, BUF + 0x10, b"xy");
    let mut code = vec![0x6a, 2, 0x68];
    code.extend(data.to_le_bytes());
    code.extend([0x6a, 3, 0x68]);
    code.extend(0x5445_5354u32.to_le_bytes());
    code.push(0xb8);
    code.extend(write.to_le_bytes());
    code.extend([0xff, 0xd0, 0x83, 0xc4, 0x10, 0xc3]); // call eax; add esp,16; ret
    e.mem.write(BUF + 0x100, &code);
    // SetSaveCallback(handle, cb) as the plugin's Load would.
    call(&mut e, i, 0, 0, &[i as u32, BUF + 0x100]);
    crate::load_one(&mut e, i).unwrap();
    let entry = nvse::plugins::built_ins(&e)
        .into_iter()
        .find(|p| p.file == "test.dll")
        .unwrap();
    let mut out = Records::default();
    (entry.save.expect("a save hook"))(&mut e, &mut out);
    assert_eq!(out.chunks.len(), 1);
    assert_eq!(
        (out.chunks[0].kind, out.chunks[0].version),
        (0x5445_5354, 3)
    );
    assert_eq!(out.chunks[0].data, b"xy");
    assert!(entry.load.is_none() && entry.new_game.is_none());
    // Set again now that it is loaded: kept, and the report says it is not called.
    call(&mut e, i, 0, 1, &[i as u32, BUF + 0x100]);
    assert!(reports(&mut e)
        .iter()
        .any(|l| l.contains("set after NVSEPlugin_Load is not called")));
    // A handle that is no plugin sets nothing.
    call(&mut e, i, 0, 2, &[99, BUF + 0x100]);
    assert_eq!(state(&mut e).plugins[i].callbacks.new_game, 0);
}

/// The data interface: the lambda functions as words a plugin calls, the preload count's word,
/// and what is not given (the C++ singletons, the other functions) reported, not a stop.
#[test]
fn the_data_interface_gives_what_it_has_and_reports_the_rest() {
    let (mut e, i) = loaded();
    let is_lambda = call(&mut e, i, 7, 1, &[10]);
    assert_ne!(is_lambda, 0);
    assert_eq!(call_guest(&mut e, i, is_lambda, &[SCRIPT]).unwrap(), 0);
    assert_ne!(call(&mut e, i, 7, 1, &[8]), 0);
    assert_ne!(call(&mut e, i, 7, 1, &[9]), 0);
    assert_eq!(
        call(&mut e, i, 7, 1, &[1]),
        0,
        "inventory references: not given"
    );
    let w = call(&mut e, i, 7, 2, &[1]);
    assert_eq!(e.mem.u32(w), 0);
    assert_eq!(call(&mut e, i, 7, 2, &[2]), 0);
    assert_eq!(call(&mut e, i, 7, 0, &[1]), 0);
    let lines = reports(&mut e);
    assert!(
        lines
            .iter()
            .any(|l| l.contains("GetSingleton(1) is not provided")),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("GetFunc(1) is not provided")),
        "{lines:?}"
    );
}

/// The log path is the game folder (empty), and `DispatchEvent` of an unknown event answers no:
/// its variadic words are read past the fixed two without a fault at the stack's end.
#[test]
fn the_log_path_is_the_game_folder_and_an_unknown_event_is_not_dispatched() {
    let (mut e, i) = loaded();
    let path = call(&mut e, i, 9, 0, &[]);
    assert_eq!(e.mem.cstr(path), b"");
    let name = put_text(&mut e, BUF, b"NoSuchEvent");
    assert_eq!(call(&mut e, i, 8, 1, &[name, 0]), 0);
}
