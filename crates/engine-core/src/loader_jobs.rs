// SPDX-License-Identifier: GPL-3.0-only
//! The model loader's per-frame work and the garbage collection pass as jobs (nv-rs#941): the main
//! loop's three calls go through here. With stepped threads and the switch on, each is posted to
//! `nv-loader` and joined before the next game call, so the call order, the guest locks (the
//! garbage collector's critical section and the model loader's lock), the x87 control word and the
//! virtual clock are the same as in an inline call. No game thread runs between a post and its
//! join: the join ends the load's uploads before the draw that needs them.
//!
//! Real threads run the calls inline: ADR-0015 keeps a component's real threads off by default
//! until its race campaign passes. `NV_LOADER_JOBS=1` turns the stepped helper on for an A/B run;
//! the default is off, so recordings keep the reference path. The job body forwards the same words
//! to the same translated function and adds no float arithmetic; typed code moved here later must
//! keep ADR-0014's f64 locals rounded at each f32 store.

use crate::{real_threads, Engine};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;

/// IOManager::UpdateQueue: finishes queued loads, including the model loader's walk.
const UPDATE_QUEUE: u32 = 0x00c3_dbf0;
/// GarbageCollector::Update.
const GC_UPDATE: u32 = 0x0086_8850;
/// GarbageCollector::ClearTempEffects.
const GC_CLEAR_TEMP_EFFECTS: u32 = 0x0086_8d10;

/// Off until the race campaign for these jobs passes (ADR-0015). Read once, before the first frame.
static JOBS_ON: AtomicBool = AtomicBool::new(false);
static JOBS_READ: Once = Once::new();

/// Whether a stepped run gives the loader and the collector a thread of their own.
pub fn jobs_on() -> bool {
    JOBS_READ.call_once(|| {
        JOBS_ON.store(
            std::env::var("NV_LOADER_JOBS").is_ok_and(|v| matches!(v.trim(), "1" | "on")),
            Ordering::Relaxed,
        );
    });
    JOBS_ON.load(Ordering::Relaxed)
}

/// Selects the path for a test run (one run per test binary). Test-only: it changes a process-wide
/// flag and races with a run already in progress; never call it from game code.
#[doc(hidden)]
pub fn set_jobs_on_for_test(on: bool) {
    JOBS_READ.call_once(|| {});
    JOBS_ON.store(on, Ordering::Relaxed);
}

/// Whether a job is posted to `nv-loader` and not joined yet.
pub fn loader_posted() -> bool {
    real_threads::loader_posted()
}

/// Runs one of the frame calls, on `nv-loader` when the run allows it. The poster joins before
/// returning: no other game thread can run between the post and the call's place in the call order.
/// A helper that refuses the job leaves the call on the poster.
fn run(e: &mut Engine, addr: u32, words: &[u32]) {
    if !jobs_on() || e.real.is_some() || !real_threads::loader_post_ready(e) {
        e.call(addr, words);
        return;
    }
    let job_words = words.to_vec();
    let posted = real_threads::post_loader(
        e,
        Box::new(move |w| {
            w.call(addr, &job_words);
        }),
    );
    if posted {
        real_threads::join_loader(e);
    } else {
        e.call(addr, words);
    }
}

/// The per-frame IOManager::UpdateQueue call, with `manager` as its one word.
pub fn update_queue(e: &mut Engine, manager: u32) {
    run(e, UPDATE_QUEUE, &[manager]);
}

/// The frame's GarbageCollector::Update call.
pub fn garbage_collector_update(e: &mut Engine) {
    run(e, GC_UPDATE, &[]);
}

/// The frame's GarbageCollector::ClearTempEffects call.
pub fn garbage_collector_clear_temp_effects(e: &mut Engine) {
    run(e, GC_CLEAR_TEMP_EFFECTS, &[]);
}
