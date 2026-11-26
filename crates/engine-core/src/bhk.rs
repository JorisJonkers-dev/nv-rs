// SPDX-License-Identifier: GPL-3.0-only
//! The Havok handle seam (nv-rs#944, ADR-0014 stage 2): code outside Havok reaches a Havok object
//! only through this module. Today the wrappers are the field reads and writes the translations
//! used to make directly; moving the objects to native layouts changes these functions, not their
//! callers. Havok's own units keep using their typed fields as before.
//!
//! The step goes through [`step`]. With stepped threads it runs on the `nv-havok` OS thread beside
//! the game thread that asked for it; the poster waits at once, so the step keeps its place in the
//! run, its words, its x87 control word and its virtual clock. With real threads, diagnostics or a
//! refused post it runs on the calling thread, as before (the race campaign for Havok's own real
//! threads is in `docs/fork/HAVOK-THREAD.md`).

use crate::{real_threads, Engine, Field, Inline, Ptr, Scalar};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;

/// The engine a wrapper reads through. Translated functions hold `&mut Engine`, tests own one;
/// both pass `&e` to [`get`] and `&mut e` to [`set`].
pub trait EngineRef {
    fn as_engine(&self) -> &Engine;
}

impl EngineRef for Engine {
    fn as_engine(&self) -> &Engine {
        self
    }
}

impl EngineRef for &Engine {
    fn as_engine(&self) -> &Engine {
        self
    }
}

impl EngineRef for &mut Engine {
    fn as_engine(&self) -> &Engine {
        self
    }
}

/// The engine a wrapper writes through.
pub trait EngineMut {
    fn as_engine_mut(&mut self) -> &mut Engine;
}

impl EngineMut for Engine {
    fn as_engine_mut(&mut self) -> &mut Engine {
        self
    }
}

impl EngineMut for &mut Engine {
    fn as_engine_mut(&mut self) -> &mut Engine {
        self
    }
}

/// A Havok field read, on the caller's behalf.
pub fn get<S, T: Scalar, E: EngineRef>(e: &E, p: Ptr<S>, f: Field<S, T>) -> T {
    e.as_engine().get(p, f)
}

/// A Havok field write, on the caller's behalf.
pub fn set<S, T: Scalar, E: EngineMut>(e: &mut E, p: Ptr<S>, f: Field<S, T>, v: T) {
    e.as_engine_mut().set(p, f, v)
}

/// The address of a Havok object's embedded field.
pub fn at<S, U>(p: Ptr<S>, f: Field<S, Inline<U>>) -> Ptr<U> {
    p.at(f)
}

/// A word inside a Havok object, for the SDK's array and map constructors.
pub fn byte_add<S>(p: Ptr<S>, off: u32) -> Ptr<()> {
    p.byte_add(off)
}

/// Whether a stepped run gives Havok's step a thread of its own. `NV_HAVOK_THREAD=0` keeps the
/// step on the calling thread for an A/B comparison; the default is on. Read once, before the
/// first frame.
static THREAD_ON: AtomicBool = AtomicBool::new(true);
static THREAD_READ: Once = Once::new();

pub fn thread_on() -> bool {
    THREAD_READ.call_once(|| {
        THREAD_ON.store(
            std::env::var("NV_HAVOK_THREAD").map_or(true, |v| v.trim() != "0"),
            Ordering::Relaxed,
        );
    });
    THREAD_ON.load(Ordering::Relaxed)
}

/// Selects the path for a test run (one run per test binary).
pub fn set_thread_on_for_test(on: bool) {
    THREAD_READ.call_once(|| {});
    THREAD_ON.store(on, Ordering::Relaxed);
}

/// Whether a step is posted to `nv-havok` and not joined yet.
pub fn havok_posted() -> bool {
    real_threads::havok_posted()
}

/// Runs one Havok step at `addr` with `words`, on `nv-havok` when the run allows it. The poster
/// joins before returning: no other game thread can run between the step's post and its place in
/// the call order. A helper that refuses the job leaves the call on the poster.
pub fn step(e: &mut Engine, addr: u32, words: &[u32]) {
    if !thread_on() || e.real.is_some() || !real_threads::havok_post_ready(e) {
        e.call(addr, words);
        return;
    }
    let job_words = words.to_vec();
    let posted = real_threads::post_havok(
        e,
        Box::new(move |w| {
            w.call(addr, &job_words);
        }),
    );
    if posted {
        real_threads::join_havok(e);
    } else {
        e.call(addr, words);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout;

    layout! {
        struct Inner: 0x04 {
            0x00 word: u32,
        }
        struct FieldHolder: 0x10 {
            0x00 word: u32,
            0x04 nested: Inline<Inner>,
        }
    }

    #[test]
    fn the_wrappers_read_and_write_the_same_bytes() {
        let mut e = Engine::new();
        let object: Ptr<FieldHolder> = e.new_object();
        set(&mut e, object, FieldHolder::word, 0x1234_5678);
        set(&mut e, at(object, FieldHolder::nested), Inner::word, 9);
        assert_eq!(get(&e, object, FieldHolder::word), 0x1234_5678);
        assert_eq!(get(&e, at(object, FieldHolder::nested), Inner::word), 9);
        assert_eq!(e.mem.u32(object.addr()), 0x1234_5678);
        assert_eq!(e.mem.u32(object.addr() + 4), 9);
        assert_eq!(byte_add(object, 8).addr(), object.addr() + 8);
    }
}
