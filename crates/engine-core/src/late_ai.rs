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
//! the helper. A diagnostic, a typed family running dual, a pool with a fork in flight, a stepped
//! helper already holding a job, a join that finds no job (the body then runs inline) and memory
//! that cannot be shared all leave the body on the caller, as does `NV_PAR_BONES` on: the bone
//! pass's subtrees that must run on main are replayed on the thread that called the pass. The
//! real-thread helper has its own thread id and TLS (not main's), a known difference from the
//! original that nothing in the port reads today. The switch is off in
//! every preset: real threads stay off for this change until the race campaign (ADR-0015) passes.
//!
//! The default path is one relaxed load and the inline call: no switch state is made until the first
//! call and no job, lock or allocation is involved. The helper takes no guest heap block; its TLS and
//! stack are in `HELPER_AREA`, so a run's heap layout is the same with and without the worker. The
//! body calls the process family's after-draw functions. That family is marked `Threads::Main`
//! because its level passes do one `f32` subtraction; this body does not call those passes. The
//! functions it does call move floats as words without arithmetic, and the bone update it reaches
//! is serial here (`NV_PAR_BONES` keeps the body on main). The seam adds no `f64` local and no float
//! store: a task's `float` argument is passed on as the same 32-bit word.

use crate::{real_threads, Engine, Ptr};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::Once;

/// The body of `Main::UpdateNonRenderSafeAITasks`: the original's calls in their original order.
pub type LateAiBody = fn(&mut Engine, Ptr);

/// `NV_LATE_AI` read once, before the first frame: off unless it names the worker.
static ON: AtomicBool = AtomicBool::new(false);
/// The late tasks stay on the caller: `NV_PAR_BONES` is on, and the bone pass's "runs on main"
/// subtrees (a head's face morphing, a world bound's allocation in the serial order) are replayed
/// on the thread that called the pass, which has to be main.
static BONES_ON_MAIN: AtomicBool = AtomicBool::new(false);
/// Late jobs in flight on either path.
static IN_FLIGHT: AtomicU8 = AtomicU8::new(0);
/// Stepped posts the helper refused (a diagnostic, a held job, memory that cannot be shared).
static REFUSED: AtomicU32 = AtomicU32::new(0);
/// The helpers a pool made by this seam gets: what the parallel culling and the bones pass ask for
/// first, so the seam never fixes the process-wide pool smaller than they need.
const POOL_HELPERS: usize = 3;
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
        let bones = std::env::var("NV_PAR_BONES").unwrap_or_default();
        BONES_ON_MAIN.store(
            matches!(bones.trim(), "on" | "1" | "check"),
            Ordering::Relaxed,
        );
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

/// Sets whether the bones pass is parallel for a test (as `NV_PAR_BONES` would). Test only.
#[doc(hidden)]
pub fn set_bones_parallel_for_test(value: bool) {
    READ.call_once(|| {});
    BONES_ON_MAIN.store(value, Ordering::Relaxed);
}

/// Sets the switch for a test that cannot set the environment before the first call. Test only:
/// it changes process-wide state and must not be used while a run is in progress.
#[doc(hidden)]
pub fn set_on_for_test(value: bool) {
    READ.call_once(|| {});
    ON.store(value, Ordering::Relaxed);
}

/// Whether a late job is in flight: the seam's own count (either path) or a job left in the stepped
/// helper. A late task joins before `update` returns, so false after it means none is left.
pub fn posted() -> bool {
    IN_FLIGHT.load(Ordering::SeqCst) != 0 || real_threads::havok_posted()
}

/// How many stepped posts the helper has refused, for tests that must tell a refusal from the
/// switch being off.
pub fn refused() -> u32 {
    REFUSED.load(Ordering::Relaxed)
}

/// Counts a job as in flight until dropped.
struct InFlight;
impl InFlight {
    fn new() -> Self {
        IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
        InFlight
    }
}
impl Drop for InFlight {
    fn drop(&mut self) {
        IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Runs `body` with `this`, on a worker when the switch allows it and on the caller otherwise.
pub fn update(e: &mut Engine, this: Ptr, body: LateAiBody) {
    if !on() || BONES_ON_MAIN.load(Ordering::Relaxed) {
        body(e, this);
        return;
    }
    let _in_flight = InFlight::new();
    if e.real.is_none() {
        // The stepped helper runs with this thread's identity, control word and clock, and starts
        // only at the join, so the body keeps its place among the game thread's calls.
        let ran = std::sync::Arc::new(AtomicBool::new(false));
        let flag = ran.clone();
        let job: real_threads::HavokJob = Box::new(move |w| {
            flag.store(true, Ordering::SeqCst);
            body(w, this)
        });
        if real_threads::post_havok(e, job) {
            let joined = real_threads::join_havok(e);
            finish_stepped(e, this, body, joined, &ran);
        } else {
            REFUSED.fetch_add(1, Ordering::Relaxed);
            body(e, this);
        }
        return;
    }
    // One helper of the pool takes the whole body; main's part (number 0) is empty. `fork_join`
    // runs no game code on main until the helper has finished, so the tail's later calls see the
    // body's writes. The pool is one caller at a time: a fork in flight (this call made from inside
    // a job) leaves the body on main. A pool that does not exist yet is made with the helpers the
    // parallel culling and the bones pass ask for, not with one; an existing pool runs this job on
    // its first helper. A refusal leaves every call on main.
    if real_threads::pool_busy() {
        body(e, this);
        return;
    }
    let helpers = if real_threads::pool_made() {
        1
    } else {
        POOL_HELPERS
    };
    let job = |w: &mut Engine, number: usize| {
        if number == 1 {
            body(w, this);
        }
    };
    if !real_threads::fork_join(e, helpers, &job) {
        body(e, this);
    }
}

/// After a stepped join: a join that found no posted job (`joined` false) with the body not run
/// means the job was lost, so the body runs here rather than the frame's AI state going stale; a
/// body that ran and was joined by someone else is not run twice.
fn finish_stepped(e: &mut Engine, this: Ptr, body: LateAiBody, joined: bool, ran: &AtomicBool) {
    if !joined && !ran.load(Ordering::SeqCst) {
        REFUSED.fetch_add(1, Ordering::Relaxed);
        body(e, this);
    }
}

/// For tests: [`finish_stepped`] with a join that reported `joined`.
#[doc(hidden)]
pub fn finish_stepped_for_test(
    e: &mut Engine,
    this: Ptr,
    body: LateAiBody,
    joined: bool,
    ran: bool,
) {
    finish_stepped(e, this, body, joined, &AtomicBool::new(ran));
}
