// SPDX-License-Identifier: GPL-3.0-only
//! The model loader's per-frame work and the garbage collection pass as jobs with stepped threads
//! (nv-rs#941): each is posted to the stepped component helper (the one Havok's step uses) and its
//! poster joins before the next game call, so the calls, their words, the guest locks and the
//! uploads they finish keep their place in the run. A stop in a job unwinds at the join with the
//! same call stack, and a per-call
//! diagnostic keeps the call on the poster. The inputs are made here: stand-ins for the three
//! frame calls and scratch memory from the test engine, not game files or a recording. One helper
//! per process, so this binary holds one run in one test.

use engine_core::bhk;
use engine_core::loader_jobs;
use engine_core::real_threads::{join_loader, on_loader_helper, post_loader};
use engine_core::units::crt::{set_x87_control_word, x87_control_word};
use engine_core::{args, Engine, Ret};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

engine_core::assemble!();

const UPDATE_QUEUE: u32 = 0x00c3_dbf0;
const GC_UPDATE: u32 = 0x0086_8850;
const GC_CLEAR: u32 = 0x0086_8d10;
const UPLOAD: u32 = 0x00c9_1001;
const DRAW: u32 = 0x00c9_1002;
const INNER: u32 = 0x00c9_1003;
const STEP: u32 = 0x00c9_1004;

static STOP_IN_JOB: AtomicBool = AtomicBool::new(false);
static STORE: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Call {
    addr: u32,
    words: Vec<u32>,
    on_helper: bool,
    control_word: u16,
    thread: std::thread::ThreadId,
}

static LOG: Mutex<Vec<Call>> = Mutex::new(Vec::new());

fn record(e: &mut Engine, addr: u32, a: &[u32]) {
    LOG.lock().unwrap().push(Call {
        addr,
        words: a.to_vec(),
        on_helper: on_loader_helper(),
        control_word: x87_control_word(e),
        thread: std::thread::current().id(),
    });
}

/// A f64 local rounded at the f32 store (ADR-0014), so the job's write can be compared byte for
/// byte with an inline one. The job itself only forwards words; no float arithmetic lives there.
fn update_stand_in(e: &mut Engine, a: &[u32]) -> Ret {
    record(e, UPDATE_QUEUE, a);
    let wide = f64::from_bits(0x3ff9_9999_8000_0000);
    e.mem.set_f32(STORE.load(Ordering::Relaxed), wide as f32);
    e.call(UPLOAD, &args![]);
    if STOP_IN_JOB.load(Ordering::Relaxed) {
        e.call(INNER, &[]);
    }
    Ret::default()
}

fn upload_stand_in(e: &mut Engine, a: &[u32]) -> Ret {
    record(e, UPLOAD, a);
    Ret::default()
}

fn gc_update_stand_in(e: &mut Engine, a: &[u32]) -> Ret {
    record(e, GC_UPDATE, a);
    Ret::default()
}

fn gc_clear_stand_in(e: &mut Engine, a: &[u32]) -> Ret {
    record(e, GC_CLEAR, a);
    Ret::default()
}

fn draw_stand_in(e: &mut Engine, a: &[u32]) -> Ret {
    record(e, DRAW, a);
    Ret::default()
}

fn step_stand_in(e: &mut Engine, a: &[u32]) -> Ret {
    record(e, STEP, a);
    Ret::default()
}

fn stop_inner(_e: &mut Engine, _a: &[u32]) -> Ret {
    panic!("stop inside a loader job")
}

fn frame(e: &mut Engine) {
    loader_jobs::update_queue(e, 0x1234_5678);
    loader_jobs::garbage_collector_update(e);
    loader_jobs::garbage_collector_clear_temp_effects(e);
}

fn log() -> Vec<Call> {
    LOG.lock().unwrap().clone()
}

fn stop_text(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
        .expect("the stand-in's stop text")
}

#[test]
fn the_loader_and_gc_jobs_keep_the_frame_order_locks_and_stores() {
    let mut e = Engine::new();
    e.repeatable = true;
    STORE.store(e.mem.alloc(4), Ordering::Relaxed);
    set_x87_control_word(&mut e, 0x007f);
    let word = x87_control_word(&mut e);
    e.register(UPDATE_QUEUE, update_stand_in);
    e.register(UPLOAD, upload_stand_in);
    e.register(GC_UPDATE, gc_update_stand_in);
    e.register(GC_CLEAR, gc_clear_stand_in);
    e.register(DRAW, draw_stand_in);
    e.register(STEP, step_stand_in);
    e.register(INNER, stop_inner);

    // Stepped jobs: the three calls and the upload they finish run on the helper, in the frame's
    // order, with the poster's control word and a f64 local rounded at its f32 store.
    loader_jobs::set_jobs_on_for_test(true);
    LOG.lock().unwrap().clear();
    frame(&mut e);
    let threaded = log();
    let loader_thread = threaded[0].thread;
    let poster_thread = std::thread::current().id();
    assert!(
        threaded.iter().all(|c| c.thread == loader_thread) && loader_thread != poster_thread,
        "the frame calls did not run on one helper OS thread other than the poster's"
    );
    let threaded_store = e.mem.u32(STORE.load(Ordering::Relaxed));
    assert!(!loader_jobs::loader_posted(), "a job was left posted");
    assert_eq!(
        threaded.iter().map(|c| c.addr).collect::<Vec<_>>(),
        [UPDATE_QUEUE, UPLOAD, GC_UPDATE, GC_CLEAR]
    );
    assert!(
        threaded.iter().all(|c| c.on_helper),
        "a frame call did not run on the component helper"
    );
    assert!(threaded.iter().all(|c| c.control_word == word));
    assert_eq!(threaded[0].words, [0x1234_5678u32]);

    // The reference path: the same calls, words, control word and bytes on the poster.
    loader_jobs::set_jobs_on_for_test(false);
    LOG.lock().unwrap().clear();
    frame(&mut e);
    let inline = log();
    assert!(!loader_jobs::loader_posted(), "a job was left posted");
    assert!(
        inline.iter().all(|c| !c.on_helper),
        "an inline call ran on the component helper"
    );
    let without_thread = |calls: &[Call]| {
        calls
            .iter()
            .map(|c| (c.addr, c.words.clone(), c.control_word))
            .collect::<Vec<_>>()
    };
    assert_eq!(without_thread(&inline), without_thread(&threaded));
    assert_eq!(
        e.mem.u32(STORE.load(Ordering::Relaxed)),
        threaded_store,
        "the rounded f32 store differs between the two paths"
    );

    // The join ends the upload before the frame's draw; nothing is still posted at the draw.
    loader_jobs::set_jobs_on_for_test(true);
    LOG.lock().unwrap().clear();
    loader_jobs::update_queue(&mut e, 0x1234_5678);
    assert!(!loader_jobs::loader_posted());
    e.call(DRAW, &[]);
    assert_eq!(
        log().iter().map(|c| c.addr).collect::<Vec<_>>(),
        [UPDATE_QUEUE, UPLOAD, DRAW]
    );

    // The post/join order: a job already posted keeps the helper, so the next frame call stays on
    // the poster and runs before the posted job, which runs only when its poster joins.
    LOG.lock().unwrap().clear();
    assert!(post_loader(
        &mut e,
        Box::new(|w| {
            w.call(DRAW, &[]);
        })
    ));
    assert!(loader_jobs::loader_posted());
    loader_jobs::update_queue(&mut e, 0x1234_5678);
    let busy = log();
    assert_eq!(
        busy.iter().map(|c| c.addr).collect::<Vec<_>>(),
        [UPDATE_QUEUE, UPLOAD],
        "the posted job ran before its join, or the busy helper took the call"
    );
    assert!(busy
        .iter()
        .all(|c| !c.on_helper && c.thread == poster_thread));
    assert!(join_loader(&mut e));
    let joined = log();
    assert_eq!(joined.last().map(|c| c.addr), Some(DRAW));
    assert!(joined
        .last()
        .is_some_and(|c| c.on_helper && c.thread == loader_thread));
    assert!(!loader_jobs::loader_posted());
    assert!(!join_loader(&mut e), "a second join found a job");

    // A per-call diagnostic refuses the post: the call stays on the poster and is recorded there.
    e.call_log = Some(Vec::new());
    LOG.lock().unwrap().clear();
    loader_jobs::update_queue(&mut e, 0x1234_5678);
    assert!(log().iter().all(|c| !c.on_helper));
    assert!(e
        .call_log
        .as_ref()
        .is_some_and(|l| l.iter().any(|(a, _)| *a == UPDATE_QUEUE)));
    e.call_log = None;

    // A stop in the job is reported at the join with the calls the helper unwound through,
    // exactly as an inline call reports them: same text, same call stack.
    STOP_IN_JOB.store(true, Ordering::Relaxed);
    let threaded = catch_unwind(AssertUnwindSafe(|| {
        loader_jobs::update_queue(&mut e, 0x1234_5678)
    }));
    assert!(
        threaded.is_err(),
        "a stop in the job was not resumed at the join"
    );
    let threaded_stack = std::mem::take(&mut e.call_stack);
    assert!(
        !loader_jobs::loader_posted(),
        "a stopped job was left posted"
    );

    loader_jobs::set_jobs_on_for_test(false);
    let inline = catch_unwind(AssertUnwindSafe(|| {
        loader_jobs::update_queue(&mut e, 0x1234_5678)
    }));
    assert!(inline.is_err(), "a stop in the inline call was not raised");
    let inline_stack = std::mem::take(&mut e.call_stack);

    STOP_IN_JOB.store(false, Ordering::Relaxed);
    loader_jobs::set_jobs_on_for_test(true);
    assert_eq!(
        stop_text(threaded.unwrap_err()),
        stop_text(inline.unwrap_err())
    );
    assert_eq!(threaded_stack, inline_stack);
    assert_eq!(inline_stack, [UPDATE_QUEUE, INNER]);

    // One stepped component helper serves the loader and Havok's step. A loader thread of its own
    // would need a helper slot of its own.
    LOG.lock().unwrap().clear();
    bhk::step(&mut e, STEP, &[]);
    let step = log();
    assert_eq!(step.iter().map(|c| c.addr).collect::<Vec<_>>(), [STEP]);
    assert!(step[0].on_helper, "the Havok step did not run on a helper");
    assert_eq!(
        step[0].thread, loader_thread,
        "the loader and Havok steps ran on different helper threads"
    );
}
