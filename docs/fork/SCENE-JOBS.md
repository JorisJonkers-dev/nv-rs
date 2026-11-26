# Scene graph and animation updates on a helper (nv-rs#950)

`NV_SCENE_JOBS=1` moves the world scene graph's frame update (`BSSceneGraph::SetViewDistanceBasedOnFrameRate`, the vtable call at `0086e9b5`) and the cell/particle animation update (`0086fc60`) off the game's main thread. Both are posted to one `nv-scene` helper and joined later in the same frame. The switch is off by default.

- With real threads (`NV_THREADS=real`, the play default) the helper starts as soon as a job is posted. Main goes on with its own work and joins before it fixes the camera and multibound visibility (`00c52020`, `0045bc80`, `00b5ac90`, `0045b070`), and again before `Main::Swap` draws the frame. Those passes stay on main, in the game's order, and see the scene update's writes.
- With stepped threads, diagnostics, a typed family in dual mode, or when a job is still pending, the post is refused and the caller runs the update itself. `NV_REPEATABLE` forces stepped threads, so a stepped recording takes the inline path and stays byte-identical.
- One job at a time: a second post before the join is refused, and the caller does that work inline. A stop in a job is recorded for main and comes back at the join with the job's calls on the poster's stack. The helper has its own TLS and stack outside the game's heap, so allocations keep their addresses; it takes the poster's x87 control word. Its helper-area slot is the first past the pool's, before the Havok helper's and the drawing helper's (`crates/engine-core/src/real_threads.rs`).
- The seam writes no float arithmetic. The typed families reached by the moved calls run on main and helpers alike (`Threads::Both`) and follow ADR-0014's float rule: `f64` locals, rounded at each `float` store.

Real threads for this change stay off by default until the race campaign below passes (ADR-0015). Stepped mode remains the reference for tests, recordings and traces.

## Tests that run anywhere

```
flock ~/.cache/nv-workers/cargo.lock ~/.local/bin/capped 10G env CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=$HOME/.cache/cargo-targets/nv-rs-gw cargo test -p engine-core --test scene_jobs_stepped
flock ~/.cache/nv-workers/cargo.lock ~/.local/bin/capped 10G env CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=$HOME/.cache/cargo-targets/nv-rs-gw cargo test -p engine-core --test scene_jobs_real
```

The typed families the moved calls reach keep their `Diff::new` cases in the engine crate's `typed_dispatch`
(scene, under the other threads' control word), `typed_animation`, `typed_particles`, `typed_crt` and `typed_leaves`
binaries; run those too when the seam changes.

## Gate left to the maintainer

No game runs were available when this changed. On the maintainer's machine, compare two stepped recordings of the same scripted input, one with the switch on and one with it off; `NV_REPEATABLE` forces stepped threads whatever `NV_THREADS` says.

```
export NV_REPEATABLE=300 NV_SOUND=out
NV_SCENE_JOBS=1 NV_D3D9_RECORD=/tmp/scene-on.stream nv-rs-dev run
NV_SCENE_JOBS=0 NV_D3D9_RECORD=/tmp/scene-off.stream nv-rs-dev run
cmp /tmp/scene-on.stream /tmp/scene-off.stream
```

Both ends must be byte-identical. Compare `NV_SOUND_RECORD` the same way when the sound device is unchanged. A difference stops the change: check the helper's x87 word and the join point before continuing.

The stepped scene tool's pictures must match too (`NV_REPEATABLE` is set by its runner):

```
nv-rs-dev scene run --places docs/fork/scenes/camera-seam.toml --out /tmp/scene-jobs-off --env NV_SCENE_JOBS=0 --game-lock "$GAME" --label scene-jobs-off
nv-rs-dev scene run --places docs/fork/scenes/camera-seam.toml --out /tmp/scene-jobs-on --env NV_SCENE_JOBS=1 --game-lock "$GAME" --label scene-jobs-on
nv-rs-scene compare /tmp/scene-jobs-off/pictures /tmp/scene-jobs-on/pictures
```

The comparison must show no changed pixels. `nv-rs-scene compare --diff-out <dir>` writes a difference picture when it does not.

For the speed gate, use `NV_PERF=1` and ABBA runs of at least one bench scene (`nv-rs-dev bench list` names them), each with the same preset, resolution and threads, alternating the switch. Take the main-thread stage times from the perf timeline and `bench report` for the run directories; the candidate must lower main-thread time on at least one scene and stay within noise on every other.

```
export NV_PERF=1
nv-rs-dev bench run <scene> --threads real --preset fixes
NV_SCENE_JOBS=1 nv-rs-dev bench run <scene> --threads real --preset fixes
NV_SCENE_JOBS=1 nv-rs-dev bench run <scene> --threads real --preset fixes
nv-rs-dev bench run <scene> --threads real --preset fixes
nv-rs-dev bench compare <first-off-run> <first-on-run>
```

## Race campaign before the switch is on by default

1. Stepped reference: the two recordings above agree byte for byte; both tests pass.
2. Shuffled schedules: run the same scene with `NV_THREADS=shuffle:1` through `shuffle:20`, then with `NV_THREADS=real`; record races with `NV_RACE_CENSUS` where needed. A race the port adds stops the change; one the original has gets a fix in `fixes`.
3. Soaks: run `nv-rs-dev scene soak --places docs/fork/scenes/soak-far-moves.toml --out /tmp/scene-soak --frames 300 --game-lock "$GAME" --label scene-real` and the same with `docs/fork/scenes/soak-rivet-ne.toml`, on Linux and Windows. Every place must be reached with no stops or error lines.
4. Distribution against the original: compare actor positions, scene graph bounds and camera state over several `NV_THREADS=real` runs of one route as distributions, not byte equality. The stepped recording is the reference for what must not move.
5. Only when all phases pass does `NV_SCENE_JOBS` become on by default in play. Until then it is an explicit opt-in.
