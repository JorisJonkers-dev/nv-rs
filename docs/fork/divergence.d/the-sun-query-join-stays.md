### The sun query keeps its join, and why
- Files: `docs/FIXES.md`, `crates/engine-fallout-misc/src/units/fallout_misc/main_p2.rs`
- Upstream approach: the fixes reference lists `006426a0` among the drawing thread's early joins without saying what state it shares with the draw; checked in `origin/fork` at `e2411dd0c`
- Fork approach: the reference states what each side owns while a world draw is posted and why dropping the sun query's join is unsafe (the shaders' counted pointers +0x30 and +0x34 are written live by both the draw's pass begin and the query's `00b7a870` set-up); a test pins the world pass's real join (a job posted before the pass is joined by the first half's call, the pass's own job stays posted to OnIdle)
- Decision: keep; offer upstream
- Reason: nv-rs#938 asked for the whole world pass on the drawing side; removing this join was tried and rejected in review as a data race, so the ticket stays open
- Evidence: `world_pass_joins_a_job_posted_before_it_at_its_entry_only`; the existing `these_entries_join_the_drawing_thread_first` for `006426a0`
