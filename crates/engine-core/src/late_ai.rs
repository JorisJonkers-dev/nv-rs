// SPDX-License-Identifier: GPL-3.0-only
//! The late AI tasks on a worker (nv-rs#940): `Main::UpdateNonRenderSafeAITasks` (`0086f6a0`) runs
//! its calls in the frame tail, after the AI threads have been joined. [`update`] runs that body on
//! a helper when `NV_LATE_AI=1`, keeping the body's call order and joining before it returns, so the
//! code that follows still sees the same writes at the same place in the run.
//!
//! With stepped threads the job is posted to the existing stepped helper beside the game thread
//! (`real_threads::post_havok`, the one the Havok step uses between frames), which takes the poster's
//! thread identity, x87 control word and virtual clock and joins at once: a stepped
//! recording takes the same calls in the same order as an inline body. With real threads the job
//! runs on one helper of the existing pool while main waits, so main's own time in the tasks goes to
//! the helper. A diagnostic, a typed family running dual, a helper already busy and memory that
//! cannot be shared all leave the body on the caller. With real threads the shared pool must have no
//! other fork in flight; this call sits in the frame tail, where it does not. The switch is off in
//! every preset: real threads stay off for this change until the race campaign (ADR-0015) passes.
//!
//! The default path is one relaxed load and the inline call: no switch state is made until the first
//! call and no job, lock or allocation is involved. The helper takes no guest heap block; its TLS and
//! stack are in `HELPER_AREA`, so a run's heap layout is the same with and without the worker. The
//! body calls the process family's after-draw functions. That family is marked `Threads::Main`
//! because its level passes do one `f32` subtraction; this body does not call those passes. The
//! functions it does call move floats as words without arithmetic, and the bone update it reaches
//! already runs on helper engines under `NV_PAR_BONES`. The seam adds no `f64` local and no float
//! store: a task's `float` argument is passed on as the same 32-bit word.

use crate::{real_threads, Engine, Ptr};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Once;

/// The body of `Main::UpdateNonRenderSafeAITasks`: the original's calls in their original order.
pub type LateAiBody = fn(&mut Engine, Ptr);

/// `NV_LATE_AI` read once, before the first frame: off unless it names the worker.
static ON: AtomicBool = AtomicBool::new(false);
static READ: Once = Once::new();

const OFF: u8 = 0;
const ON_SWITCH: u8 = 1;

fn mode() -> u8 {
    READ.call_once(|| {
        let m = match std::env::var("NV_LATE_AI").as_deref() {
            Ok("1") | Ok("on") => ON_SWITCH,
            _ => OFF,
        };
        ON.store(m == ON_SWITCH, Ordering::Relaxed);
    });
    if ON.load(Ordering::Relaxed) {
        ON_SWITCH
    } else {
        OFF
    }
}

/// Whether the late AI tasks take a worker. Read once from `NV_LATE_AI`; off by default.
pub fn on() -> bool {
    mode() == ON_SWITCH
}

/// Sets the switch for a test that cannot set the environment before the first call. Test only:
/// it changes process-wide state and must not be used while a run is in progress.
#[doc(hidden)]
pub fn set_on_for_test(value: bool) {
    READ.call_once(|| {});
    ON.store(value, Ordering::Relaxed);
}

/// Whether the stepped helper holds a job. A late task uses that helper one at a time and joins
/// before returning, so false here means no late job is left posted.
pub fn posted() -> bool {
    real_threads::havok_posted()
}

/// Runs `body` with `this`, on a worker when the switch allows it and on the caller otherwise.
pub fn update(e: &mut Engine, this: Ptr, body: LateAiBody) {
    if !on() {
        body(e, this);
        return;
    }
    if e.real.is_none() {
        // The stepped helper runs with this thread's identity, control word and clock, and starts
        // only at the join, so the body keeps its place among the game thread's calls.
        let job: real_threads::HavokJob = Box::new(move |w| body(w, this));
        if real_threads::post_havok(e, job) {
            real_threads::join_havok(e);
        } else {
            body(e, this);
        }
        return;
    }
    // One helper of the pool takes the whole body; main's part (number 0) is empty. `fork_join`
    // runs no game code on main until the helper has finished, so the tail's later calls see the
    // body's writes. A refusal leaves every call on main.
    let ran = AtomicU8::new(0);
    let job = |w: &mut Engine, number: usize| {
        if number != 0 {
            body(w, this);
            ran.store(1, Ordering::Relaxed);
        }
    };
    if real_threads::fork_join(e, 1, &job) && ran.load(Ordering::Relaxed) != 0 {
        return;
    }
    body(e, this);
}
