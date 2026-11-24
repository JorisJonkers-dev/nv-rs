### Holder tests ask for a tenth of the wait as processor time
- Files: `crates/engine-core/src/real_threads.rs` (the tests `main_s_long_wait_for_the_kernel_lock_names_the_holder` and `main_s_long_wait_for_a_section_names_its_owner`)
- Upstream approach: the tests assert that the named holder thread used at least half of main's wait as processor time
- Fork approach: the same tests, with a tenth; the holder is still named (call, thread, start, wait) and, where its processor time can be read, must have worked (a sleeping or blocked holder has none); the clock table is keyed by run and thread, so a thread of another test with the same key can take the entry over and make the time unreadable on a loaded machine
- Decision: keep; drop when upstream's tests no longer depend on the machine's load
- Reason: upstream's half fails on a loaded two-core machine (the coverage job, `taskset -c 0,1`: 78 ms of 252 ms), whatever the code does
- Evidence: the two tests, run on two cores (failed before, pass after) and on all cores
