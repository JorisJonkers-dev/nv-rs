// SPDX-License-Identifier: GPL-3.0-only
//! The model loader's per-frame work and the garbage collection pass as jobs (nv-rs#941). The
//! stepped component helper already runs Havok's step with the poster's thread id and TLS values,
//! x87 control word, virtual clock and stop stack; these frame calls use the same helper, one job
//! at a time. Both producers share its `posted` flag, so a pending step leaves a loader call inline
//! and a pending loader job leaves a step inline. There is no loader thread of its own: the helper
//! area's slots past the pool stay the Havok helper's and the drawing helper's
//! ([`super::MOST_HELPERS`]), so no slot is taken.
//!
//! The poster posts and joins at once, so the call keeps its place in the frame, the guest locks
//! keep their owner and order, and the join ends the loader's uploads before the frame's draw.
//! Real threads run these calls on the game thread: ADR-0015 keeps a component's real threads off
//! by default until its race campaign passes.

use super::*;

/// A job for [`post_loader`], run once on the stepped component helper's engine.
pub type LoaderJob = HavokJob;

/// Whether this OS thread is the stepped component helper that runs loader jobs.
pub fn on_loader_helper() -> bool {
    on_havok_helper()
}

/// Whether a job for the component helper is posted and not joined yet.
pub fn loader_posted() -> bool {
    havok_posted()
}

/// Whether [`post_loader`] would take a job from `e` now. False where the helper would refuse (with
/// real threads, on any helper, while a typed family runs dual, while a per-call diagnostic is on,
/// or while a job is posted). Posts nothing and changes nothing.
pub fn loader_post_ready(e: &Engine) -> bool {
    havok_post_ready(e)
}

/// Posts `job` to the stepped component helper, taking the poster's x87 control word and virtual
/// clock for it. With stepped threads the job runs when the poster joins ([`join_loader`]), so its
/// place in the run is the poster's join. False and no job posted when the helper would refuse, so
/// the caller can run the call itself.
pub fn post_loader(e: &mut Engine, job: LoaderJob) -> bool {
    post_havok(e, job)
}

/// Returns once the posted job has finished, moving its virtual-clock readings onto the poster and
/// resuming a stop with the calls it unwound through. False when none was posted. The join is the
/// barrier that ends the loader's uploads before the frame's draw.
pub fn join_loader(e: &mut Engine) -> bool {
    join_havok(e)
}
