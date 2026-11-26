// SPDX-License-Identifier: GPL-3.0-only
//! The late AI tasks on a worker with real threads (nv-rs#940): with `NV_LATE_AI=1` the body runs on
//! a helper while main waits, and its writes are visible when the call returns. A diagnostic keeps
//! the body on the caller. The stand-ins and guest words are made here; one run per process, so this
//! binary holds one run.

use engine_core::late_ai::{self, LateAiBody};
use engine_core::real_threads;
use engine_core::units::crt::x87_control_word;
use engine_core::{args, Engine, Ptr, Ret};
use std::sync::{Mutex, OnceLock};

engine_core::assemble!();

const TASK: u32 = 0x0096_c240;
const VALUE: u32 = 0x0300_0200;
const OWNER: Ptr = Ptr::new(0x011e_0e80);

/// The test runner's thread name, captured on this thread; the pool's helpers have their own.
static CALLER: OnceLock<String> = OnceLock::new();

#[derive(Clone, Debug, PartialEq, Eq)]
struct Call {
    thread: String,
    on_caller: bool,
    words: Vec<u32>,
    control_word: u16,
}

static CALLS: Mutex<Vec<Call>> = Mutex::new(Vec::new());

fn task(e: &mut Engine, a: &[u32]) -> Ret {
    let thread = std::thread::current()
        .name()
        .unwrap_or("unnamed")
        .to_string();
    CALLS.lock().unwrap().push(Call {
        on_caller: Some(&thread) == CALLER.get(),
        thread,
        words: a.to_vec(),
        control_word: x87_control_word(e),
    });
    e.mem.set_u32(VALUE, a[0]);
    Ret::default()
}

fn body(e: &mut Engine, this: Ptr) {
    e.call(TASK, &args![this.addr(), 1.5f32]);
}

fn run(e: &mut Engine) -> Vec<Call> {
    CALLS.lock().unwrap().clear();
    late_ai::update(e, OWNER, body as LateAiBody);
    let calls = CALLS.lock().unwrap().clone();
    assert_eq!(calls.len(), 1, "the late tasks ran once");
    assert!(!late_ai::posted(), "a late job was left posted");
    calls
}

#[test]
fn the_job_runs_on_a_helper_with_real_threads_and_keeps_its_words() {
    CALLER
        .set(
            std::thread::current()
                .name()
                .unwrap_or("unnamed")
                .to_string(),
        )
        .expect("set once");
    let mut e = Engine::new();
    e.start_real_clock();
    e.map(VALUE, 0x3000);
    e.register(TASK, task);
    let word = x87_control_word(&mut e);
    real_threads::start(&mut e).expect("real threads on");

    assert!(!late_ai::on(), "NV_LATE_AI turned the worker on by default");
    let inline = run(&mut e);
    assert!(inline[0].on_caller, "{:?}", inline[0]);

    late_ai::set_on_for_test(true);
    let threaded = run(&mut e);
    assert!(!threaded[0].on_caller, "{:?}", threaded[0]);
    assert!(
        threaded[0].thread.starts_with("render helper"),
        "{:?}",
        threaded[0]
    );
    assert_eq!(threaded[0].words, inline[0].words);
    assert_eq!(threaded[0].control_word, word, "the x87 word drifted");
    assert_eq!(e.mem.u32(VALUE), OWNER.addr());

    // A per-call diagnostic keeps the body on the caller, as it does with stepped threads.
    e.call_log = Some(Vec::new());
    let refused = run(&mut e);
    assert!(refused[0].on_caller, "{:?}", refused[0]);
    e.call_log = None;
    late_ai::set_on_for_test(false);
}
