// SPDX-License-Identifier: GPL-3.0-only
//! Havok's 16 ms step on its own OS thread with stepped threads (nv-rs#944): the step runs on
//! `nv-havok` when its poster waits, is given the poster's x87 control word and virtual clock, and
//! calls the same function with the same words as a step run on the poster. A stop inside the step
//! unwinds at the join with the same stopped calls as an inline step, and a per-call diagnostic
//! keeps the step on the poster. The inputs are made here: a registered stand-in for the step, not
//! the game's Havok or a recording. The helper is one per process, so this binary holds one run.

use engine_core::bhk;
use engine_core::real_threads::on_havok_helper;
use engine_core::units::crt::{set_x87_control_word, x87_control_word};
use engine_core::{args, Engine, Ret};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

engine_core::assemble!();

const STEP: u32 = 0x00c9_1b10;
const INNER: u32 = 0x00c9_1b20;
/// Makes the stand-in call `INNER`, which stops, so the stop path can be compared with an inline
/// one. The helper's function table is a copy made at its first post, so both functions are
/// registered before any run.
static STOP_IN_STEP: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Call {
    words: Vec<u32>,
    on_helper: bool,
    control_word: u16,
}

static CALLS: Mutex<Vec<Call>> = Mutex::new(Vec::new());

fn stop_inner(_e: &mut Engine, _a: &[u32]) -> Ret {
    panic!("stop inside the step")
}

fn step_stand_in(e: &mut Engine, a: &[u32]) -> Ret {
    CALLS.lock().unwrap().push(Call {
        words: a.to_vec(),
        on_helper: on_havok_helper(),
        control_word: x87_control_word(e),
    });
    if STOP_IN_STEP.load(Ordering::Relaxed) {
        e.call(INNER, &[]);
    } else {
        // What a step's clock readings would move: handed to main at the join when the step is on
        // the helper, moved on main itself when it is not.
        e.advance_clock_us(7);
    }
    Ret::default()
}

fn run(e: &mut Engine, on_thread: bool) -> (Vec<Call>, u64) {
    CALLS.lock().unwrap().clear();
    bhk::set_thread_on_for_test(on_thread);
    e.clock_ms = 5;
    e.clock_us = 3;
    let words = args![0x1234_5678u32, 1.5f32];
    bhk::step(e, STEP, &words);
    let calls = CALLS.lock().unwrap().clone();
    assert_eq!(calls.len(), 1, "the step ran once");
    assert!(!bhk::havok_posted(), "a step was left posted");
    assert_eq!(calls[0].words, words.as_ref());
    (calls, e.clock_peek_us())
}

fn stop_text(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
        .expect("the stand-in's stop text")
}

#[test]
fn the_step_keeps_the_threads_words_and_clock_when_it_goes_on_havok() {
    let mut e = Engine::new();
    e.repeatable = true;
    // Not the default 0x027f: the helper must take this word from the poster, not from a new
    // thread's initial state.
    set_x87_control_word(&mut e, 0x007f);
    let word = x87_control_word(&mut e);
    assert_eq!(word, 0x007f);
    e.register(STEP, step_stand_in);
    e.register(INNER, stop_inner);

    let (threaded, threaded_clock) = run(&mut e, true);
    assert!(threaded[0].on_helper, "the step did not run on nv-havok");
    assert_eq!(threaded[0].control_word, word);

    let (serial, serial_clock) = run(&mut e, false);
    assert!(!serial[0].on_helper, "the inline step ran on nv-havok");
    assert_eq!(serial[0].control_word, word);
    assert_eq!(threaded_clock, serial_clock, "the virtual clock drifted");

    // A per-call diagnostic refuses the post: no job is left, the step runs on the poster and the
    // call is recorded there.
    e.call_log = Some(Vec::new());
    let (refused, _) = run(&mut e, true);
    assert!(
        !refused[0].on_helper,
        "a diagnostic did not keep the step inline"
    );
    assert!(
        e.call_log
            .as_ref()
            .is_some_and(|log| log.iter().any(|(addr, _)| *addr == STEP)),
        "the refused step was not recorded on the poster"
    );
    e.call_log = None;

    // A stop inside the step is reported at the join with the calls the helper unwound through,
    // exactly as an inline call reports them: same text, same call stack.
    STOP_IN_STEP.store(true, Ordering::Relaxed);
    let words = args![0x1234_5678u32, 1.5f32];
    let stopped = catch_unwind(AssertUnwindSafe(|| {
        bhk::set_thread_on_for_test(true);
        bhk::step(&mut e, STEP, &words);
    }));
    assert!(
        stopped.is_err(),
        "a stop in the posted step was not resumed"
    );
    let threaded_stack = std::mem::take(&mut e.call_stack);
    assert!(!bhk::havok_posted(), "a stopped step was left posted");

    let stopped_inline = catch_unwind(AssertUnwindSafe(|| {
        bhk::set_thread_on_for_test(false);
        bhk::step(&mut e, STEP, &words);
    }));
    assert!(
        stopped_inline.is_err(),
        "a stop in the inline step was not raised"
    );
    let inline_stack = std::mem::take(&mut e.call_stack);
    STOP_IN_STEP.store(false, Ordering::Relaxed);

    assert_eq!(
        stop_text(stopped.unwrap_err()),
        stop_text(stopped_inline.unwrap_err()),
        "the stop's text differs between the two paths"
    );
    assert_eq!(
        threaded_stack, inline_stack,
        "the stop's call stack differs between the two paths"
    );
    assert_eq!(inline_stack, [STEP, INNER]);

    bhk::set_thread_on_for_test(true);
}
