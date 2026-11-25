// SPDX-License-Identifier: GPL-3.0-only
//! The `fs` segment: the prefix in the interpreter (synthetic code, as compilers emit it for the
//! SEH chain and the thread block), the block the host makes, the chain put back after a call
//! and an image's implicit TLS.

use super::*;
use crate::call_guest;
use crate::x86::{Bridge, Cpu, Stop, EAX, ECX, ESP};

struct NoHost;
impl Bridge for NoHost {
    fn at(&mut self, _: &mut Engine, _: &mut Cpu, _: u32) -> Option<Result<(), Stop>> {
        None
    }
}

/// Runs `code` with `fs` at a block of its own whose words are `fs_words`; the engine, the CPU,
/// the block's address and how the run ended.
fn run_fs(code: &[u8], fs_words: &[(u32, u32)]) -> (Engine, Cpu, u32, Result<(), Stop>) {
    let mut e = Engine::new();
    let at = e.mem.alloc(0x3000);
    e.mem.write(at, code);
    let fs = at + 0x2000;
    for &(off, v) in fs_words {
        e.mem.set_u32(fs + off, v);
    }
    let mut cpu = Cpu {
        fs_base: fs,
        ..Cpu::default()
    };
    cpu.r[ESP] = at + 0x1ff0;
    cpu.eip = at;
    let end = at + code.len() as u32;
    let r = cpu.run(&mut e, &mut NoHost, end, 1000);
    (e, cpu, fs, r)
}

/// `mov eax, fs:[0]` (`64 a1`), `push dword fs:[0]` (`64 ff 35`), `mov fs:[0], ecx`
/// (`64 89 0d`), `mov ecx, fs:[0x18]` (`64 8b 0d`): each reads or writes the block, not address
/// 0, as an MSVC function's SEH prologue does.
#[test]
fn an_fs_prefix_reads_and_writes_the_thread_block() {
    let code = [
        0x64, 0xa1, 0, 0, 0, 0, // mov eax, fs:[0]
        0x64, 0xff, 0x35, 0, 0, 0, 0, // push fs:[0]
        0xb9, 0x44, 0x33, 0x22, 0x11, // mov ecx, 0x11223344
        0x64, 0x89, 0x0d, 0, 0, 0, 0, // mov fs:[0], ecx
        0x64, 0x8b, 0x0d, 0x18, 0, 0, 0, // mov ecx, fs:[0x18]
    ];
    let (e, cpu, fs, r) = run_fs(&code, &[(0, 0xffff_ffff), (0x18, 0xabcd_0000)]);
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(cpu.r[EAX], 0xffff_ffff);
    assert_eq!(e.mem.u32(cpu.r[ESP]), 0xffff_ffff, "pushed the chain head");
    assert_eq!(e.mem.u32(fs), 0x1122_3344, "the new head");
    assert_eq!(cpu.r[ECX], 0xabcd_0000);
}

/// The short forms: `mov eax, fs:[0x2c]` with a 16-bit offset (`64 67 a1 2c 00`), an fs-based
/// operand with a register (`64 8b 01`: `mov eax, fs:[ecx]`), and `lea` gives the offset alone.
#[test]
fn a_16_bit_offset_and_a_register_operand_use_the_segment_and_lea_does_not() {
    let code = [
        0x64, 0x67, 0xa1, 0x2c, 0x00, // mov eax, fs:[0x2c]
        0x89, 0xc2, // mov edx, eax
        0xb9, 0x24, 0, 0, 0, // mov ecx, 0x24
        0x64, 0x8b, 0x01, // mov eax, fs:[ecx]
        0x64, 0x8d, 0x0d, 0x10, 0, 0, 0, // lea ecx, fs:[0x10]
    ];
    let (_, cpu, _, r) = run_fs(&code, &[(0x2c, 0x5555_0000), (0x24, 7)]);
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(cpu.r[2], 0x5555_0000);
    assert_eq!(cpu.r[EAX], 7);
    assert_eq!(cpu.r[ECX], 0x10);
}

/// What is not built stops the call and says so: `gs:`, `movs` through `fs:`, 16-bit
/// addressing with a memory operand, and a string instruction with `0x67`.
#[test]
fn gs_fs_movs_and_16_bit_addressing_stop_the_call() {
    for code in [&[0x65, 0xa1, 0, 0, 0, 0][..], &[0x64, 0xa5], &[0x67, 0xab]] {
        let (_, _, _, r) = run_fs(code, &[]);
        assert!(
            matches!(r, Err(Stop::BadOpcode { bytes, .. }) if bytes[0] == code[0]),
            "{code:02x?}: {r:?}"
        );
    }
    let (_, _, _, r) = run_fs(&[0x67, 0x8b, 0x00], &[]);
    assert!(
        matches!(&r, Err(Stop::Missing(m)) if m.contains("16-bit")),
        "{r:?}"
    );
}

/// The block the host gives: the chain's end, the stack around the guest stack, itself, the ids
/// `GetCurrentProcessId` and `GetCurrentThreadId` answer, a TLS array and a PEB; made once.
#[test]
fn the_thread_block_has_the_fields_compiled_code_reads() {
    let mut e = Engine::new();
    let b = thread_block(&mut e);
    assert_eq!(e.mem.u32(b + EXCEPTION_LIST), CHAIN_END);
    let (top, bottom) = crate::guest_stack(&mut e);
    assert_eq!(e.mem.u32(b + STACK_BASE), top);
    assert_eq!(e.mem.u32(b + STACK_LIMIT), bottom);
    assert_eq!(top - bottom, 1 << 20);
    assert_eq!(e.mem.u32(b + SELF), b);
    assert_eq!(e.mem.u32(b + PROCESS_ID), 1);
    assert_eq!(e.mem.u32(b + THREAD_ID), 1);
    assert_ne!(e.mem.u32(b + TLS_POINTER), 0);
    assert_ne!(e.mem.u32(b + PEB), 0);
    assert_eq!(thread_block(&mut e), b, "one block");
}

/// A plugin call that pushes an SEH frame (`push handler; push fs:[0]; mov fs:[0], esp`) sees
/// the block through `fs`, and the chain is as it was once the call ends, popped or not.
#[test]
fn a_call_sees_the_block_through_fs_and_the_chain_is_put_back() {
    let mut e = Engine::new();
    let code = [
        0x68, 0x78, 0x56, 0x34, 0x12, // push 0x12345678 (the handler)
        0x64, 0xff, 0x35, 0, 0, 0, 0, // push fs:[0]
        0x64, 0x89, 0x25, 0, 0, 0, 0, // mov fs:[0], esp
        0x64, 0xa1, 0x18, 0, 0, 0, // mov eax, fs:[0x18]
        0x83, 0xc4, 0x08, // add esp, 8 (the frame left in the chain)
        0xc3, // ret
    ];
    let at = e.mem.alloc(0x100);
    e.mem.write(at, &code);
    let got = call_guest(&mut e, 0, at, &[]).expect("returns");
    let b = thread_block(&mut e);
    assert_eq!(got, b, "fs:[0x18] is the block");
    assert_eq!(
        e.mem.u32(b + EXCEPTION_LIST),
        CHAIN_END,
        "the chain put back"
    );
}

/// An image's implicit TLS: the template copied with its zero fill, the block in the next slot of
/// the array and the slot's number written at `AddressOfIndex`; code reading
/// `fs:[0x2c]`, the slot and a word of it gets the template's word.
#[test]
fn an_images_tls_template_is_copied_into_its_slot() {
    let mut e = Engine::new();
    let base = e.mem.alloc(0x1000);
    e.mem.zero(base, 0x1000);
    let (dir, template, index_at) = (0x100, 0x200, 0x300);
    e.mem.set_u32(base + template, 0xcafe_f00d);
    e.mem.set_u32(base + template + 4, 0x0bad_beef);
    e.mem.set_u32(base + index_at, 0xffff_ffff);
    for (off, v) in [
        (0, base + template),
        (4, base + template + 8),
        (8, base + index_at),
        (12, 0),
        (16, 8),
        (20, 0),
    ] {
        e.mem.set_u32(base + dir + off, v);
    }
    attach_tls(&mut e, base, dir).expect("TLS given");
    assert_eq!(e.mem.u32(base + index_at), 0, "the first slot");
    let b = thread_block(&mut e);
    let block = e.mem.u32(e.mem.u32(b + TLS_POINTER));
    assert_eq!(e.mem.u32(block), 0xcafe_f00d);
    assert_eq!(e.mem.u32(block + 4), 0x0bad_beef);
    assert_eq!((e.mem.u32(block + 8), e.mem.u32(block + 12)), (0, 0));
    // A second image gets the next slot.
    e.mem.set_u32(base + index_at, 0xffff_ffff);
    attach_tls(&mut e, base, dir).expect("TLS given");
    assert_eq!(e.mem.u32(base + index_at), 1);

    // mov eax, fs:[0x2c]; mov eax, [eax]; mov eax, [eax+4]; ret
    let code = [
        0x64, 0xa1, 0x2c, 0, 0, 0, 0x8b, 0x00, 0x8b, 0x40, 0x04, 0xc3,
    ];
    let at = e.mem.alloc(0x40);
    e.mem.write(at, &code);
    assert_eq!(call_guest(&mut e, 0, at, &[]), Ok(0x0bad_beef));
}

/// A TLS directory that does not fit: the template ends before it starts, or the directory is
/// outside mapped memory. The image is refused with the reason, nothing written.
#[test]
fn a_bad_tls_directory_is_refused_with_its_reason() {
    let mut e = Engine::new();
    let base = e.mem.alloc(0x1000);
    e.mem.zero(base, 0x1000);
    e.mem.set_u32(base + 0x100, base + 0x210);
    e.mem.set_u32(base + 0x104, base + 0x200);
    let err = attach_tls(&mut e, base, 0x100).unwrap_err();
    assert!(err.contains("ends before it starts"), "{err}");
    let err = attach_tls(&mut e, 0x10, 0).unwrap_err();
    assert!(err.contains("outside"), "{err}");
}
