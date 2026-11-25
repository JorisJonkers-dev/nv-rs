// SPDX-License-Identifier: GPL-3.0-only
//! The string scan, compare and load instructions (`scas`, `cmps`, `lods`), alone and under
//! `repne` / `repe`, on synthetic code: the forms compilers emit for `strlen`, `strcmp` and
//! `memcmp`.

use super::*;

struct NoHost;
impl Bridge for NoHost {
    fn at(&mut self, _: &mut Engine, _: &mut Cpu, _: u32) -> Option<R<()>> {
        None
    }
}

/// Runs `code` with `data` at `at + 0x1000`; `setup` gets the CPU and that address, which is
/// returned with the CPU.
fn run(code: &[u8], data: &[u8], setup: impl FnOnce(&mut Cpu, u32)) -> (Cpu, R<()>, u32) {
    let mut e = Engine::new();
    let at = e.mem.alloc(0x3000);
    e.mem.write(at, code);
    e.mem.write(at + 0x1000, data);
    let mut cpu = Cpu::default();
    cpu.r[ESP] = at + 0x2ff0;
    cpu.eip = at;
    setup(&mut cpu, at + 0x1000);
    let end = at + code.len() as u32;
    let r = cpu.run(&mut e, &mut NoHost, end, 1000);
    (cpu, r, at + 0x1000)
}

/// `strlen` as MSVC inlines it: `xor eax,eax; or ecx,-1; repne scasb; not ecx; dec ecx`.
#[test]
fn repne_scasb_finds_the_nul_and_gives_the_length() {
    let code = [0x31, 0xc0, 0x83, 0xc9, 0xff, 0xf2, 0xae, 0xf7, 0xd1, 0x49];
    let (c, r, d) = run(&code, b"hello\0xyz", |c, d| c.r[EDI] = d);
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(c.r[ECX], 5);
    assert_eq!(c.r[EDI], d + 6, "one past the NUL");
}

/// `repe cmpsb` stops at the first difference with the flags of that byte pair; equal blocks
/// run the count out with ZF set.
#[test]
fn repe_cmpsb_stops_at_the_first_difference() {
    // mov ecx,6; repe cmpsb
    let code = [0xb9, 6, 0, 0, 0, 0xf3, 0xa6];
    let (c, r, d) = run(&code, b"abcXef\0abcdef", |c, d| {
        c.r[ESI] = d;
        c.r[EDI] = d + 7;
    });
    assert!(r.is_ok(), "{r:?}");
    assert!(!c.zf, "a difference");
    assert!(c.cf, "'X' is below 'd'");
    assert_eq!(c.r[ECX], 2, "four pairs compared");
    assert_eq!((c.r[ESI], c.r[EDI]), (d + 4, d + 11));
    let (c, r, d) = run(&code, b"abcdef\0abcdef", |c, d| {
        c.r[ESI] = d;
        c.r[EDI] = d + 7;
    });
    assert!(r.is_ok(), "{r:?}");
    assert!(c.zf, "equal");
    assert_eq!(c.r[ECX], 0);
    assert_eq!(c.r[ESI], d + 6);
}

/// `scasd` without a repeat compares one word and moves `edi` by 4; with the direction flag set
/// it moves back.
#[test]
fn scasd_compares_one_word_and_follows_the_direction_flag() {
    let code = [0xaf]; // scasd
    let (c, r, d) = run(&code, &0x1234_5678u32.to_le_bytes(), |c, d| {
        c.r[EDI] = d;
        c.r[EAX] = 0x1234_5678;
    });
    assert!(r.is_ok(), "{r:?}");
    assert!(c.zf);
    assert_eq!(c.r[EDI], d + 4);
    let (c, _, d2) = run(&code, &[0; 4], |c, d| {
        c.r[EDI] = d;
        c.r[EAX] = 1;
        c.df = true;
    });
    assert!(!c.zf);
    assert_eq!(c.r[EDI], d2 - 4, "moved back by 4");
}

/// `lodsb` loads the byte into `al` and leaves the rest of `eax`; `repne scasb` with a count of
/// 0 does nothing.
#[test]
fn lodsb_loads_al_and_a_zero_count_repeats_nothing() {
    let (c, r, d) = run(&[0xac], b"Z", |c, d| {
        c.r[ESI] = d;
        c.r[EAX] = 0x1122_3300;
    });
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(c.r[EAX], 0x1122_335a);
    assert_eq!(c.r[ESI], d + 1);
    let (c, r, d) = run(&[0xf2, 0xae], b"\0", |c, d| {
        c.r[EDI] = d;
        c.r[ECX] = 0;
    });
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(c.r[EDI], d, "not moved");
}

/// A scan that runs into unmapped memory stops the call with a fault, and `lods` through `fs:`
/// is not built.
#[test]
fn a_scan_into_unmapped_memory_faults_and_lods_through_fs_stops() {
    let code = [0x31, 0xc0, 0x83, 0xc9, 0xff, 0xf2, 0xae];
    let (_, r, _) = run(&code, &[1; 0x10], |c, _| c.r[EDI] = 0x10);
    assert!(matches!(r, Err(Stop::FaultAt { addr: 0x10, .. })), "{r:?}");
    let (_, r, _) = run(&[0x64, 0xac], b"", |_, _| {});
    assert!(matches!(r, Err(Stop::BadOpcode { .. })), "{r:?}");
}

/// Each round of a repeated string instruction counts against the call's budget: a scan over a
/// long run of non-zero bytes stops as a loop that does not end would, not after 4 G rounds.
#[test]
fn a_long_repeat_runs_out_the_budget() {
    let code = [0x31, 0xc0, 0x83, 0xc9, 0xff, 0xf2, 0xae];
    let (_, r, _) = run(&code, &[1; 0x1000], |c, d| c.r[EDI] = d);
    assert_eq!(r, Err(Stop::Budget));
}
