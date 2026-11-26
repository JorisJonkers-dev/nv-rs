// SPDX-License-Identifier: GPL-3.0-only
//! The loader's own OS thread for stepped runs (nv-rs#941): the model loader's per-frame work and
//! the garbage collection pass are posted on the game thread and run on `nv-loader` when that
//! thread joins them, as [`super::post_draw`] does for the world draw. They keep the poster's thread
//! id, TLS values, x87 control word and virtual clock, so the guest locks they take have the same
//! owner and the same order as in an inline call. One helper per process, for the run that first
//! posts. Real threads run these calls on the game thread: their race campaign has not passed yet.

use super::*;

/// The loader helper's slot of [`crate::mem::HELPER_AREA`], beside the drawing and Havok helpers'.
const SLOT: u32 = DRAW_SLOT - 2;
const _: () = assert!(SLOT >= MOST_HELPERS as u32 && SLOT < DRAW_SLOT - 1);

/// A job for [`post_loader`], run once on the loader helper's engine.
pub type LoaderJob = Box<dyn FnOnce(&mut Engine) + Send>;

struct LoaderHelper {
    state: Mutex<LoaderState>,
    start: Condvar,
    done: Condvar,
    kernel: usize,
}

#[derive(Default)]
struct LoaderState {
    /// A job is posted and not joined yet.
    posted: bool,
    /// The job, until the helper takes it.
    job: Option<LoaderJob>,
    /// It may run: set when the poster joins (a stepped run has no other choice of thread here).
    go: bool,
    finished: bool,
    /// The poster's x87 control word and virtual clock, taken for the job.
    control_word: u16,
    clock: Option<(u32, u32)>,
    frame_ends: u64,
    clock_moved: Option<u64>,
    /// The job's panic, resumed at the join.
    panic: Option<Box<dyn std::any::Any + Send>>,
    /// The calls the job unwound through, innermost last: a stop at the join reports the same
    /// functions as one in an inline call.
    stop_stack: Vec<u32>,
}

impl LoaderHelper {
    fn assert_same_run(&self, kernel: usize) {
        assert!(
            self.kernel == kernel,
            "post_loader: the loader helper belongs to another run (its engine is a view of that \
             run's memory); one run per process may post"
        );
    }
}

static LOADER: std::sync::OnceLock<&'static LoaderHelper> = std::sync::OnceLock::new();

thread_local! {
    /// This OS thread is the loader helper.
    static LOADER_HELPER: Cell<bool> = const { Cell::new(false) };
}

/// Whether this OS thread is the loader helper.
pub fn on_loader_helper() -> bool {
    LOADER_HELPER.with(Cell::get)
}

/// Whether [`post_loader`] would take a job from `e` now. False where it would refuse (with real
/// threads, on any helper, while a typed family runs dual, while a per-call diagnostic is on, or
/// while a job is posted). Posts nothing and changes nothing.
pub fn loader_post_ready(e: &Engine) -> bool {
    e.real.is_none()
        && !HELPER.with(Cell::get)
        && !on_draw_helper()
        && !on_havok_helper()
        && !on_loader_helper()
        && !e.typed_any_dual()
        && diagnostic_on(e).is_none()
        && !loader_posted()
}

/// Whether a job is posted to `nv-loader` and not joined yet.
pub fn loader_posted() -> bool {
    LOADER.get().is_some_and(|h| guard(&h.state).posted)
}

/// Posts `job` to `nv-loader`, taking the poster's x87 control word and virtual clock for it. With
/// stepped threads it starts when the poster joins ([`join_loader`]), so the job's place in the run
/// is the poster's join. The memory becomes shared at the first post and the helper's TLS block and
/// stack have their own slot outside the heap. False and no job posted when the helper would
/// refuse, so the caller can run the call itself.
pub fn post_loader(e: &mut Engine, job: LoaderJob) -> bool {
    if !loader_post_ready(e) {
        return false;
    }
    let kernel = e.kernel.id();
    if let Some(h) = LOADER.get() {
        h.assert_same_run(kernel);
    }
    if !e.mem.is_shared() {
        if !e.mem.can_share() {
            return false;
        }
        e.kernel.leak();
        e.device.leak();
        e.mem.start_sharing();
    }
    let h = *LOADER.get_or_init(|| make_loader_helper(e));
    h.assert_same_run(kernel);
    let control_word = crate::units::crt::x87_control_word(e);
    let clock = e.repeatable.then_some((e.clock_ms, e.clock_us));
    let frame_ends = e.repeatable_frame_ends();
    {
        let mut s = guard(&h.state);
        if s.posted {
            return false;
        }
        s.posted = true;
        s.finished = false;
        s.go = false;
        s.panic = None;
        s.stop_stack.clear();
        s.control_word = control_word;
        s.clock = clock;
        s.frame_ends = frame_ends;
        s.clock_moved = None;
        s.job = Some(job);
    }
    h.start.notify_all();
    true
}

/// Returns once the posted job has finished (it runs now, while the poster waits), and moves the
/// job's virtual-clock readings onto the poster. False when none was posted. A stop in the job is
/// resumed here, with the calls it unwound through on the poster's stack: the same stop the inline
/// call would have reported. The join is the barrier that ends the loader's uploads before the
/// frame's draw.
pub fn join_loader(e: &mut Engine) -> bool {
    let Some(h) = LOADER.get() else {
        return false;
    };
    {
        let mut s = guard(&h.state);
        if !s.posted {
            return false;
        }
        s.go = true;
    }
    h.start.notify_all();
    let mut s = guard(&h.state);
    while !s.finished {
        s = wait(&h.done, s);
    }
    let clock_moved = s.clock_moved.take();
    let frame_ends = s.frame_ends;
    let panic = s.panic.take();
    let stop_stack = std::mem::take(&mut s.stop_stack);
    s.posted = false;
    drop(s);
    if let Some(us) = clock_moved {
        e.advance_clock_for_join(us, frame_ends);
    }
    if let Some(payload) = panic {
        e.call_stack.extend(stop_stack);
        std::panic::resume_unwind(payload);
    }
    true
}

/// The loader helper and its OS thread, with an engine made on this thread.
fn make_loader_helper(e: &mut Engine) -> &'static LoaderHelper {
    let h: &'static LoaderHelper = Box::leak(Box::new(LoaderHelper {
        state: Mutex::new(LoaderState::default()),
        start: Condvar::new(),
        done: Condvar::new(),
        kernel: e.kernel.id(),
    }));
    let mut helper = e.helper_context();
    helper.place_thread_memory(
        crate::mem::HELPER_AREA.start + SLOT * crate::mem::HELPER_SLOT,
        crate::mem::HELPER_SLOT,
    );
    let engine = SendEngine(Box::new(helper));
    std::thread::Builder::new()
        .name("nv-loader".into())
        .stack_size(THREAD_STACK)
        .spawn(move || {
            qos::thread_started();
            run_loader_helper(engine, h)
        })
        .expect("the loader helper's thread");
    h
}

/// The loader helper's body: waits for each job, runs it on its engine, marks it finished. The
/// poster's setup is inside the catch too: a stop there must not kill the thread and leave the
/// poster waiting for a job that can never finish.
fn run_loader_helper(engine: SendEngine, h: &'static LoaderHelper) {
    LOADER_HELPER.with(|v| v.set(true));
    let mut e = engine.0;
    loop {
        let (job, control_word, clock) = {
            let mut s = guard(&h.state);
            while !(s.go && s.job.is_some()) {
                s = wait(&h.start, s);
            }
            (s.job.take().unwrap(), s.control_word, s.clock)
        };
        let mark = e.guest_stack_mark();
        let mut from = 0;
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            sync_registered(&mut e);
            crate::units::crt::set_x87_control_word(&mut e, control_word);
            if let Some((ms, us)) = clock {
                e.repeatable = true;
                e.real_clock = None;
                e.clock_ms = ms;
                e.clock_us = us;
                from = e.clock_peek_us();
            }
            job(&mut e)
        }));
        let stop_stack = if out.is_err() {
            e.guest_stack_restore(mark);
            let stack = std::mem::take(&mut e.call_stack);
            e.stack_frames.clear();
            stack
        } else {
            Vec::new()
        };
        {
            let mut s = guard(&h.state);
            if clock.is_some() {
                s.clock_moved = Some(e.clock_peek_us().saturating_sub(from));
            }
            if let Err(payload) = out {
                s.panic = Some(payload);
                s.stop_stack = stop_stack;
            }
            s.finished = true;
            s.go = false;
        }
        h.done.notify_all();
    }
}
