### The extra passes join a posted world draw first
- Files: `docs/FIXES.md`, `crates/engine-typed/src/draw/drivers.rs`, `crates/engine-bs/src/units/bsshader/run_00b6c0d0.rs`
- Upstream approach: `00b6c0d0` (the secondary scenes' draw) runs on main while a detached world draw is posted; the fixes reference leaves open whether the two share state
- Fork approach: `00b6c0d0` joins the posted draw at its entry; both write the shaders' counted pointers (+0x28, +0x30, +0x34) of the same shader objects live in guest memory
- Decision: keep; offer upstream
- Reason: nv-rs#990: a counted pointer assigned from two threads loses a count, and with stepped threads the draw's pointers land after the extra pass's, not before as in the serial order
- Evidence: `scene_draw_joins_a_posted_world_draw_first`
