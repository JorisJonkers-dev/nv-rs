### A run's number and the open packets' slots
- Files: `crates/engine-core/src/units/platform.rs` (`KernelRef::id`), `crates/engine-typed/src/draw/capture.rs` (`OpenSlot`)
- Upstream approach: a run is named by its kernel's address, and a packet's slot is given back only by `capture::end`; checked at the base
- Fork approach: a run number that is never used twice, and a slot given back however the packet ends
- Decision: keep; offer upstream
- Reason: a later engine at a freed kernel's address counted as beside a dead run's packet, which failed a test on Windows (nv-rs#789)
- Evidence: `a_new_run_is_not_beside_the_packet_of_a_run_gone`, `a_packet_dropped_without_an_end_gives_its_slot_back`
