### Water frames draw on main when the world draw is detached
- Files: `docs/FIXES.md`, `crates/engine-typed/src/draw/cut.rs`, `crates/engine/tests/typed_shader.rs`
- Upstream approach: the world draw's second half is not detached (`NV_DRAW_THREAD` does not exist); its water reflection copy runs where the draw runs
- Fork approach: a frame whose second half copies the water reflection (`+0x38` set, a pass of the types 0x288..0x2ce listed) is not handed to the drawing thread and draws on main; POISON's serial cut of it opens no packet
- Decision: keep (until the image space chain and the renderer's target state are typed per side); not for upstream
- Reason: nv-rs#1002: the reflection copy's image space effect reads own state in guest memory and the renderer's target words main writes meanwhile; the first posted draw of a water scene stopped the run
- Evidence: `a_reflection_copy_frame_is_not_handed_to_the_drawing_thread`; Boulder Beach (`docs/fork/scenes/bench-water.toml`) runs to the end stepped, POISON and real threads
