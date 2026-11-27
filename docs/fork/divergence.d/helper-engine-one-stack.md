### A helper engine keeps its locals on one stack
- Files: `crates/engine-core/src/engine.rs` (`stack_key`, `push_frame`)
- Upstream approach: a helper engine's stack for `with_stack` locals is chosen by the shared kernel's current game thread, and the engine stops the run if a second game thread asks for locals on the same placed stack, because a helper ran only inside the main thread's fork.
- Fork approach: an engine with a placed stack (a render helper's) has that one stack whichever game thread is current. The Havok helper is posted from whichever game thread steps the world, so it asks for locals as different game threads in one run, and the stop ended the scene soak at frame 121 (nv-rs#997).
- Decision: keep
- Reason: a helper runs one job at a time, so one placed stack is correct for it; the owner check was an assumption the Havok helper broke
- Evidence: `a_helpers_locals_stay_on_its_placed_stack_whichever_game_thread_is_current`; scene soak runs below (pull request)
