// SPDX-License-Identifier: GPL-3.0-only
//! Havok's 16 ms step on its own OS thread with stepped threads (nv-rs#944): the step runs on
//! `nv-havok` when its poster waits, is given the poster's x87 control word and virtual clock, and
//! calls the same function with the same words as a step run on the poster. The inputs are made
//! here: a registered stand-in for the step, not the game's Havok or a recording. The helper is one
//! per process, so this binary holds one run.

use engine_core::bhk;
use engine_core::real_threads::on_havok_helper;
use engine_core::units::crt::{set_x87_control_word, x87_control_word};
use engine_core::{args, Engine, Ret};
use std::sync::Mutex;

engine_core::assemble!();

const STEP: u32 = 0x00c9_1b10;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Call {
    words: Vec<u32>,
    on_helper: bool,
    control_word: u16,
}

static CALLS: Mutex<Vec<Call>> = Mutex::new(Vec::new());

fn step_stand_in(e: &mut Engine, a: &[u32]) -> Ret {
    CALLS.lock().unwrap().push(Call {
        words: a.to_vec(),
        on_helper: on_havok_helper(),
        control_word: x87_control_word(e),
    });
    // What a step's clock readings would move: handed to main at the join when the step is on the
    // helper, moved on main itself when it is not.
    e.advance_clock_us(7);
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

#[test]
fn the_step_keeps_the_threads_words_and_clock_when_it_goes_on_havok() {
    let mut e = Engine::new();
    e.repeatable = true;
    set_x87_control_word(&mut e, 0x027f);
    let word = x87_control_word(&mut e);
    e.register(STEP, step_stand_in);

    let (threaded, threaded_clock) = run(&mut e, true);
    assert!(threaded[0].on_helper, "the step did not run on nv-havok");
    assert_eq!(threaded[0].control_word, word);

    let (serial, serial_clock) = run(&mut e, false);
    assert!(!serial[0].on_helper, "the inline step ran on nv-havok");
    assert_eq!(serial[0].control_word, word);
    assert_eq!(threaded_clock, serial_clock, "the virtual clock drifted");

    bhk::set_thread_on_for_test(true);
}
