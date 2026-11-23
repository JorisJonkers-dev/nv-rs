# A new run counted as beside the packet of a run that is gone

- Issue: nv-rs#789
- Since: the upstream sync #763 (0cb1f46d)
- Cause: `capture::OPEN_RUNS` named a run by its kernel's address and kept the slot of a run dropped with its packet open, so a later engine at the same address counted as beside a packet.
- Tests: `a_new_run_is_not_beside_the_packet_of_a_run_gone`, `a_packet_dropped_without_an_end_gives_its_slot_back` (`crates/engine-typed/src/draw/capture.rs`)
