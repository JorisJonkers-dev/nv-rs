### Thread clock test measured in short slices
- Files: `crates/engine-core/src/thread_cpu.rs` (test `a_thread_s_own_clock_tells_working_from_sleeping` only)
- Upstream approach: upstream measures one 40 ms busy stretch and takes the best of five; on a loaded two-core CI runner a preemption inside the stretch drops its share to 0.21–0.56, below the 0.6 bound, and the test fails now and then
- Fork approach: each busy stretch is cut into 2 ms slices and the best uninterrupted slice counts; the bounds are unchanged (busy share 0.6 to 1.05, never above 1.05, sleeping share below 0.2)
- Decision: keep; drop when upstream makes the test robust against preemption
- Reason: the test failed on an unrelated pull request (nv-rs#959) in the Linux core job
- Evidence: the failing run on #959; 0 failures in 30 runs under load with the change, and a deliberately half-speed clock still fails the test
