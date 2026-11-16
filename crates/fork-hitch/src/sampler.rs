//! The hitch sampler: what the main thread is doing while one frame will not end.
//!
//! The main thread publishes, at every platform call it makes (a few stores, no lock), which call
//! it is in, the guest function chain that made it and what class of argument it has (a file's
//! extension and a hash of its name, never a path). A sampler thread reads that every couple of
//! milliseconds while the frame in progress is older than a limit, and keeps a histogram. When
//! the frame ends as a hitch the histogram goes into the record. A frame stuck in the game's own
//! code makes no platform call, so its samples say "in the game's code" and name the chain of
//! the last call it made; with `NV_PROFILE` on and [`set_stack_source`] registered the sample
//! names the running guest function chain itself.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// How many guest functions of the chain a platform call publishes (innermost last).
pub const CHAIN: usize = 8;

/// The published state of the main thread's platform call. `seq` is odd while it is written and
/// grows by two a call, so it is also the count of the main thread's platform calls.
struct CallState {
    seq: AtomicU64,
    slot: AtomicU32,
    depth: AtomicU32,
    since_ns: AtomicU64,
    /// The argument class: the extension in the low word, the name hash in the high one.
    arg: AtomicU64,
    chain: [AtomicU32; CHAIN],
}

static CALL: CallState = CallState {
    seq: AtomicU64::new(0),
    slot: AtomicU32::new(0),
    depth: AtomicU32::new(0),
    since_ns: AtomicU64::new(0),
    arg: AtomicU64::new(0),
    chain: [const { AtomicU32::new(0) }; CHAIN],
};

/// The main thread enters platform function `slot`, called from `stack` (the guest call stack,
/// the import itself last).
#[inline]
pub(crate) fn publish_enter(slot: u32, stack: &[u32], now_ns: u64) {
    let seq = CALL.seq.load(Ordering::Relaxed);
    CALL.seq.store(seq.wrapping_add(1) | 1, Ordering::Release);
    // The callers of the import: the stack without its last word.
    let callers = stack.split_last().map_or(&[][..], |(_, rest)| rest);
    let take = callers.len().min(CHAIN);
    for (i, a) in callers[callers.len() - take..].iter().enumerate() {
        CALL.chain[i].store(*a, Ordering::Relaxed);
    }
    CALL.depth.store(take as u32, Ordering::Relaxed);
    CALL.arg.store(0, Ordering::Relaxed);
    CALL.since_ns.store(now_ns, Ordering::Relaxed);
    CALL.slot.store(slot, Ordering::Relaxed);
    CALL.seq.store(seq.wrapping_add(2) & !1, Ordering::Release);
}

/// [`publish_enter`] for a frame that is not over its budget: the innermost caller only (the
/// chain's copy is about half the cost of a call's bookkeeping). A slow call that began then
/// still names its function and caller; calls made after the frame passed its budget publish
/// the whole chain.
#[inline]
pub(crate) fn publish_enter_brief(slot: u32, stack: &[u32], now_ns: u64) {
    let seq = CALL.seq.load(Ordering::Relaxed);
    CALL.seq.store(seq.wrapping_add(1) | 1, Ordering::Release);
    let top = stack.len().checked_sub(2).map_or(0, |i| stack[i]);
    CALL.chain[0].store(top, Ordering::Relaxed);
    CALL.depth.store(u32::from(top != 0), Ordering::Relaxed);
    CALL.arg.store(0, Ordering::Relaxed);
    CALL.since_ns.store(now_ns, Ordering::Relaxed);
    CALL.slot.store(slot, Ordering::Relaxed);
    CALL.seq.store(seq.wrapping_add(2) & !1, Ordering::Release);
}

/// How many platform calls the main thread has made (published ones).
#[inline]
pub(crate) fn calls() -> u64 {
    CALL.seq.load(Ordering::Relaxed) / 2
}

/// Whether the main thread is inside a published platform call (only it writes the slot, so
/// this is exact on the main thread).
#[inline]
pub(crate) fn in_call() -> bool {
    CALL.slot.load(Ordering::Relaxed) != 0
}

/// The main thread left its platform call.
#[inline]
pub(crate) fn publish_leave() {
    CALL.slot.store(0, Ordering::Release);
}

/// The class of the running call's argument: a file extension (up to four lowercase bytes) and
/// a hash of the file's name.
#[inline]
pub(crate) fn publish_arg(ext: u32, hash: u32) {
    CALL.arg
        .store(u64::from(ext) | u64::from(hash) << 32, Ordering::Relaxed);
}

/// A consistent copy of the published state, or None between calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seen {
    pub slot: u32,
    pub since_ns: u64,
    pub ext: u32,
    pub hash: u32,
    pub depth: usize,
    pub chain: [u32; CHAIN],
}

pub(crate) fn read_call() -> Option<Seen> {
    for _ in 0..4 {
        let a = CALL.seq.load(Ordering::Acquire);
        if a & 1 == 1 {
            std::hint::spin_loop();
            continue;
        }
        let slot = CALL.slot.load(Ordering::Acquire);
        let mut chain = [0; CHAIN];
        for (c, s) in chain.iter_mut().zip(&CALL.chain) {
            *c = s.load(Ordering::Relaxed);
        }
        let arg = CALL.arg.load(Ordering::Relaxed);
        let seen = Seen {
            slot,
            since_ns: CALL.since_ns.load(Ordering::Relaxed),
            ext: arg as u32,
            hash: (arg >> 32) as u32,
            depth: (CALL.depth.load(Ordering::Relaxed) as usize).min(CHAIN),
            chain,
        };
        if CALL.seq.load(Ordering::Acquire) == a {
            return (slot != 0).then_some(seen);
        }
    }
    None
}

/// The chain of the last platform call the main thread made, whether it is still in it or not:
/// for a thread stuck in the game's own code it names where the game last came from.
pub(crate) fn stale_chain() -> ([u32; CHAIN], usize) {
    let mut chain = [0; CHAIN];
    for (c, s) in chain.iter_mut().zip(&CALL.chain) {
        *c = s.load(Ordering::Relaxed);
    }
    (
        chain,
        (CALL.depth.load(Ordering::Relaxed) as usize).min(CHAIN),
    )
}

/// A source of the running guest chain (the profiler's shadow stack): copies up to `out.len()`
/// addresses, innermost last, and says how many.
pub type StackSource = fn(&mut [u32]) -> usize;

static SOURCE: OnceLock<StackSource> = OnceLock::new();

/// Registers the source of the guest call chain for a thread that makes no platform call.
pub fn set_stack_source(f: StackSource) {
    let _ = SOURCE.set(f);
}

/// What the samples of one frame found.
#[derive(Default, Clone, Debug)]
pub struct Samples {
    pub epoch: u64,
    pub n: u32,
    pub in_guest: u32,
    /// (platform function slot, extension, name hash, innermost caller) -> samples.
    pub calls: HashMap<(u32, u32, u32, u32), u32>,
    /// Guest function address -> samples it was on the chain of.
    pub hot: HashMap<u32, u32>,
    /// The last platform call's chain at the latest in-game sample (innermost last).
    pub last_chain: Vec<u32>,
    /// The longest a platform call had run at a sample, nanoseconds.
    pub longest_call_ns: u64,
}

static SAMPLES: Mutex<Option<Samples>> = Mutex::new(None);

fn lock() -> std::sync::MutexGuard<'static, Option<Samples>> {
    SAMPLES.lock().unwrap_or_else(|p| p.into_inner())
}

/// One sample, for frame `epoch`, with the clock at `now_ns`.
pub(crate) fn sample(epoch: u64, now_ns: u64) {
    let seen = read_call();
    let mut guest = [0u32; 32];
    let mut guest_n = 0;
    if seen.is_none() {
        if let Some(f) = SOURCE.get() {
            guest_n = f(&mut guest).min(guest.len());
        }
    }
    let mut g = lock();
    let s = g.get_or_insert_with(Samples::default);
    if s.epoch != epoch {
        *s = Samples {
            epoch,
            ..Samples::default()
        };
    }
    s.n += 1;
    match seen {
        Some(c) => {
            let top = if c.depth > 0 { c.chain[c.depth - 1] } else { 0 };
            *s.calls.entry((c.slot, c.ext, c.hash, top)).or_default() += 1;
            for a in &c.chain[..c.depth] {
                *s.hot.entry(*a).or_default() += 1;
            }
            s.longest_call_ns = s.longest_call_ns.max(now_ns.saturating_sub(c.since_ns));
        }
        None => {
            s.in_guest += 1;
            let (chain, depth) = stale_chain();
            s.last_chain = chain[..depth].to_vec();
            for a in &guest[..guest_n] {
                *s.hot.entry(*a).or_default() += 1;
            }
        }
    }
}

/// The samples of frame `epoch` (taken: the next frame starts empty).
pub(crate) fn take(epoch: u64) -> Option<Samples> {
    let mut g = lock();
    match g.take() {
        Some(s) if s.epoch == epoch && s.n > 0 => Some(s),
        _ => None,
    }
}

/// The ext field as text (`nif`), empty for none.
pub fn ext_text(ext: u32) -> String {
    ext.to_le_bytes()
        .iter()
        .take_while(|b| **b != 0)
        .map(|b| *b as char)
        .collect()
}

/// A file name's extension (up to four bytes, lowercased, packed) and the FNV-1a hash of its
/// last path component, lowercased. Nothing else of the name is kept.
pub fn file_class(path: &[u8]) -> (u32, u32) {
    let name = path
        .iter()
        .rposition(|b| *b == b'\\' || *b == b'/')
        .map_or(path, |i| &path[i + 1..]);
    let mut hash: u32 = 0x811c_9dc5;
    for b in name {
        hash ^= u32::from(b.to_ascii_lowercase());
        hash = hash.wrapping_mul(0x0100_0193);
    }
    let ext = name
        .iter()
        .rposition(|b| *b == b'.')
        .map(|i| &name[i + 1..])
        .filter(|e| !e.is_empty() && e.len() <= 4 && e.iter().all(u8::is_ascii_alphanumeric))
        .map_or(0, |e| {
            let mut w = [0u8; 4];
            for (d, s) in w.iter_mut().zip(e) {
                *d = s.to_ascii_lowercase();
            }
            u32::from_le_bytes(w)
        });
    (ext, hash)
}

/// The published call and the samples are globals: every test that touches them takes a turn,
/// also the recorder's tests in lib.rs (`report_hitch` takes the samples).
#[cfg(test)]
pub(crate) fn turn() -> std::sync::MutexGuard<'static, ()> {
    static TURN: Mutex<()> = Mutex::new(());
    TURN.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_published_call_reads_back_whole() {
        let _turn = turn();
        publish_enter(0x1234, &[10, 20, 30, 0x1234], 99);
        publish_arg(0x6669_6e6e, 7);
        let s = read_call().unwrap();
        assert_eq!(s.slot, 0x1234);
        assert_eq!(s.depth, 3);
        assert_eq!(&s.chain[..3], &[10, 20, 30]);
        assert_eq!(s.since_ns, 99);
        assert_eq!(s.hash, 7);
        publish_leave();
        assert!(read_call().is_none());
    }

    #[test]
    fn only_the_innermost_callers_are_kept() {
        let _turn = turn();
        let stack: Vec<u32> = (1..=20).collect();
        publish_enter(5, &stack, 0);
        let s = read_call().unwrap();
        assert_eq!(s.depth, CHAIN);
        // The import (20) is dropped; 19 is the innermost caller, last.
        assert_eq!(s.chain[CHAIN - 1], 19);
        assert_eq!(s.chain[0], 12);
        publish_leave();
    }

    #[test]
    fn file_class_keeps_the_extension_and_a_hash_only() {
        let (ext, hash) = file_class(b"Data\\Meshes\\Weapons\\10mm.NIF");
        assert_eq!(ext_text(ext), "nif");
        let (ext2, hash2) = file_class(b"other/dir/10MM.nif");
        assert_eq!(
            (ext, hash),
            (ext2, hash2),
            "case and directory do not change it"
        );
        assert_eq!(file_class(b"noext").0, 0);
        assert_eq!(file_class(b"a.toolongext").0, 0);
    }

    #[test]
    fn samples_of_one_frame_are_kept_apart_from_the_next() {
        let _turn = turn();
        let _ = take(0);
        publish_enter(9, &[1, 2, 9], 0);
        sample(41, 5_000_000);
        sample(41, 7_000_000);
        publish_leave();
        sample(41, 8_000_000);
        let s = take(41).unwrap();
        assert_eq!((s.n, s.in_guest), (3, 1));
        assert_eq!(s.calls[&(9, 0, 0, 2)], 2);
        assert_eq!(s.hot[&1], 2);
        assert_eq!(s.longest_call_ns, 7_000_000);
        assert!(take(41).is_none());
        sample(42, 0);
        assert!(take(41).is_none());
    }
}
