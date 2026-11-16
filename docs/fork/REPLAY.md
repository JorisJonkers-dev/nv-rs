# The replay-bench (`d3d9-replay`)

The cost of a draw call, measured on a recorded stream instead of on the whole game. The stream is the Direct3D 9
call stream of `d3d9-stream` as `NV_D3D9_RECORD` writes it; the bench plays it into a backend over and over and times
the backend only: no engine, no game thread, no window. The first use is the decision in
`docs/research/nv-rs-vulkan-renderer.md` (stage S0); the numbers go to `docs/fork/VULKAN-S0.md` when the stage is done.

## Backends

| Backend | What it is | Where |
| --- | --- | --- |
| `null` | counts the commands and hashes the draws; the floor of the harness | `crates/d3d9-replay/src/null.rs` |
| `wgpu` | the game's backend (`d3d9-wgpu`, vendored wgpu-hal) on an offscreen device, with its finishing and queue threads | `crates/d3d9-replay/src/replay.rs` |
| `vulkan` | a throwaway raw `ash` prototype of the draw, state and bind path, a measurement tool and not a backend: Vulkan 1.3, dynamic rendering, push descriptors, one ring for the registers, pipelines made on first use, one thread; textures keep no contents and queries, copies, multisampling, clip planes and the alpha test are left out | `crates/d3d9-replay/src/vulkan.rs` (`NV_REPLAY_VK_DEVICE` picks the adapter by name) |

## Streams

Streams are recordings of the game: shader bytecode, draw parameters and texture bytes. They are private, stay under
`~/.local/share/nv-rs-research/streams/` and are never committed or shared; only numbers leave the machine. A stream
is kept as a container (`*.nvrs`, `crates/d3d9-replay/src/container.rs`): a magic, a version, the command-list hash it
was recorded with (a stream of another build is refused with both hashes in the message), a few numbers about it and
the wire file compressed.

1. Record a run with the viewer, through the bench, at 640x480 at the most:
   `NV_D3D9_RECORD=~/.local/share/nv-rs-research/streams/raw/newgame.d3d9s nv-rs-dev bench run newgame --threads real --resolution 640x480`.
   The file holds every command of the run from the first (the whole run's resources), so it is large.
2. `nv-rs-dev replay info FILE` lists the commands and the draws a frame in runs of 100 frames, to choose a window.
3. `nv-rs-dev replay pack RAW OUT --scene doc-room --from 2400 --frames 60` cuts the window out
   (`crates/d3d9-replay/src/window.rs`): the resources the window needs are made first (those destroyed before it, and
   uploads a later one covers whole, are left out), the window's own creations are moved ahead of its frames and its
   destroys left out, so the frames can be replayed over and over.

## Measuring

`nv-rs-dev replay build` makes the release binary in the loop's target dir. `nv-rs-dev replay run doc-room --backend all
--runs 5` takes the machine-wide bench lock (`~/.cache/nv-rs/bench.lock`, as `bench run`: nobody else measures while it
runs) and prints, for each run of each backend:

- `us/draw`: the commands of a frame up to its present over the frame's draws (state, binds and constants included:
  what the backend costs the thread that feeds it), the same with the present, the feeding thread's CPU time and all
  the process's threads' CPU time. The finishing and queue threads of the wgpu backend are in the last; the table
  `threads, cpu us/draw` names each thread's share;
- the milliseconds a frame takes as mean, median, p90, p99 and max, over the measured frames (the first `--warmup`
  passes over the frames, default 3, are left out: pipelines, bind groups and the driver warm up there);
- draws a second;
- with `--gpu`, the GPU time of each frame by timestamp queries (the span from a timestamp before the frame's first
  batch to one after its last: with a CPU-bound backend it includes the card's idle time between batches);
- with `--split`, one pass with every command timed on its own, by class (draw, state, bind, constants, upload).

`--size WxH` replays the stream at a larger render target without recording again: the back buffers and the
screen-sized targets (the screen's size and its halves down to a sixteenth) change size, with their viewports,
scissors, clears and copies (`crates/d3d9-replay/src/rescale.rs`). The picture is not the game's at that size (shader
constants holding the screen size and pixel-space vertices stay as recorded); the work is what it is for.

`NV_REPLAY_GPU_TIMING=1` asks `d3d9-wgpu` for timestamp queries when the device is made (`--gpu` sets it); the game's
run never does. `nv-rs-dev replay synth OUT` writes a made-up scene (no game data) for checking the harness.
`NV_WGPU_CACHE=DIR` keeps the driver's compiled pipelines between runs as the game does.

## Tests

`cargo test -p d3d9-replay`: the container round trip and its refusals, the cut of a window, the rescale, the null
backend's determinism, and the synthetic scene replayed into wgpu (on lavapipe in CI; it skips without an adapter).
`scripts/fork/tests/test_d3d9replay.py`: the wrapper and the lock.
