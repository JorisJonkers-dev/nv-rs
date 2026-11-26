//! The scene job seam with stepped threads (`engine_core::real_threads`): a scene update is never
//! posted, whatever the switch says, so a stepped frame keeps the game's own order and a repeatable
//! recording stays byte-identical. The frame path's post-or-do pattern then updates on the caller.
//! One scene helper per process, so this binary holds one run.

use engine_core::real_threads;
use engine_core::Engine;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

engine_core::assemble!();

/// The word the scene update advances, in guest memory.
const VALUE: u32 = 0x0300_0000;

static RUNS: AtomicUsize = AtomicUsize::new(0);

/// The work the scene update does, as the frame path calls it.
fn update(e: &mut Engine) {
    let value = e.global::<u32>(VALUE);
    e.set_global(VALUE, value.wrapping_add(1));
    RUNS.fetch_add(1, SeqCst);
}

/// The frame path's call: post the update, or do it here when the seam refuses.
fn frame_update(e: &mut Engine) {
    if !real_threads::post_scene(
        e,
        Box::new(|e| {
            update(e);
        }),
    ) {
        update(e);
    }
}

#[test]
fn with_stepped_threads_the_scene_update_stays_on_the_caller() {
    real_threads::scene_jobs_set_for_tests(false);
    assert!(!real_threads::scene_posted(), "no job at the start");

    let mut e = Engine::new();
    e.map(VALUE, 4);
    e.set_global(VALUE, 0u32);

    frame_update(&mut e);
    assert_eq!(e.global::<u32>(VALUE), 1, "one update on the caller");
    assert_eq!(RUNS.load(SeqCst), 1);
    assert!(!real_threads::scene_posted(), "nothing was posted");
    assert!(!real_threads::join_scene(&e), "nothing to join");

    // The switch alone does not move work: stepped threads are the reference for recordings.
    real_threads::scene_jobs_set_for_tests(true);
    frame_update(&mut e);
    assert_eq!(
        e.global::<u32>(VALUE),
        2,
        "the second update is on the caller too"
    );
    assert_eq!(RUNS.load(SeqCst), 2);
    assert!(!real_threads::scene_posted());
}
