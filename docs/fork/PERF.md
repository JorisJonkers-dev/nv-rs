# Profiling the frame (`NV_PERF`)

`NV_PERF=1` turns on a per-frame CPU timeline of the wgpu backend (`crates/d3d9-wgpu/src/perf.rs`, std only, one relaxed
load per span when off). Every 600 presents the log (`viewer.log` under the bench) gets, at info level:

- a line with the wall median, p99 and max of the frames and the mean milliseconds a frame in each category: `draw`
  (recording a draw), `pipeline` (made on a draw), `pass`, `group`, `flush`, `upload`, `create`, `readback`, `sync` (waiting
  for the queue thread), `present`, and `queue_submit` (off-thread, on the queue thread, shown apart). `other` is the
  device thread's time no span covers: waiting for the game thread's commands, which is the translated game code;
- a line counting the frames over twice the median by dominant cause, and the eight worst with their split.

`NV_PERF_FILE=path` also writes one line per frame (wall ns and the ns of each category) for plotting. A span's time has
its children's taken out, so the categories add up to the frame. Use it with the bench: `NV_PERF=1 nv-rs-dev bench run
newgame --threads real`. Pair it with `NV_RENDER_THREAD_STATS=1` (a line a second on the recording thread's waits).

Sampling the game's own code: `perf` (`nix shell nixpkgs#linuxPackages.perf`) works on this machine with
`perf record -e cpu-clock -F 499 -p <pid>` attached to a running offscreen bench (no `-g`: callchains fail to open at
`perf_event_paranoid=2`; the hardware cycles event is not available). The viewer's symbols are kept, so a flat profile by
thread (`--sort comm,dso`) and symbol is readable.

## Findings (2026-11-16, unmodded first room, offscreen, 144 Hz cap, RX 7900 XTX)

- The backend is not the cost. At 945 draws a frame the device thread spends 0.6 ms recording draws (0.65 us a draw), the
  queue thread 0.6 to 1.6 ms in submits, and pipelines made on draws, uploads, resource creation, read-backs and queue waits
  are each under 0.1 ms a frame in steady play. The remaining 6.2 of 7.0 ms of the device thread is waiting for commands
  from the game's main thread.
- A sample of the running viewer: 67% of the process's samples are the game main thread, and it is flat (the dispatcher,
  typed shader constants and lighting, culling, transforms; nothing over 3%). The game main thread is one CPU-bound
  thread, so the frame rate is its speed.
- The bench's default thread mode is not the play default: `newgame` without `--threads real` ran at 70 fps (14.4 ms,
  1% low 33 fps) against 135 to 143 fps (7.4 ms, 1% low 67 to 82) with `--threads real`. With stepped threads the queue
  thread's submit time was 10 ms a frame and the device thread waited on it ("sync").
- The `play` profile builds the engine at opt-level 1; `play-opt` (4 minutes here) gives 143.4 fps (at the cap) with
  p99 9.2 ms, 1% low 82 fps, against 135.5, p99 10.1, 1% low 67 for `play`.
- Hitches in play with real threads are few: 0 to 3 frames of 600 over 2x the median, 14 to 18 ms, all in the game main
  thread (cause `other`, none in pipelines, uploads or read-backs). The 35 to 400 ms frames are on the loading screens
  (a handful of draws a frame, the game's own cell and file work).
