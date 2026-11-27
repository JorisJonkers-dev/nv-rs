### Patrol time that is not finite
- Files: `crates/engine-fallout-ai/src/units/fallout_ai/lowprocess.rs`, `crates/engine-typed/src/ai/low_process.rs`
- Upstream approach: since the sync to `af5ad9f4` upstream types `LowProcess` part 2 and calls the time-delta function by address; it passes the time on as it comes
- Fork approach: `patrol_time_to_spend` spends a time that is not finite as no time, in the translation and in the typed `process_patrol`; finite times pass unchanged
- Decision: keep; drop when upstream guards the patrol's time
- Reason: with a time scale of 0 the time is infinite after a change of the hour, the patrol loop never ends and the task queue overflows into a null node (nv-rs#457)
- Evidence: nv-rs#457 and the fork's test `a_time_that_is_not_finite_is_spent_as_none`
