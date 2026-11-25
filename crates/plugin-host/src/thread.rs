// SPDX-License-Identifier: GPL-3.0-only
//! The thread block the `fs` segment points at, as 32-bit Windows lays out the start of its
//! Thread Information Block (Microsoft's published `NT_TIB` and the documented `TEB` fields a
//! compiler's code reads), and the implicit thread-local storage of a plugin image (the PE TLS
//! directory, data directory 9, from Microsoft's PE format).
//!
//! Plugin code runs on one guest thread, so there is one block for the engine:
//!
//! | offset | field | value |
//! |---:|---|---|
//! | `0x00` | `ExceptionList` | the SEH chain's head; `0xFFFFFFFF` is its end |
//! | `0x04` | `StackBase` | the top of the guest stack |
//! | `0x08` | `StackLimit` | its bottom |
//! | `0x18` | `Self` | the block's own address |
//! | `0x20` | process id | 1, as `GetCurrentProcessId` answers |
//! | `0x24` | thread id | 1, as `GetCurrentThreadId` answers |
//! | `0x2c` | `ThreadLocalStoragePointer` | the array of each image's TLS block |
//! | `0x30` | `ProcessEnvironmentBlock` | a zeroed block |
//!
//! Every other word is 0.

use crate::state;
use engine_core::prelude::*;

pub const EXCEPTION_LIST: u32 = 0x00;
pub const STACK_BASE: u32 = 0x04;
pub const STACK_LIMIT: u32 = 0x08;
pub const SELF: u32 = 0x18;
pub const PROCESS_ID: u32 = 0x20;
pub const THREAD_ID: u32 = 0x24;
pub const TLS_POINTER: u32 = 0x2c;
pub const PEB: u32 = 0x30;
/// The end of the SEH chain.
pub const CHAIN_END: u32 = 0xffff_ffff;
const BLOCK: u32 = 0x1000;
/// How many images may have implicit TLS (each a word of the array).
pub const TLS_SLOTS: u32 = 64;

/// The thread block and how many TLS indexes are given.
#[derive(Default)]
pub struct Thread {
    pub block: u32,
    tls_used: u32,
}

/// The thread block, made on first use with the guest stack `[limit, base)`.
pub fn block(e: &mut Engine, base: u32, limit: u32) -> u32 {
    if state(e).thread.block != 0 {
        return state(e).thread.block;
    }
    let at = e.mem.alloc(BLOCK);
    e.mem.zero(at, BLOCK);
    let peb = e.mem.alloc(BLOCK);
    e.mem.zero(peb, BLOCK);
    let tls = e.mem.alloc(4 * TLS_SLOTS);
    e.mem.zero(tls, 4 * TLS_SLOTS);
    for (off, v) in [
        (EXCEPTION_LIST, CHAIN_END),
        (STACK_BASE, base),
        (STACK_LIMIT, limit),
        (SELF, at),
        (PROCESS_ID, 1),
        (THREAD_ID, 1),
        (TLS_POINTER, tls),
        (PEB, peb),
    ] {
        e.mem.set_u32(at + off, v);
    }
    state(e).thread.block = at;
    at
}

/// `IMAGE_TLS_DIRECTORY32`, its addresses already relocated in the mapped image.
struct TlsDir {
    start: u32,
    end: u32,
    index_at: u32,
    zero_fill: u32,
}

/// The implicit TLS of an image mapped at `base` whose TLS directory is at `rva`: the template
/// copied into a new block, the block put in the next slot of the thread's array and the slot's
/// number written where the image keeps its index (`AddressOfIndex`). TLS callbacks are not
/// run. `Err` names why the image's TLS cannot be given.
pub fn attach_tls(e: &mut Engine, base: u32, rva: u32) -> Result<(), String> {
    let dir = base.wrapping_add(rva);
    let mut b = [0u8; 24];
    e.mem
        .try_read(dir, &mut b)
        .map_err(|_| "the TLS directory lies outside the image".to_string())?;
    let w = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let d = TlsDir {
        start: w(0),
        end: w(4),
        index_at: w(8),
        zero_fill: w(16),
    };
    let len = d
        .end
        .checked_sub(d.start)
        .ok_or("the TLS template ends before it starts")?;
    let total = len
        .checked_add(d.zero_fill)
        .filter(|&n| n <= 0x100_0000)
        .ok_or("the TLS block is larger than 16 MiB")?;
    let mut template = vec![0u8; len as usize];
    e.mem
        .try_read(d.start, &mut template)
        .map_err(|_| "the TLS template lies outside the image".to_string())?;
    let block = thread_block(e);
    let index = state(e).thread.tls_used;
    if index >= TLS_SLOTS {
        return Err(format!("more than {TLS_SLOTS} images with TLS"));
    }
    e.mem
        .try_write(d.index_at, &index.to_le_bytes())
        .map_err(|_| "the TLS index lies outside the image".to_string())?;
    let at = e.mem.alloc(total.max(4));
    e.mem.zero(at, total.max(4));
    e.mem.write(at, &template);
    let array = e.mem.u32(block + TLS_POINTER);
    e.mem.set_u32(array + 4 * index, at);
    state(e).thread.tls_used = index + 1;
    Ok(())
}

/// The thread block, with the guest stack made first if no call has run yet.
pub(crate) fn thread_block(e: &mut Engine) -> u32 {
    let (base, limit) = crate::guest_stack(e);
    block(e, base, limit)
}

#[cfg(test)]
#[path = "thread_tests.rs"]
mod tests;
