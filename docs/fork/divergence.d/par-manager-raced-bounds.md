### The parallel task manager test does not pin raced bounds
- Files: `crates/engine/tests/par_manager.rs`
- Upstream approach: the test compares the bounds of the nodes two items update at once with a tolerance of 1e-5 of the one-thread result
- Fork approach: those bounds are only checked to be a finite sphere of the same size to within a factor of two; the world transforms, every other bound and the syncs stay exact
- Decision: keep
- Reason: the exe's two runners race on those bounds (each resets the bound and merges into it in place), so a sphere merge's centre and radius depend on the order the children are merged in, and one runner's reset can drop what the other merged; on Linux the old tolerance failed about one run in three under load, on Windows CI now and then (nv-rs#1022)
- Evidence: 200 runs of the test in a row pass with 4 and 8 threads; the old tolerance failed 20 of 60
