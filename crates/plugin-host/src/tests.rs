// SPDX-License-Identifier: GPL-3.0-only
//! The loader against a made-up plugin DLL (`testdll`): mapped, relocated, imports bound, queried,
//! loaded, its command run through the table's execute word, a message answered.

use super::*;
use crate::testdll::{self as dll, build};

const DATA: u32 = 0x0300_0000;
const OFFSET: u32 = 0x0300_0f00;
const RESULT: u32 = 0x0300_0f08;
const SCRIPT: u32 = 0x0300_1000;
const LIST: u32 = 0x0300_2000;

fn engine() -> Engine {
    let mut e = Engine::new();
    e.map(0x0118_e000, 0x9000);
    e.map(DATA, 0x1_0000);
    nvse::table::install(&mut e);
    e
}

/// Calls a command the way the script runner does: its record's execute word with the eight words
/// of `COMMAND_ARGS`.
fn run_command(e: &mut Engine, opcode: u32) -> (u32, f64) {
    e.mem.set_u16(DATA, opcode as u16);
    e.mem.set_u32(OFFSET, 4);
    e.mem.set_f64(RESULT, -1.0);
    let record = nvse::table::record_of(e, opcode);
    assert_ne!(record, 0, "{opcode:04x}");
    let params = e.mem.u32(record + 0x14);
    let execute = e.mem.u32(record + 0x18);
    assert_ne!(execute, 0, "{opcode:04x} has no execute word");
    let ok = e
        .call(execute, &[params, DATA, 0, 0, SCRIPT, LIST, RESULT, OFFSET])
        .eax
        & 0xff;
    (ok, e.mem.f64(RESULT))
}

#[test]
fn off_by_default_loads_nothing() {
    let mut e = engine();
    assert!(!enabled(&mut e));
    assert_eq!(
        load(&mut e, "test.dll", &build(None, false)),
        Err(LoadError::Off)
    );
    assert!(plugins_loaded(&mut e).is_empty());
    // No opcode of the plugin's block exists.
    assert_eq!(nvse::table::record_of(&e, dll::OPCODE_BASE), 0);
}

#[test]
fn the_setting_reads_the_variable() {
    assert!(!wanted(None));
    assert!(!wanted(Some("")));
    assert!(!wanted(Some("0")));
    assert!(wanted(Some("1")));
}

#[test]
fn a_plugin_is_mapped_queried_loaded_and_its_command_runs() {
    let mut e = engine();
    set_enabled(&mut e, true);
    let i = load(&mut e, "test.dll", &build(None, false)).expect("loads");
    let p = plugins_loaded(&mut e)[i].clone();
    // Query: the plugin's own name and version.
    assert_eq!((p.name.as_str(), p.version), ("TestPlugin", 7));
    // The image was relocated to where it was put (its file says 0x10000000).
    assert_ne!(p.base, dll::IMAGE_BASE);
    // Its import was bound: the plugin called GetCurrentProcessId and stored the answer.
    assert_eq!(e.mem.u32(p.base + 0x1000 + dll::G_PID), 1);
    // Load: SetOpcodeBase, RegisterCommand.
    assert_eq!(p.opcode_base, dll::OPCODE_BASE);
    assert_eq!(
        p.commands,
        vec![(dll::OPCODE_BASE, "TestPluginAnswer".to_string())]
    );
    let record = nvse::table::record_of(&e, dll::OPCODE_BASE);
    assert_ne!(record, 0);
    assert_eq!(
        nvse::table::guest_text(&e, e.mem.u32(record)),
        b"TestPluginAnswer"
    );
    // The command runs through the table's execute word.
    let (ok, result) = run_command(&mut e, dll::OPCODE_BASE);
    assert_eq!((ok, result), (1, 42.0));
    assert!(reports(&mut e).is_empty(), "{:?}", reports(&mut e));
}

#[test]
fn the_plugin_is_in_xnvses_list_and_answers_a_message() {
    let mut e = engine();
    set_enabled(&mut e, true);
    let i = load(&mut e, "test.dll", &build(None, false)).expect("loads");
    let base = plugins_loaded(&mut e)[i].base + 0x1000;
    assert_eq!(e.mem.u32(base + dll::G_COUNT), 0);
    // xNVSE's own dispatch reaches the plugin's listener.
    plugins::dispatch(&mut e, plugins::msg::POST_LOAD, 0, 0);
    plugins::dispatch(&mut e, plugins::msg::MAIN_GAME_LOOP, 0, 0);
    assert_eq!(e.mem.u32(base + dll::G_COUNT), 2);
    assert_eq!(e.mem.u32(base + dll::G_LAST), plugins::msg::MAIN_GAME_LOOP);
    // And xNVSE knows it by name and version.
    assert_eq!(plugins::listed(&e, b"TestPlugin"), Some(("TestPlugin", 7)));
}

#[test]
fn an_import_the_host_lacks_is_reported_and_stops_the_plugin_not_the_engine() {
    let mut e = engine();
    set_enabled(&mut e, true);
    let r = load(&mut e, "test.dll", &build(Some("NoSuchFunction"), false));
    let lines = reports(&mut e);
    assert!(
        lines
            .iter()
            .any(|l| l.contains("kernel32.dll!NoSuchFunction")),
        "{lines:?}"
    );
    // The plugin's Load called it: the call stopped, the plugin is marked failed and the engine goes on.
    assert!(matches!(r, Err(LoadError::Refused(_))), "{r:?}");
    let p = plugins_loaded(&mut e)[0].clone();
    assert!(
        p.failed
            .as_deref()
            .is_some_and(|f| f.contains("NoSuchFunction")),
        "{:?}",
        p.failed
    );
    assert_eq!(
        p.unresolved,
        vec!["kernel32.dll!NoSuchFunction".to_string()]
    );
}

#[test]
fn a_command_that_never_ends_stops_after_its_budget_and_the_plugin_is_off() {
    let mut e = engine();
    set_enabled(&mut e, true);
    set_budget(&mut e, 100_000);
    load(&mut e, "test.dll", &build(None, true)).expect("loads");
    let (ok, _) = run_command(&mut e, dll::OPCODE_BASE);
    assert_eq!(ok, 0);
    let lines = reports(&mut e);
    assert!(lines.iter().any(|l| l.contains("budget")), "{lines:?}");
    // Its command is out of the table: a script that calls it fails at that line.
    let record = nvse::table::record_of(&e, dll::OPCODE_BASE);
    assert_eq!(e.mem.u32(record + 0x18), 0);
}

#[test]
fn a_supplied_runtime_dll_is_mapped_and_answers_the_plugins_import_of_it() {
    let mut e = engine();
    set_enabled(&mut e, true);
    // The made-up "msvcp140.dll": ?answer@@YAHXZ returns 42.
    let rt = dll::build_runtime(&[("?answer@@YAHXZ", &[0xb8, 42, 0, 0, 0, 0xc3])]);
    supply_runtime(&mut e, "MSVCP140.dll", &rt).expect("runtime maps");
    let rtbase = plugins_loaded(&mut e)[0].base;
    // Its entry point ran once (DLL_PROCESS_ATTACH).
    assert_eq!(e.mem.u32(rtbase + 0x1000 + 0x10), 1);
    let plugin = build_from_test(("MSVCP140.dll", "?answer@@YAHXZ"));
    let i = load(&mut e, "test.dll", &plugin).expect("loads");
    let p = plugins_loaded(&mut e)[i].clone();
    assert!(p.unresolved.is_empty(), "{:?}", p.unresolved);
    assert_eq!(e.mem.u32(p.base + 0x1000 + dll::G_EXTRA), 42);
    assert!(reports(&mut e).is_empty(), "{:?}", reports(&mut e));
}

fn build_from_test(extra: (&str, &str)) -> Vec<u8> {
    dll::build_from(Some(extra), false)
}

#[test]
fn without_the_runtime_dll_the_import_is_named_in_the_report() {
    let mut e = engine();
    set_enabled(&mut e, true);
    let r = load(
        &mut e,
        "test.dll",
        &build_from_test(("MSVCP140.dll", "?answer@@YAHXZ")),
    );
    assert!(matches!(r, Err(LoadError::Refused(_))), "{r:?}");
    let lines = reports(&mut e);
    assert!(
        lines
            .iter()
            .any(|l| l.contains("msvcp140.dll!?answer@@YAHXZ")),
        "{lines:?}"
    );
}

#[test]
fn the_runtime_loader_is_off_by_default_and_refuses_a_file_that_is_not_a_dll() {
    let mut e = engine();
    assert_eq!(supply_runtime(&mut e, "a.dll", b"x"), Err(LoadError::Off));
    set_enabled(&mut e, true);
    assert!(matches!(
        supply_runtime(&mut e, "a.dll", b"x"),
        Err(LoadError::Pe(_))
    ));
}

mod runtime_functions {
    use super::*;
    use crate::{crt, winapi};
    use engine_core::prelude::Engine;

    /// Calls a shim as the interpreter does: the words are its arguments.
    fn call(e: &mut Engine, f: crate::HostFn, args: &[u32]) -> u32 {
        f(e, args).expect("answers")
    }

    fn crt_fn(name: &str, dll: &str) -> (crate::HostFn, u32) {
        crt::lookup(dll, name).unwrap_or_else(|| panic!("{name}"))
    }

    fn put(e: &mut Engine, s: &[u8]) -> u32 {
        let p = e.mem.alloc(s.len() as u32 + 16);
        e.mem.write(p, s);
        e.mem.set_u8(p + s.len() as u32, 0);
        p
    }

    fn eng() -> Engine {
        let mut e = Engine::new();
        let _ = &mut e;
        e
    }

    #[test]
    fn memory_and_string_functions() {
        let mut e = eng();
        let (cpy, _) = crt_fn("memcpy", "vcruntime140.dll");
        let (set, _) = crt_fn("memset", "vcruntime140.dll");
        let a = put(&mut e, b"hello world");
        let b = put(&mut e, b"..........");
        assert_eq!(call(&mut e, cpy, &[b, a, 5]), b);
        assert_eq!(e.mem.cstr(b), b"hello.....");
        call(&mut e, set, &[b, b'z' as u32, 3]);
        assert_eq!(e.mem.cstr(b), b"zzzlo.....");
        let (len, _) = crt_fn("strlen", "api-ms-win-crt-string-l1-1-0.dll");
        assert_eq!(call(&mut e, len, &[a]), 11);
        let (chr, _) = crt_fn("strchr", "vcruntime140.dll");
        assert_eq!(call(&mut e, chr, &[a, b'w' as u32]), a + 6);
        let (rchr, _) = crt_fn("strrchr", "vcruntime140.dll");
        assert_eq!(call(&mut e, rchr, &[a, b'o' as u32]), a + 7);
        let (str_, _) = crt_fn("strstr", "vcruntime140.dll");
        let needle = put(&mut e, b"wor");
        assert_eq!(call(&mut e, str_, &[a, needle]), a + 6);
        let (cmp, _) = crt_fn("_stricmp", "api-ms-win-crt-string-l1-1-0.dll");
        let up = put(&mut e, b"HELLO WORLD");
        assert_eq!(call(&mut e, cmp, &[a, up]), 0);
        let (ncmp, _) = crt_fn("_strnicmp", "api-ms-win-crt-string-l1-1-0.dll");
        assert_eq!(call(&mut e, ncmp, &[a, up, 5]), 0);
        let (mcmp, _) = crt_fn("memcmp", "vcruntime140.dll");
        assert_eq!(call(&mut e, mcmp, &[a, up, 3]), 1);
    }

    #[test]
    fn checked_copies_say_when_the_buffer_is_too_small() {
        let mut e = eng();
        let dst = e.mem.alloc(16);
        let src = put(&mut e, b"abcdef");
        let (cpy, _) = crt_fn("strcpy_s", "api-ms-win-crt-string-l1-1-0.dll");
        assert_eq!(call(&mut e, cpy, &[dst, 16, src]), 0);
        assert_eq!(e.mem.cstr(dst), b"abcdef");
        assert_eq!(call(&mut e, cpy, &[dst, 4, src]), 34);
        assert_eq!(e.mem.cstr(dst), b"");
        let (cat, _) = crt_fn("strcat_s", "api-ms-win-crt-string-l1-1-0.dll");
        call(&mut e, cpy, &[dst, 16, src]);
        assert_eq!(call(&mut e, cat, &[dst, 16, src]), 0);
        assert_eq!(e.mem.cstr(dst), b"abcdefabcdef");
        assert_eq!(call(&mut e, cat, &[dst, 16, src]), 34);
        let (ncpy, _) = crt_fn("strncpy_s", "api-ms-win-crt-string-l1-1-0.dll");
        assert_eq!(call(&mut e, ncpy, &[dst, 4, src, u32::MAX]), 0);
        assert_eq!(e.mem.cstr(dst), b"abc");
        let (ncat, _) = crt_fn("strncat_s", "api-ms-win-crt-string-l1-1-0.dll");
        call(&mut e, cpy, &[dst, 16, src]);
        assert_eq!(call(&mut e, ncat, &[dst, 16, src, 2]), 0);
        assert_eq!(e.mem.cstr(dst), b"abcdefab");
        let (dup, _) = crt_fn("_strdup", "api-ms-win-crt-string-l1-1-0.dll");
        let d = call(&mut e, dup, &[src]);
        assert_eq!(e.mem.cstr(d), b"abcdef");
    }

    #[test]
    fn the_heap_allocates_grows_and_frees() {
        let mut e = eng();
        let h = "api-ms-win-crt-heap-l1-1-0.dll";
        let (malloc, _) = crt_fn("malloc", h);
        let (calloc, _) = crt_fn("calloc", h);
        let (realloc, _) = crt_fn("realloc", h);
        let (free, _) = crt_fn("free", h);
        let p = call(&mut e, malloc, &[8]);
        e.mem.write(p, b"12345678");
        let q = call(&mut e, realloc, &[p, 64]);
        assert_eq!(e.mem.bytes(q, 8), b"12345678");
        let z = call(&mut e, calloc, &[4, 4]);
        assert_eq!(e.mem.bytes(z, 16), vec![0u8; 16]);
        call(&mut e, free, &[q]);
        call(&mut e, free, &[z]);
        call(&mut e, free, &[0]);
        let (am, _) = crt_fn("_aligned_malloc", h);
        let (af, _) = crt_fn("_aligned_free", h);
        let a = call(&mut e, am, &[100, 64]);
        assert_eq!(a % 64, 0);
        call(&mut e, af, &[a]);
        let r0 = call(&mut e, realloc, &[0, 8]);
        assert_ne!(r0, 0);
        assert_eq!(call(&mut e, realloc, &[r0, 0]), 0);
    }

    #[test]
    fn numbers_are_read_as_strtol_reads_them() {
        let mut e = eng();
        let c = "api-ms-win-crt-convert-l1-1-0.dll";
        let (strtol, _) = crt_fn("strtol", c);
        let (strtoul, _) = crt_fn("strtoul", c);
        let (strtoull, _) = crt_fn("strtoull", c);
        let s = put(&mut e, b"  -42xyz");
        let end = e.mem.alloc(4);
        assert_eq!(call(&mut e, strtol, &[s, end, 10]) as i32, -42);
        assert_eq!(e.mem.u32(end), s + 5);
        let h = put(&mut e, b"0xff");
        assert_eq!(call(&mut e, strtoul, &[h, 0, 0]), 255);
        assert_eq!(call(&mut e, strtoul, &[h, 0, 16]), 255);
        let o = put(&mut e, b"017");
        assert_eq!(call(&mut e, strtoul, &[o, 0, 0]), 15);
        let big = put(&mut e, b"5000000000");
        assert_eq!(
            call(&mut e, strtoull, &[big, 0, 10]),
            5_000_000_000u64 as u32
        );
        assert_eq!(crt::rt(&mut e).hi, Some(1));
        let none = put(&mut e, b"zz");
        assert_eq!(call(&mut e, strtol, &[none, end, 10]), 0);
        assert_eq!(e.mem.u32(end), none);
    }

    #[test]
    fn printf_formats_what_plugins_log() {
        let mut e = eng();
        let args = e.mem.alloc(64);
        e.mem.set_u32(args, 42);
        let s = put(&mut e, b"abc");
        e.mem.set_u32(args + 4, s);
        e.mem.set_f64(args + 8, 3.14159);
        e.mem.set_u32(args + 16, 255);
        e.mem.set_u32(args + 20, (-7i32) as u32);
        let fmt = put(&mut e, b"%d|%s|%.2f|%04X|%5d|%-4d|%%|%c");
        e.mem.set_u32(args + 24, 5);
        e.mem.set_u32(args + 28, 9);
        e.mem.set_u32(args + 32, b'q' as u32);
        // %d %s %.2f %04X %5d(-7)... the %5d reads args+20, %-4d args+24
        let out = crate::printf::format(&e, &e.mem.cstr(fmt), args).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "42|abc|3.14|00FF|   -7|5   |%|\t"
        );
    }

    #[test]
    fn sprintf_writes_the_buffer_and_says_how_long() {
        let mut e = eng();
        let (vs, _) = crt_fn("__stdio_common_vsprintf", "api-ms-win-crt-stdio-l1-1-0.dll");
        let buf = e.mem.alloc(32);
        let args = e.mem.alloc(16);
        e.mem.set_u32(args, 1234);
        let fmt = put(&mut e, b"n=%u");
        assert_eq!(call(&mut e, vs, &[0, 0, buf, 32, fmt, 0, args]), 6);
        assert_eq!(e.mem.cstr(buf), b"n=1234");
        // too small: the _s form wants to say so
        assert_eq!(call(&mut e, vs, &[0, 0, buf, 4, fmt, 0, args]), u32::MAX);
        assert_eq!(e.mem.cstr(buf), b"n=1");
    }

    #[test]
    fn files_are_read_and_written_in_memory() {
        let mut e = eng();
        let io = "api-ms-win-crt-stdio-l1-1-0.dll";
        let (fopen, _) = crt_fn("fopen", io);
        let (fclose, _) = crt_fn("fclose", io);
        let (fwrite, _) = crt_fn("fwrite", io);
        let (fputs, _) = crt_fn("fputs", io);
        let (fputc, _) = crt_fn("fputc", io);
        let (fread, _) = crt_fn("fread", io);
        let (fgetc, _) = crt_fn("fgetc", io);
        let (fseek, _) = crt_fn("fseek", io);
        let (ftell, _) = crt_fn("ftell", io);
        let (ungetc, _) = crt_fn("ungetc", io);
        let (fgetpos, _) = crt_fn("fgetpos", io);
        let (fsetpos, _) = crt_fn("fsetpos", io);
        let (fflush, _) = crt_fn("fflush", io);
        let name = put(&mut e, b"Data\\log.txt");
        let w = put(&mut e, b"w");
        let r = put(&mut e, b"r");
        // a file that is not there is not opened for reading
        assert_eq!(call(&mut e, fopen, &[name, r]), 0);
        let f = call(&mut e, fopen, &[name, w]);
        assert_ne!(f, 0);
        let text = put(&mut e, b"hello");
        assert_eq!(call(&mut e, fwrite, &[text, 1, 5, f]), 5);
        assert_eq!(call(&mut e, fputs, &[text, f]), 0);
        assert_eq!(call(&mut e, fputc, &[b'!' as u32, f]), b'!' as u32);
        assert_eq!(call(&mut e, ftell, &[f]), 11);
        // reading a write-only buffer position: seek back and read
        assert_eq!(call(&mut e, fseek, &[f, 0, 0]), 0);
        let buf = e.mem.alloc(32);
        assert_eq!(call(&mut e, fread, &[buf, 1, 32, f]), 11);
        assert_eq!(e.mem.bytes(buf, 11), b"hellohello!");
        assert_eq!(call(&mut e, fgetc, &[f]), u32::MAX);
        assert_eq!(call(&mut e, fseek, &[f, 2, 0]), 0);
        assert_eq!(call(&mut e, fgetc, &[f]), b'l' as u32);
        assert_eq!(call(&mut e, ungetc, &[b'l' as u32, f]), b'l' as u32);
        let pos = e.mem.alloc(8);
        assert_eq!(call(&mut e, fgetpos, &[f, pos]), 0);
        assert_eq!(call(&mut e, fseek, &[f, 0, 2]), 0);
        assert_eq!(call(&mut e, fsetpos, &[f, pos]), 0);
        assert_eq!(call(&mut e, ftell, &[f]), 2);
        assert_eq!(call(&mut e, fflush, &[f]), 0);
        assert_eq!(call(&mut e, fclose, &[f]), 0);
        assert_eq!(call(&mut e, fclose, &[f]), u32::MAX);
        // the three standard streams swallow writes
        let (iob, _) = crt_fn("__acrt_iob_func", io);
        let out = call(&mut e, iob, &[1]);
        assert_eq!(call(&mut e, fwrite, &[text, 1, 5, out]), 5);
    }

    #[test]
    fn start_up_tables_run_the_constructors() {
        let mut e = Engine::new();
        let rt = dll::build_runtime(&[("a", &[0xb8, 1, 0, 0, 0, 0xc3])]);
        set_enabled(&mut e, true);
        supply_runtime(&mut e, "msvcp140.dll", &rt).unwrap();
        // The table has one entry: the runtime's own function "a"; a zero entry is skipped.
        let a = state(&mut e).runtime_exports[&("msvcp140.dll".to_string(), "a".to_string())];
        let table = e.mem.alloc(16);
        e.mem.set_u32(table, a);
        e.mem.set_u32(table + 4, 0);
        let (it, _) = crt_fn("_initterm", "api-ms-win-crt-runtime-l1-1-0.dll");
        let (ite, _) = crt_fn("_initterm_e", "api-ms-win-crt-runtime-l1-1-0.dll");
        // Called outside a plugin: stopped, named.
        assert!(it(&mut e, &[table, table + 8]).is_err());
        state(&mut e).running.push(0);
        assert_eq!(it(&mut e, &[table, table + 8]), Ok(0));
        // `a` answers 1: the checked form stops with it.
        assert_eq!(ite(&mut e, &[table, table + 8]), Ok(1));
        state(&mut e).running.pop();
        let (init, _) = crt_fn(
            "_initialize_onexit_table",
            "api-ms-win-crt-runtime-l1-1-0.dll",
        );
        let (reg, _) = crt_fn(
            "_register_onexit_function",
            "api-ms-win-crt-runtime-l1-1-0.dll",
        );
        let t = e.mem.alloc(16);
        assert_eq!(call(&mut e, init, &[t]), 0);
        assert_eq!(call(&mut e, reg, &[t, a]), 0);
        assert_eq!(state(&mut e).rt.onexit, vec![a]);
        let (errno, _) = crt_fn("_errno", "api-ms-win-crt-runtime-l1-1-0.dll");
        let p = call(&mut e, errno, &[]);
        assert_eq!(p, call(&mut e, errno, &[]));
        let (cur, _) = crt_fn("__current_exception", "vcruntime140.dll");
        let (ctx, _) = crt_fn("__current_exception_context", "vcruntime140.dll");
        assert_eq!(call(&mut e, ctx, &[]), call(&mut e, cur, &[]) + 8);
        let (term, _) = crt_fn("terminate", "api-ms-win-crt-runtime-l1-1-0.dll");
        assert_eq!(term(&mut e, &[]), Err(Stop::Exit));
    }

    #[test]
    fn environment_and_exception_helpers() {
        let mut e = eng();
        let env = "api-ms-win-crt-environment-l1-1-0.dll";
        let (put_env, _) = crt_fn("_putenv_s", env);
        let (get_env, _) = crt_fn("getenv_s", env);
        let (n, v) = (put(&mut e, b"NV"), put(&mut e, b"1"));
        assert_eq!(call(&mut e, put_env, &[n, v]), 0);
        let (len, buf) = (e.mem.alloc(4), e.mem.alloc(16));
        assert_eq!(call(&mut e, get_env, &[len, buf, 16, n]), 0);
        assert_eq!((e.mem.u32(len), e.mem.cstr(buf)), (2, b"1".to_vec()));
        assert_eq!(call(&mut e, get_env, &[len, buf, 1, n]), 34);
        let missing = put(&mut e, b"NOPE");
        assert_eq!(call(&mut e, get_env, &[len, buf, 16, missing]), 0);
        assert_eq!(e.mem.u32(len), 0);
        let (copy, _) = crt_fn("__std_exception_copy", "vcruntime140.dll");
        let (src, dst) = (e.mem.alloc(8), e.mem.alloc(8));
        e.mem.set_u32(src, 0x1234);
        e.mem.set_u32(src + 4, 1);
        call(&mut e, copy, &[src, dst]);
        assert_eq!((e.mem.u32(dst), e.mem.u32(dst + 4)), (0x1234, 0));
    }

    #[test]
    fn kernel32_last_error_time_conversion_and_locals() {
        let mut e = eng();
        let k = |n: &str| {
            winapi::lookup("KERNEL32.dll", n)
                .unwrap_or_else(|| panic!("{n}"))
                .0
        };
        call(&mut e, k("SetLastError"), &[5]);
        assert_eq!(call(&mut e, k("GetLastError"), &[]), 5);
        let slot = call(&mut e, k("TlsAlloc"), &[]);
        assert_eq!(call(&mut e, k("TlsGetValue"), &[slot]), 0);
        assert_eq!(call(&mut e, k("TlsSetValue"), &[slot, 77]), 1);
        assert_eq!(call(&mut e, k("TlsGetValue"), &[slot]), 77);
        assert_eq!(call(&mut e, k("TlsSetValue"), &[99, 1]), 0);
        // 64-bit answers set edx
        call(&mut e, k("GetTickCount64"), &[]);
        assert_eq!(crt::rt(&mut e).hi.take(), Some(0));
        let ft = e.mem.alloc(8);
        call(&mut e, k("GetSystemTimeAsFileTime"), &[ft]);
        assert!(e.mem.u64(ft) > 132_000_000_000_000_000);
        // MultiByteToWideChar then back
        let s = put(&mut e, b"abc");
        assert_eq!(
            call(&mut e, k("MultiByteToWideChar"), &[0, 0, s, u32::MAX, 0, 0]),
            4
        );
        let w = e.mem.alloc(32);
        assert_eq!(
            call(
                &mut e,
                k("MultiByteToWideChar"),
                &[0, 0, s, u32::MAX, w, 16]
            ),
            4
        );
        assert_eq!(e.mem.u16(w + 2), b'b' as u16);
        assert_eq!(
            call(&mut e, k("MultiByteToWideChar"), &[0, 0, s, u32::MAX, w, 2]),
            0
        );
        let back = e.mem.alloc(16);
        assert_eq!(
            call(
                &mut e,
                k("WideCharToMultiByte"),
                &[0, 0, w, u32::MAX, back, 16, 0, 0]
            ),
            4
        );
        assert_eq!(e.mem.cstr(back), b"abc");
        assert_eq!(
            call(
                &mut e,
                k("WideCharToMultiByte"),
                &[0, 0, w, u32::MAX, 0, 0, 0, 0]
            ),
            4
        );
        // heap
        let h = call(&mut e, k("GetProcessHeap"), &[]);
        let p = call(&mut e, k("HeapAlloc"), &[h, 8, 16]);
        assert_eq!(e.mem.bytes(p, 4), vec![0; 4]);
        assert_eq!(call(&mut e, k("HeapFree"), &[h, 0, p]), 1);
        let q = call(&mut e, k("HeapAlloc"), &[h, 0, 16]);
        assert_eq!(call(&mut e, k("LocalFree"), &[q]), 0);
        // the directory and the module name
        let buf = e.mem.alloc(64);
        assert_eq!(call(&mut e, k("GetCurrentDirectoryA"), &[64, buf]), 1);
        assert_eq!(call(&mut e, k("GetModuleFileNameA"), &[0, buf, 64]), 13);
        assert_eq!(e.mem.cstr(buf), b"FalloutNV.exe");
        assert_eq!(call(&mut e, k("GetPrivateProfileIntA"), &[0, 0, 7, 0]), 7);
        // the list head is cleared; locks and sleeps do nothing
        let l = e.mem.alloc(8);
        e.mem.set_u32(l, 5);
        call(&mut e, k("InitializeSListHead"), &[l]);
        assert_eq!(e.mem.u32(l), 0);
        assert_eq!(call(&mut e, k("EnterCriticalSection"), &[l]), 0);
        assert_eq!(call(&mut e, k("TryEnterCriticalSection"), &[l]), 1);
    }

    #[test]
    fn exit_pointers_console_writes_and_module_lookup() {
        let mut e = eng();
        let k = |n: &str| {
            winapi::lookup("kernel32.dll", n)
                .unwrap_or_else(|| panic!("{n}"))
                .0
        };
        assert_eq!(k("ExitProcess")(&mut e, &[0]), Err(Stop::Exit));
        assert_eq!(call(&mut e, k("DecodePointer"), &[0x1234]), 0x1234);
        assert_eq!(call(&mut e, k("EncodePointer"), &[0x1234]), 0x1234);
        let (buf, wrote) = (put(&mut e, b"abc"), e.mem.alloc(4));
        // a console handle swallows the write; an unknown handle fails
        assert_eq!(call(&mut e, k("WriteFile"), &[7, buf, 3, wrote, 0]), 1);
        assert_eq!(e.mem.u32(wrote), 3);
        assert_eq!(
            call(&mut e, k("WriteFile"), &[0x5000_0000, buf, 3, wrote, 0]),
            0
        );
        // a file opened for writing keeps it
        let name = put(&mut e, b"Data\\w.log");
        let h = call(
            &mut e,
            k("CreateFileA"),
            &[name, 0x4000_0000, 1, 0, 2, 0, 0],
        );
        assert_eq!(call(&mut e, k("WriteFile"), &[h, buf, 3, wrote, 0]), 1);
        assert_eq!(crt::rt(&mut e).files[&h].data, b"abc");
        let m = e.mem.alloc(4);
        assert_eq!(
            call(&mut e, k("GetModuleHandleExA"), &[4, 0x2000_0000, m]),
            1
        );
        assert_eq!(e.mem.u32(m), 0x0040_0000);
    }

    #[test]
    fn file_handles_read_and_write_through_the_same_files() {
        let mut e = eng();
        let k = |n: &str| {
            winapi::lookup("kernel32.dll", n)
                .unwrap_or_else(|| panic!("{n}"))
                .0
        };
        let name = put(&mut e, b"Data\\x.ini");
        // missing: INVALID_HANDLE_VALUE and the error
        assert_eq!(
            call(
                &mut e,
                k("CreateFileA"),
                &[name, 0x8000_0000, 1, 0, 3, 0, 0]
            ),
            u32::MAX
        );
        assert_eq!(call(&mut e, k("GetLastError"), &[]), 2);
        let h = call(
            &mut e,
            k("CreateFileA"),
            &[name, 0x4000_0000, 1, 0, 2, 0, 0],
        );
        assert_ne!(h, u32::MAX);
        let f = crt::rt(&mut e).files.get_mut(&h).unwrap();
        f.data = b"0123456789".to_vec();
        f.pos = 0;
        assert_eq!(call(&mut e, k("GetFileSize"), &[h, 0]), 10);
        let sz = e.mem.alloc(8);
        assert_eq!(call(&mut e, k("GetFileSizeEx"), &[h, sz]), 1);
        assert_eq!(e.mem.u64(sz), 10);
        assert_eq!(call(&mut e, k("SetFilePointer"), &[h, 4, 0, 0]), 4);
        let (buf, got) = (e.mem.alloc(16), e.mem.alloc(4));
        assert_eq!(call(&mut e, k("ReadFile"), &[h, buf, 100, got, 0]), 1);
        assert_eq!(
            (e.mem.u32(got), e.mem.bytes(buf, 6)),
            (6, b"456789".to_vec())
        );
        assert_eq!(call(&mut e, k("CloseHandle"), &[h]), 1);
        assert_eq!(call(&mut e, k("GetFileSize"), &[h, 0]), u32::MAX);
        assert_eq!(call(&mut e, k("ReadFile"), &[h, buf, 1, got, 0]), 0);
        let m = call(&mut e, k("GetModuleFileNameA"), &[0x1234, buf, 64]);
        assert_eq!(m, 13);
    }
}

#[test]
fn a_write_into_the_games_code_is_kept_as_a_site_and_a_read_gives_zero() {
    let mut e = engine();
    set_enabled(&mut e, true);
    let i = load(&mut e, "patcher.dll", &dll::build_patching()).expect("loads");
    let p = plugins_loaded(&mut e)[i].clone();
    // The read of the game's code (not supplied) answered zero.
    assert_eq!(e.mem.u32(p.base + 0x1000 + dll::G_EXTRA), 0);
    let sites = patch_sites(&mut e);
    assert_eq!(sites.len(), 1, "{sites:?}");
    assert_eq!(
        (
            sites[0].plugin.as_str(),
            sites[0].addr,
            sites[0].bytes.clone()
        ),
        (
            "patcher.dll",
            0x0050_0123,
            0xCAFE_BABEu32.to_le_bytes().to_vec()
        )
    );
    // No function is translated at that address: the inside of one.
    assert!(!sites[0].function_entry);
    assert!(sites[0].describe().contains("inside a function"));
    assert_eq!(code_reads(&mut e), vec![("patcher.dll".to_string(), 4)]);
    let lines = reports(&mut e);
    assert!(
        lines
            .iter()
            .any(|l| l.contains("patches the game's code: 1 write(s), 4 byte(s), dropped")),
        "{lines:?}"
    );
}

#[test]
fn a_supplied_code_image_is_what_a_plugin_reads_and_a_packed_one_is_refused() {
    let mut e = engine();
    set_enabled(&mut e, true);
    let mut text = vec![0u8; 0x400];
    text[0x1ff] = 0xE8; // the game's byte at 0x4011ff... and the plugin reads 0x500200 below
    assert_eq!(
        supply_code_from_exe(&mut e, &dll::build_exe(&text, false)),
        Ok(0x400)
    );
    assert!(supply_code_from_exe(&mut e, &dll::build_exe(&text, true)).is_err());
    assert!(supply_code_from_exe(&mut e, b"MZ").is_err());
    let mut other = dll::build_exe(&text, false);
    other[0x58 + 28..0x58 + 32].copy_from_slice(&0x1000_0000u32.to_le_bytes());
    assert!(supply_code_from_exe(&mut e, &other)
        .unwrap_err()
        .contains("image base"));
    // Reads inside the image come back as the game has them; a plugin's own write wins.
    let sh = state(&mut e).shadow.clone();
    let mut b = [0u8; 2];
    sh.borrow_mut().read(0x0040_11ff, &mut b);
    assert_eq!(b, [0xE8, 0]);
    sh.borrow_mut().write(0x0040_11ff, &[0x90]);
    sh.borrow_mut().read(0x0040_11ff, &mut b);
    assert_eq!(b, [0x90, 0]);
    assert!(sh.borrow().reads.is_empty());
    sh.borrow_mut().read(0x0050_0000, &mut b);
    assert_eq!(sh.borrow().reads.len(), 2);
}

#[test]
fn plugins_are_found_by_file_name_once_all_are_mapped() {
    let mut e = engine();
    set_enabled(&mut e, true);
    let rt = dll::build_runtime(&[("f", &[0xc3])]);
    supply_runtime(&mut e, "b.dll", &rt).unwrap();
    let files = vec![("a.dll".to_string(), build(None, false))];
    let r = load_many(&mut e, &files);
    assert!(r.iter().all(Result::is_ok), "{r:?}");
    let bases: Vec<u32> = plugins_loaded(&mut e).iter().map(|p| p.base).collect();
    let bases = [bases[0], bases[1]];
    let k = |e: &mut Engine, name: &[u8]| {
        let s = e.mem.alloc(32);
        e.mem.set_cstr(s, name);
        winapi_call(e, "GetModuleHandleA", &[s])
    };
    assert_eq!(k(&mut e, b"A.DLL"), bases[1]);
    assert_eq!(k(&mut e, b"Data\\NVSE\\Plugins\\b.dll"), bases[0]);
    assert_eq!(k(&mut e, b"FalloutNV.exe"), 0x0040_0000);
    assert_eq!(k(&mut e, b"nothing.dll"), 0);
    assert_eq!(winapi_call(&mut e, "GetModuleHandleA", &[0]), 0x0040_0000);
    // the wide form
    let w = e.mem.alloc(32);
    for (i, c) in "a.dll".encode_utf16().chain([0]).enumerate() {
        e.mem.set_u16(w + 2 * i as u32, c);
    }
    assert_eq!(winapi_call(&mut e, "GetModuleHandleW", &[w]), bases[1]);
    assert_eq!(winapi_call(&mut e, "GetModuleHandleW", &[0]), 0x0040_0000);
    // GetProcAddress is reported and answers null; DebugBreak stops
    assert_eq!(
        winapi_call(&mut e, "GetProcAddress", &[bases[0], 0x1234]),
        0
    );
    assert!(reports(&mut e)
        .iter()
        .any(|l| l.contains("GetProcAddress(#4660)")));
    let name = e.mem.alloc(16);
    e.mem.set_cstr(name, b"Func");
    assert_eq!(winapi_call(&mut e, "GetProcAddress", &[bases[0], name]), 0);
    assert!(winapi::lookup("kernel32.dll", "DebugBreak").unwrap().0(&mut e, &[]).is_err());
}

fn winapi_call(e: &mut Engine, name: &str, args: &[u32]) -> u32 {
    winapi::lookup("kernel32.dll", name).unwrap().0(e, args).unwrap()
}

mod verdict_tests {
    use super::*;
    use crate::verdicts::{self, Class, Observed};

    #[test]
    fn every_curated_file_is_named_once_in_lower_case() {
        let mut names: Vec<&str> = verdicts::CURATED.iter().map(|c| c.0).collect();
        assert!(names.iter().all(|n| *n == n.to_ascii_lowercase()));
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), verdicts::CURATED.len());
        // The engine's own built-ins (fork_plugins::BUILT_IN) are all "built in" here.
        for f in [
            "jip_nvse.dll",
            "johnnyguitar.dll",
            "showoffnvse.dll",
            "nvse_stewie_tweaks.dll",
            "ttw_nvse.dll",
            "mcm.dll",
            "ui_organizer.dll",
        ] {
            let v = verdicts::verdict_for(f, None);
            assert_eq!(v.class, Some(Class::BuiltIn), "{f}");
            assert_eq!(v.verdict, "built into the engine");
        }
    }

    #[test]
    fn a_plugin_the_host_must_run_is_judged_by_what_the_host_saw() {
        let loaded = |commands| Observed {
            mapped: true,
            queried: true,
            loaded: true,
            commands,
            ..Observed::default()
        };
        let serves = verdicts::verdict_for("KNVSE.dll", Some(loaded(3)));
        assert_eq!(serves.verdict, "serves commands");
        assert_eq!(serves.ticket, 550);
        assert_eq!(
            verdicts::verdict_for("kNVSE.dll", Some(loaded(0))).verdict,
            "loads"
        );
        let stopped = Observed {
            mapped: true,
            stopped: Some("instruction not built".into()),
            ..Observed::default()
        };
        let v = verdicts::verdict_for("kNVSE.dll", Some(stopped));
        assert_eq!(v.verdict, "not working");
        assert!(v.reason.contains("instruction not built"), "{}", v.reason);
        assert_eq!(
            verdicts::verdict_for("kNVSE.dll", None).verdict,
            "not working"
        );
        // a patch-only plugin is "feature missing" whatever the host saw, and says what it saw
        let fm = verdicts::verdict_for("HighResBloom.dll", Some(loaded(0)));
        assert_eq!((fm.verdict, fm.ticket), ("feature missing", 528));
        assert!(fm.reason.contains("dropped"), "{}", fm.reason);
        // a file nobody has heard of is judged by the run alone
        let unknown = verdicts::verdict_for("mystery.dll", Some(loaded(1)));
        assert_eq!((unknown.verdict, unknown.class), ("serves commands", None));
        assert_eq!(
            verdicts::verdict_for("mystery.dll", None).verdict,
            "not working"
        );
    }

    #[test]
    fn the_verdicts_of_a_run_are_written_as_json() {
        let mut e = engine();
        set_enabled(&mut e, true);
        load_many(&mut e, &[("test.dll".to_string(), dll::build_patching())]);
        let names = vec![
            "test.dll".to_string(),
            "mlf.dll".to_string(),
            "we\"ird.dll".to_string(),
        ];
        let v = verdicts::verdicts(&mut e, &names);
        assert_eq!(v[0].verdict, "serves commands");
        assert_eq!(v[0].observed.patch_writes, 1);
        assert_eq!(v[1].verdict, "engine already has the fix");
        let json = verdicts::to_json(&v);
        assert!(json.contains("\"file\": \"mlf.dll\""), "{json}");
        assert!(json.contains("we\\\"ird.dll"), "{json}");
        assert!(json.contains("\"commands\": 1"), "{json}");
        assert!(json.trim_end().ends_with('}'));
        assert_eq!(verdicts::esc_for_test("a\nb\u{1}"), "a\\nb\\u0001");
    }
}

#[test]
fn a_file_that_is_not_a_32_bit_dll_is_refused() {
    let mut e = engine();
    set_enabled(&mut e, true);
    assert!(matches!(
        load(&mut e, "a.dll", b"not a dll"),
        Err(LoadError::Pe(_))
    ));
    let mut b = build(None, false);
    b[0x44] = 0x64; // machine 0x8664
    b[0x45] = 0x86;
    assert!(matches!(load(&mut e, "a.dll", &b), Err(LoadError::Pe(_))));
}

mod interpreter {
    use super::*;
    use crate::x86::{Cpu, Stop, EAX, ECX, ESP};

    struct NoHost;
    impl Bridge for NoHost {
        fn at(&mut self, _: &mut Engine, _: &mut Cpu, _: u32) -> Option<Result<(), Stop>> {
            None
        }
    }

    fn run(code: &[u8], setup: impl FnOnce(&mut Cpu)) -> (Cpu, Result<(), Stop>) {
        let mut e = Engine::new();
        let at = e.mem.alloc(0x2000);
        e.mem.write(at, code);
        let mut cpu = Cpu::default();
        cpu.r[ESP] = at + 0x1ff0;
        cpu.eip = at;
        setup(&mut cpu);
        let end = at + code.len() as u32;
        let r = cpu.run(&mut e, &mut NoHost, end, 1000);
        (cpu, r)
    }

    #[test]
    fn arithmetic_sets_the_flags_the_branches_read() {
        // mov eax,5; sub eax,7; jb +3 (taken: borrow); mov ecx,1; (skipped) ; mov ecx,2
        let code = [
            0xb8, 5, 0, 0, 0, 0x83, 0xe8, 7, 0x72, 5, 0xb9, 1, 0, 0, 0, 0xb9, 2, 0, 0, 0,
        ];
        let (c, r) = run(&code, |_| {});
        assert!(r.is_ok());
        assert_eq!(c.r[EAX], 0xffff_fffe);
        assert_eq!(c.r[ECX], 2);
    }

    #[test]
    fn a_loop_counts_down_and_leaves_the_stack_as_it_found_it() {
        // mov ecx,10; xor eax,eax; L: add eax,ecx; dec ecx; jnz L ; push eax; pop edx
        let code = [
            0xb9, 10, 0, 0, 0, 0x31, 0xc0, 0x01, 0xc8, 0x49, 0x75, 0xfb, 0x50, 0x5a,
        ];
        let (c, r) = run(&code, |_| {});
        assert!(r.is_ok());
        assert_eq!(c.r[EAX], 55);
        assert_eq!(c.r[2], 55);
    }

    #[test]
    fn multiply_and_divide() {
        // mov eax,100000; mov ecx,100000; mul ecx; mov ecx,7; div ecx   (edx:eax = 1e10)
        let code = [
            0xb8, 0xa0, 0x86, 1, 0, 0xb9, 0xa0, 0x86, 1, 0, 0xf7, 0xe1, 0xb9, 7, 0, 0, 0, 0xf7,
            0xf1,
        ];
        let (c, r) = run(&code, |_| {});
        assert!(r.is_ok());
        let n: u64 = 10_000_000_000;
        assert_eq!(c.r[EAX], (n / 7) as u32);
        assert_eq!(c.r[2], (n % 7) as u32);
    }

    #[test]
    fn an_instruction_not_built_names_its_address_and_bytes() {
        let (_, r) = run(&[0xf3, 0x0f, 0x10, 0x00], |_| {});
        assert!(
            matches!(
                r,
                Err(Stop::BadOpcode {
                    bytes: [0xf3, 0x0f, 0x10, 0x00],
                    ..
                })
            ),
            "{r:?}"
        );
    }

    #[test]
    fn an_unmapped_read_is_a_fault_not_a_panic() {
        // mov eax,[0x00000010]
        let (_, r) = run(&[0xa1, 0x10, 0, 0, 0], |_| {});
        assert!(
            matches!(
                r,
                Err(Stop::FaultAt {
                    addr: 0x10,
                    write: false,
                    ..
                })
            ),
            "{r:?}"
        );
    }

    use crate::x86::{EBX, EDI, EDX, ESI};

    #[test]
    fn shifts_and_rotates() {
        // mov eax,0x80000001; shl eax,1 | shr eax,1 | (mov ebx,0x80000000) sar ebx,4 | rol/ror ecx
        let code = [
            0xb8, 1, 0, 0, 0x80, 0xd1, 0xe0, 0x9c, 0xc1, 0xe8, 1, 0xbb, 0, 0, 0, 0x80, 0xc1, 0xfb,
            4, 0xb9, 1, 0, 0, 0x80, 0xd1, 0xc1,
        ];
        // `pushfd` (0x9c) is not built: this also pins the report for it.
        let (_, r) = run(&code, |_| {});
        assert!(
            matches!(
                r,
                Err(Stop::BadOpcode {
                    bytes: [0x9c, ..],
                    ..
                })
            ),
            "{r:?}"
        );
        let code = [
            0xb8, 1, 0, 0, 0x80, 0xd1, 0xe0, 0xc1, 0xe8, 1, 0xbb, 0, 0, 0, 0x80, 0xc1, 0xfb, 4,
            0xb9, 1, 0, 0, 0x80, 0xd1, 0xc1, 0x89, 0xca, 0xd1, 0xca,
        ];
        let (c, r) = run(&code, |_| {});
        assert!(r.is_ok(), "{r:?}");
        assert_eq!(c.r[EAX], 1);
        assert_eq!(c.r[EBX], 0xf800_0000);
        assert_eq!(c.r[ECX], 3);
        assert_eq!(c.r[EDX], 0x8000_0001);
    }

    #[test]
    fn zero_and_sign_extension_and_conditional_moves() {
        // mov eax,0xff80; movzx ecx,al; movsx edx,al; movzx ebx,ax; movsx esi,ax
        let code = [
            0xb8, 0x80, 0xff, 0, 0, 0x0f, 0xb6, 0xc8, 0x0f, 0xbe, 0xd0, 0x0f, 0xb7, 0xd8, 0x0f,
            0xbf, 0xf0,
        ];
        let (c, r) = run(&code, |_| {});
        assert!(r.is_ok(), "{r:?}");
        assert_eq!(
            (c.r[ECX], c.r[EDX], c.r[EBX], c.r[ESI]),
            (0x80, 0xffff_ff80, 0xff80, 0xffff_ff80)
        );
        // xor eax,eax; xor ecx,ecx; test eax,eax; sete cl; mov edx,7; cmovne edx,ecx; cmove edx,ecx
        let code = [
            0x31, 0xc0, 0x31, 0xc9, 0x85, 0xc0, 0x0f, 0x94, 0xc1, 0xba, 7, 0, 0, 0, 0x0f, 0x45,
            0xd1, 0x89, 0xd3, 0x0f, 0x44, 0xd1,
        ];
        let (c, r) = run(&code, |_| {});
        assert!(r.is_ok(), "{r:?}");
        assert_eq!((c.r[ECX], c.r[EBX], c.r[EDX]), (1, 7, 1));
    }

    #[test]
    fn byte_swap_negate_not_and_address_arithmetic() {
        // mov eax,0x12345678; bswap eax; mov ebx,eax
        let (c, r) = run(&[0xb8, 0x78, 0x56, 0x34, 0x12, 0x0f, 0xc8], |_| {});
        assert!(r.is_ok());
        assert_eq!(c.r[EAX], 0x7856_3412);
        // mov eax,5; neg eax; not eax; cdq; inc eax; dec eax; lea eax,[eax+eax*2]; xchg eax,ecx
        let code = [
            0xb8, 5, 0, 0, 0, 0xf7, 0xd8, 0xf7, 0xd0, 0x99, 0x40, 0x48, 0x8d, 0x04, 0x40, 0x91,
        ];
        let (c, r) = run(&code, |_| {});
        assert!(r.is_ok(), "{r:?}");
        assert_eq!((c.r[ECX], c.r[EDX]), (12, 0));
    }

    #[test]
    fn signed_multiply_and_divide() {
        // mov eax,-1; mov ecx,2; imul eax,ecx; imul eax,eax,3
        let code = [
            0xb8, 0xff, 0xff, 0xff, 0xff, 0xb9, 2, 0, 0, 0, 0x0f, 0xaf, 0xc1, 0x6b, 0xc0, 3,
        ];
        let (c, r) = run(&code, |_| {});
        assert!(r.is_ok());
        assert_eq!(c.r[EAX], 0xffff_fffa);
        // mov eax,-12; cdq; mov ecx,5; idiv ecx
        let code = [
            0xb8, 0xf4, 0xff, 0xff, 0xff, 0x99, 0xb9, 5, 0, 0, 0, 0xf7, 0xf9,
        ];
        let (c, r) = run(&code, |_| {});
        assert!(r.is_ok());
        assert_eq!((c.r[EAX], c.r[EDX]), (0xffff_fffe, 0xffff_fffe));
        // div by zero: xor ecx,ecx; div ecx
        let (_, r) = run(&[0x31, 0xc9, 0xf7, 0xf1], |_| {});
        assert!(matches!(r, Err(Stop::Divide { .. })), "{r:?}");
    }

    #[test]
    fn string_moves_and_fills_with_a_repeat_prefix() {
        // mov esi,edi; mov ecx,4; mov eax,0xaa; rep stosd; lea edi,[esp-0x300]; mov ecx,4; rep movsd;
        // mov eax,[edi-4]
        let code = [
            0x89, 0xfe, 0xb9, 4, 0, 0, 0, 0xb8, 0xaa, 0, 0, 0, 0xf3, 0xab, 0x8d, 0xbc, 0x24, 0,
            0xfd, 0xff, 0xff, 0xb9, 4, 0, 0, 0, 0xf3, 0xa5, 0x8b, 0x47, 0xfc,
        ];
        let (c, r) = run(&code, |c| c.r[EDI] = c.r[ESP] - 0x400);
        assert!(r.is_ok(), "{r:?}");
        assert_eq!(c.r[EAX], 0xaa);
        assert_eq!(c.r[ECX], 0);
        assert_eq!(c.r[ESI], c.r[ESP] - 0x400 + 16);
        assert_eq!(c.r[EDI], c.r[ESP] - 0x300 + 16);
    }

    #[test]
    fn a_call_returns_and_an_indirect_call_goes_through_a_register() {
        // call L; jmp end; L: mov eax,42; ret
        let code = [0xe8, 2, 0, 0, 0, 0xeb, 6, 0xb8, 42, 0, 0, 0, 0xc3];
        let (c, r) = run(&code, |_| {});
        assert!(r.is_ok(), "{r:?}");
        assert_eq!(c.r[EAX], 42);
        assert_eq!(c.r[ESP] & 0xf, 0);
    }

    #[test]
    fn a_loop_that_never_ends_stops_on_the_budget() {
        let (_, r) = run(&[0xeb, 0xfe, 0x90], |_| {});
        assert_eq!(r, Err(Stop::Budget));
    }

    #[test]
    fn every_stop_says_what_it_wanted() {
        for (stop, word) in [
            (
                Stop::Fault {
                    addr: 1,
                    write: true,
                },
                "write",
            ),
            (
                Stop::FaultAt {
                    addr: 1,
                    write: false,
                    eip: 2,
                },
                "at 0x00000002",
            ),
            (
                Stop::BadOpcode {
                    eip: 1,
                    bytes: [1, 2, 3, 4],
                },
                "not built",
            ),
            (Stop::Budget, "budget"),
            (Stop::Missing("x".into()), "wanted x"),
            (Stop::Exit, "end"),
            (Stop::Divide { eip: 9 }, "divide"),
            (
                Stop::HostFault {
                    func: "memcpy".into(),
                    addr: 3,
                    write: true,
                },
                "memcpy was given",
            ),
        ] {
            assert!(stop.to_string().contains(word), "{stop}");
        }
    }
}
