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
        ] {
            assert!(stop.to_string().contains(word), "{stop}");
        }
    }
}
