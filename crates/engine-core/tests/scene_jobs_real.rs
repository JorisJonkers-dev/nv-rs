//! The scene job seam with real threads (`engine_core::real_threads`): a scene update posted before
//! the camera and visibility passes runs on the `nv-scene` helper beside main, is joined before the
//! frame draws, and does not allow a second post before the join. One scene helper per process, so
//! this binary holds one run and one test.

use engine_core::real_threads::{self, on_scene_helper};
use engine_core::Engine;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering::SeqCst};

engine_core::assemble!();

/// The word the scene update advances, in guest memory.
const VALUE: u32 = 0x0300_0000;

static MAIN_WENT_ON: AtomicUsize = AtomicUsize::new(0);
static RAN: AtomicU32 = AtomicU32::new(0);

#[test]
fn with_real_threads_the_scene_update_runs_beside_main_and_joins() {
    let mut e = Engine::new();
    e.start_real_clock();
    real_threads::start(&mut e).expect("real threads on");
    e.map(VALUE, 4);
    e.set_global(VALUE, 0u32);
    real_threads::scene_jobs_set_for_tests(true);

    let posted = real_threads::post_scene(
        &mut e,
        Box::new(|e| {
            // The helper waits for main to go on: the update really runs beside the frame's work.
            while MAIN_WENT_ON.load(SeqCst) == 0 {
                std::thread::yield_now();
            }
            assert!(on_scene_helper(), "the update runs on the scene helper");
            let value = e.global::<u32>(VALUE);
            e.set_global(VALUE, value.wrapping_add(1));
            RAN.fetch_add(1, SeqCst);
        }),
    );
    assert!(posted, "the job was taken");
    assert!(real_threads::scene_posted(), "the job is pending");

    MAIN_WENT_ON.store(1, SeqCst);
    // Main goes on while the job runs; it cannot replace the pending job with another one.
    assert!(
        !real_threads::post_scene(
            &mut e,
            Box::new(|_| {
                RAN.fetch_add(1, SeqCst);
            })
        ),
        "one job at a time"
    );
    assert!(real_threads::join_scene(&mut e), "the job joined");
    assert_eq!(RAN.load(SeqCst), 1, "only the posted job ran");
    assert_eq!(e.global::<u32>(VALUE), 1, "the helper wrote the guest word");
    assert!(!real_threads::scene_posted(), "the join cleared the job");

    // A stop in a job comes back at the join, and the next job can still be posted.
    assert!(real_threads::post_scene(
        &mut e,
        Box::new(|_| panic!("the scene stopped"))
    ));
    let panic = catch_unwind(AssertUnwindSafe(|| real_threads::join_scene(&mut e)));
    assert!(panic.is_err(), "the job's panic was resumed at the join");
    assert!(!real_threads::scene_posted());
    assert!(real_threads::post_scene(
        &mut e,
        Box::new(|_| {
            RAN.fetch_add(1, SeqCst);
        })
    ));
    assert!(real_threads::join_scene(&mut e));
    assert_eq!(RAN.load(SeqCst), 2);
}
