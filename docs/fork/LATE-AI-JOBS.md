# The late AI tasks on a worker (nv-rs#940)

Fork-only. `NV_LATE_AI=1` moves the body of `Main::UpdateNonRenderSafeAITasks` (`0086f6a0`) off the game's main thread. The body keeps the original's call order: the dialog timer, the process lists' projectiles, animation scene graphs, detection lists and process step, the Pip-Boy and child attaches, the first task queue's drain and the queued node updates. The seam joins before the call returns, so `00870610` and the next frame see the writes at the same place in the run as before.

- With stepped threads the body is posted to the existing stepped helper (`post_havok`, the one the Havok step uses between frames), which runs only when its poster joins and takes one job at a time. It takes the poster's thread identity, x87 control word and virtual clock, and a stop inside it comes back at the join with the calls the helper unwound through. `NV_REPEATABLE` forces stepped threads, so a recording with the switch on takes the same calls in the same order.
- With real threads one helper of the existing pool runs the body while main waits. The helper takes no guest heap block: its TLS and stack have a fixed place in `HELPER_AREA`, so the run's heap layout does not change. No new helper slot is reserved; the scene, Havok and drawing helpers keep theirs. The call is made once in the frame tail, where no other pool fork is in flight.
- The switch is off in every preset (ADR-0015). A per-call diagnostic, a typed family running dual, a stepped helper already holding a job or unshareable memory leaves the body inline. On the default path the seam is one relaxed load and the inline call.
- The seam adds no float arithmetic. The body calls the process family's after-draw functions: that family is marked `Threads::Main` because its level passes do one `f32` subtraction, and those passes are not called here. The functions it does call move floats as words without arithmetic, and the bone update it reaches already runs on helper engines under `NV_PAR_BONES`. Nothing in the seam introduces an `f64` local or a float store, so ADR-0014's rule for code on other threads has no new arithmetic to apply to.

Real threads for this change stay off by default until the race campaign below passes (ADR-0015). Stepped mode remains the reference for tests, recordings and traces.

## Tests that run anywhere

```
flock ~/.cache/nv-workers/cargo.lock ~/.local/bin/capped 10G env CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=$HOME/.cache/cargo-targets/nv-rs-gw cargo test -p engine-core --test late_ai_stepped
flock ~/.cache/nv-workers/cargo.lock ~/.local/bin/capped 10G env CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=$HOME/.cache/cargo-targets/nv-rs-gw cargo test -p engine-core --test late_ai_real
```

Both binaries build their inputs from scratch: registered stand-ins for the tasks and made-up guest words. The stepped test compares the posted and inline bodies' calls, words, x87 control word, virtual clock, bytes written and stop stack. The real-threads test checks that the body runs on a helper, that a diagnostic keeps it on the caller, and that the words survive.

## Gate left to the maintainer

No game runs were available when this changed. Compare two stepped recordings of the same scripted input, one with the switch on and one with it off:

```
export NV_REPEATABLE=300 NV_SOUND=out
NV_LATE_AI=1 NV_D3D9_RECORD=/tmp/late-ai-on.stream nv-rs-dev run
NV_LATE_AI=0 NV_D3D9_RECORD=/tmp/late-ai-off.stream nv-rs-dev run
cmp /tmp/late-ai-on.stream /tmp/late-ai-off.stream
```

Both ends must be byte-identical. `NV_SOUND_RECORD` can be compared the same way when the sound device is unchanged. A difference stops the change: check the helper's x87 word, clock hand-off and the join point before continuing.

For the speed gate, use `NV_PERF=1` and ABBA runs of at least one bench scene, alternating the switch with the same preset, resolution and threads. Take the main-thread stage times from the perf timeline and `bench-compare` for the run directories. The candidate must lower main-thread time on at least one scene and stay within noise on every other.

```
export NV_PERF=1
NV_LATE_AI=0 nv-rs-dev scene bench --suite quick --lock "$GAME" --out /tmp/late-a --label late-ai-off
NV_LATE_AI=1 nv-rs-dev scene bench --suite quick --lock "$GAME" --out /tmp/late-b --label late-ai-on
NV_LATE_AI=1 nv-rs-dev scene bench --suite quick --lock "$GAME" --out /tmp/late-c --label late-ai-on
NV_LATE_AI=0 nv-rs-dev scene bench --suite quick --lock "$GAME" --out /tmp/late-d --label late-ai-off
nv-rs-dev scene bench-compare /tmp/late-a/soak/bench.json /tmp/late-b/soak/bench.json
```

## Race campaign before the switch is on by default

1. Stepped reference: the two recordings above agree byte for byte; both tests pass.
2. Shuffled schedules: run the same scene with `NV_THREADS=shuffle:1` through `shuffle:20`, then with `NV_THREADS=real`; record races with `NV_RACE_CENSUS` where needed. `NV_LATE_AI=1` is set for the candidate runs and absent for the reference. A race the port adds stops the change; one the original has gets a fix in `fixes`.
3. Soaks: run `nv-rs-dev scene soak --places docs/fork/scenes/soak-far-moves.toml --out /tmp/late-ai-soak --frames 300 --game-lock "$GAME" --label late-ai-real` and the same with `docs/fork/scenes/soak-rivet-ne.toml`, on Linux and Windows. Every place must be reached with no stops or error lines.
4. Distribution against the original: compare actor positions, process states and detection results over several `NV_THREADS=real` runs of one route as distributions, not byte equality. The stepped recording remains the reference for what must not move.
5. Only when all phases pass does `NV_LATE_AI` become on by default in play. Until then it is an explicit opt-in.
