### The accumulator's check no longer joins a posted world draw (nv-rs#939)
- Files: `crates/engine-bs/src/units/bsshader/bsshaderaccumulator.rs`, `crates/engine-bs/src/units/bsshader/run_00b5fd60.rs`, `crates/engine-fallout-misc/src/units/fallout_misc/main_p2.rs`, `crates/engine-typed/src/draw/detach.rs`, `docs/fork/DRAW-WORKERS.md`
- Upstream approach: `00b63a90` (the world accumulator's check) joins a posted world draw before it sums register group 6's counts
- Fork approach: the cut leaves group 6's holder, list and renderers in guest memory (`detach::MAIN_ONLY_GROUPS`; the draw never reads or empties group 6), so the check reads its count where main left it and does not join; the refraction normals' (`00874920`) and the screens' (`00875110`) joins stay
- Decision: keep; offer upstream
- Reason: one of the three joins after the world pass (nv-rs#939); removing it alone does not shorten main's wait yet, the next join follows a few hundred microseconds later
- Evidence: `the_group_count_of_the_world_accumulator_does_not_join`, `the_group_main_counts_after_the_world_draw_stays_in_guest_memory`; the recorded quick-suite runs in DRAW-WORKERS.md
