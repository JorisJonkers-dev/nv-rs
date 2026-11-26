// SPDX-License-Identifier: GPL-3.0-only
//! The late AI tasks on a worker with stepped threads (nv-rs#940): the job runs on the helper when
//! its poster joins, takes the poster's x87 control word and virtual clock, and leaves the same
//! calls, guest bytes and stop stack as the inline body. The inputs are made here: registered
//! stand-ins for the tasks, not game files or a recording. The helper is one per process and belongs
//! to the run that first posts, so this binary holds one engine and one test.

use engine_core::late_ai::{self, LateAiBody};
use engine_core::real_threads::on_havok_helper;
use engine_core::units::crt::{set_x87_control_word, x87_control_word};
use engine_core::{args, Engine, Ptr, Ret};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

engine_core::assemble!();

const FIRST: u32 = 0x0096_c240;
const INNER: u32 = 0x0096_c250;
const VALUE: u32 = 0x0300_0100;
const OWNER: Ptr = Ptr::new(0x011e_0e80);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Call {
    addr: u32,
    words: Vec<u32>,
    on_helper: bool,
    control_word: u16,
}

static CALLS: Mutex<Vec<Call>> = Mutex::new(Vec::new());
static STOP_IN_BODY: AtomicBool = AtomicBool::new(false);

fn record(e: &mut Engine, addr: u32, a: &[u32]) {
    CALLS.lock().unwrap().push(Call {
        addr,
        words: a.to_vec(),
        on_helper: on_havok_helper(),
        control_word: x87_control_word(e),
    });
}

fn first(e: &mut Engine, a: &[u32]) -> Ret {
    record(e, FIRST, a);
    e.mem.set_u32(VALUE, a[0] ^ a[1]);
    // What a task's clock readings would move: handed to main at the join when it runs on the
    // helper, moved on main itself when it does not.
    e.advance_clock_us(11);
    if STOP_IN_BODY.load(Ordering::Relaxed) {
        e.call(INNER, &[]);
    }
    Ret::default()
}

fn inner(_e: &mut Engine, _a: &[u32]) -> Ret {
    panic!("stop inside the late tasks")
}

fn body(e: &mut Engine, this: Ptr) {
    e.call(FIRST, &args![this.addr(), 0x5a5a_u32]);
}

fn run(e: &mut Engine, threaded: bool) -> (Vec<Call>, u64, u32) {
    CALLS.lock().unwrap().clear();
    late_ai::set_on_for_test(threaded);
    e.clock_ms = 4;
    e.clock_us = 7;
    late_ai::update(e, OWNER, body as LateAiBody);
    let calls = CALLS.lock().unwrap().clone();
    assert_eq!(calls.len(), 1, "the late tasks ran once");
    assert!(!late_ai::posted(), "a late job was left posted");
    (calls, e.clock_peek_us(), e.mem.u32(VALUE))
}

fn stop_text(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
        .expect("the stand-in's stop text")
}

#[test]
fn the_job_keeps_the_call_order_words_clock_bytes_and_stop_of_the_inline_body() {
    let mut e = Engine::new();
    e.repeatable = true;
    e.map(VALUE, 0x2000);
    // Not the default 0x027f: the helper must take this word from the poster.
    set_x87_control_word(&mut e, 0x007f);
    let word = x87_control_word(&mut e);
    e.register(FIRST, first);
    e.register(INNER, inner);

    let (threaded, threaded_clock, threaded_bytes) = run(&mut e, true);
    assert!(threaded[0].on_helper, "the job did not run on the helper");
    assert_eq!(threaded[0].control_word, word, "the x87 word drifted");

    let (inline, inline_clock, inline_bytes) = run(&mut e, false);
    assert!(!inline[0].on_helper, "the inline body ran on the helper");
    assert_eq!(inline[0].control_word, word);
    assert_eq!(threaded[0].addr, inline[0].addr);
    assert_eq!(threaded[0].words, inline[0].words);
    assert_eq!(threaded_clock, inline_clock, "the virtual clock drifted");
    assert_eq!(threaded_bytes, inline_bytes, "the guest bytes differ");

    // A per-call diagnostic refuses the post: the call is made on the poster and recorded there.
    CALLS.lock().unwrap().clear();
    e.call_log = Some(Vec::new());
    late_ai::set_on_for_test(true);
    late_ai::update(&mut e, OWNER, body as LateAiBody);
    let logged = e
        .call_log
        .take()
        .is_some_and(|log| log.iter().any(|(addr, _)| *addr == FIRST));
    assert!(!late_ai::posted());
    assert!(logged, "the refused body was not recorded on the poster");
    let calls = CALLS.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert!(
        !calls[0].on_helper,
        "a diagnostic did not keep the body inline"
    );
    drop(calls);

    // A stop inside the body is reported at the join with the calls the helper unwound through,
    // exactly as an inline call reports them.
    STOP_IN_BODY.store(true, Ordering::Relaxed);
    let threaded = catch_unwind(AssertUnwindSafe(|| {
        late_ai::set_on_for_test(true);
        late_ai::update(&mut e, OWNER, body as LateAiBody);
    }));
    assert!(
        threaded.is_err(),
        "a stop in the posted job was not resumed"
    );
    let threaded_stack = std::mem::take(&mut e.call_stack);
    assert!(!late_ai::posted(), "a stopped job was left posted");

    let inline = catch_unwind(AssertUnwindSafe(|| {
        late_ai::set_on_for_test(false);
        late_ai::update(&mut e, OWNER, body as LateAiBody);
    }));
    assert!(inline.is_err(), "a stop in the inline body was not raised");
    let inline_stack = std::mem::take(&mut e.call_stack);
    STOP_IN_BODY.store(false, Ordering::Relaxed);

    assert_eq!(
        stop_text(threaded.unwrap_err()),
        stop_text(inline.unwrap_err()),
        "the stop text differs"
    );
    assert_eq!(threaded_stack, inline_stack, "the stop stack differs");
    assert_eq!(inline_stack, [FIRST, INNER]);
    late_ai::set_on_for_test(false);
}
