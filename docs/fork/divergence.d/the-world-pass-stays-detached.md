### The world pass stays detached
- Files: `crates/engine-fallout-unplaced-a/src/units/unplaced/run_006426a0.rs`, `crates/engine-fallout-misc/src/units/fallout_misc/main_p2.rs`, `docs/FIXES.md`
- Upstream approach: the sun's occlusion query (`006426a0`) joins a posted world draw at its entry, inside the world pass, and the fixes reference lists it among the early joins; checked at the base
- Fork approach: the query runs without joining, so the draw posted at the world pass's first half stays on the helper through the whole pass and is joined at the first accumulator user after it; the reference describes that path
- Decision: keep; offer upstream
- Reason: nv-rs#938 — the early join inside the world pass pulls the detached draw back to main, so the cut and its value packet cover only part of the pass
- Evidence: `world_pass_keeps_a_posted_draw_posted_through_the_sun_query` (the pass's order and no join before OnIdle) and `the_sun_query_keeps_the_world_draw_posted` fail when the entry joins; `draw_thread::Counts::same_pass` is the join counter, zero for a pass with no early join
