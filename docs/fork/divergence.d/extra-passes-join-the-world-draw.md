### The extra passes run beside a posted world draw
- Files: `docs/FIXES.md`, `crates/engine-typed/src/draw/drivers.rs`, `crates/engine-bs/src/units/bsshader/run_00b6c0d0.rs`
- Upstream approach: `00b6c0d0` (the secondary scenes' draw) runs on main while a detached world draw is posted; the fixes reference leaves open whether the two share state
- Fork approach: `00b6c0d0` no longer joins the posted draw (it did, at its entry, from nv-rs#990 to nv-rs#992): the shaders' counted pointers, the constant entries, the stage objects, their state caches and the render state blocks both write are the draw's own blocks while it is posted, and main's writes win at the join where main wrote last
- Decision: keep; offer upstream
- Reason: nv-rs#992 (part of #938): the join kept the world pass's extra scenes from overlapping the draw
- Evidence: `scene_draw_does_not_join_a_posted_world_draw`, `the_draw_reads_the_pass_objects_as_cut_while_main_begins_the_same_pass`
