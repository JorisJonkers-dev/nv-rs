# Hitch tracing (`fork-hitch`)

A flight recorder that is on by default in real play. When a frame is far over the usual one it writes one line to the JSON
log that says what the frame was spent on, what led up to it and which function was running; at the end of every run it
writes a small summary file with flags, so a stutter can be traced after the fact and a bad run is noticed without anyone
pointing at it. Fork-owned (`crates/fork-hitch`, std only apart from `fork-log`); the hot path is a coarse clock word, a few
relaxed stores and adds, nothing allocates there (see "Overhead").

## Switches

| Variable | Meaning |
| --- | --- |
| `NV_HITCH` | `on` (default), `off`, or `detail` (the sampler runs on every frame over about three typical ones, full counters for the frames before a hitch) |
| `NV_HITCH_MS` | Hitch threshold, 25 ms. A frame is a hitch when it is over `max(factor x rolling median, threshold)` |
| `NV_HITCH_FACTOR` | The multiple of the rolling median (last 128 frames), 3 |
| `NV_HITCH_SUMMARY_SECS` | Terminal summary interval, 60; 0 for none (the exit summary stays) |
| `NV_HITCH_CSV` | Path of the hitch marker CSV; default beside the JSON log (`<log>.hitches.csv`) |
| `NV_HITCH_FLAG_PER_MIN`, `NV_HITCH_FLAG_P99_MS`, `NV_HITCH_TARGET_FPS`, `NV_HITCH_FLAG_LOADING_S`, `NV_HITCH_FLAG_MIN_SECS` | Flag levels of the run summary: 3 hitches a minute, 20 ms p99, 60 fps, 120 s of loading, 20 s of play before rates are judged |

## What a frame record holds

The viewer calls `fork_hitch::frame_end` once per main-loop frame. The ring keeps the last 2048 frames (about ten seconds). A
frame has its wall time; the change of every counter (platform call time on the main thread, kernel-lock and device-lock waits,
file time and file reads and bytes, sound, object waits, waits on the render thread, draws, pipeline, shader, texture and buffer
creations with their counts and times, presents, GPU waits, device-thread and queue-thread time); minor and major page faults,
growth of the peak resident size and process CPU time (`getrusage`); and the player's context: cell and world-space form ids,
position, and the screen (`play`, `menu`, `loading`). Counters are fed by `fork_hitch::add` and `fork_hitch::span`; the ones
nothing feeds yet read zero (see "Status").

## The hitch record

One line in the JSON log, `"kind":"hitch"`, level `warn`, with a `hitch` object: `frame_idx`, `wall_ms`, `median_ms`, `cause`,
`buckets` (the time in each cause bucket, ms), `trigger`, `frame` (the full breakdown above), `before` (the five frames before),
`events` (everything noted in the preceding 500 ms with its age) and `samples` when the sampler had any. More than twelve records
in ten seconds (a loading screen is one long hitch, not a hundred) are written `"brief":true` without `before` and `samples`; they
are still counted. A hitch over twice the threshold is also an info line on the terminal.

`trigger` is the newest event before the frame: `cell.change`, `world.change`, `screen.loading`, `loading.begin`, the game
trace's `menu.loading` and `load.thread`, or `slow:<Win32 call>` for a platform call over 2 ms. Events carry ids and numbers only.

### Cause taxonomy

| Cause | Chosen when |
| --- | --- |
| `loading` | the screen is a loading screen, or a load event is in the preceding 500 ms: a loading kind of the game trace, or `world.change` (a change of worldspace, which a console teleport makes with no loading screen). A `cell.change` inside a worldspace, as when walking across a cell border, is play |
| `gc/alloc` | 10 000 page faults (about 40 MiB of fresh pages) or 32 MiB of resident growth in the frame, and the faults cost a fifth of it |
| `file_io` | file calls on the main thread hold a third of the frame |
| `pipeline_create`, `shader_compile` | pipeline or shader-module creation holds a third |
| `texture_upload` | texture and buffer creation and upload hold a third |
| `lock_contention` | waiting for the kernel lock holds a third |
| `render_wait`, `device_wait` | waiting for the render/drawing thread, or for the device lock and the GPU, holds a third |
| `game_logic` | none of those, and the main thread ran the game's code for over half the frame |
| `unknown` | the rest, usually time inside a platform call no bucket names (`platform_other_ms` in the record, and the sampler's call name) |

### The sampler

The main thread publishes, at each platform call, which call it is in, the guest function that made it (the whole chain of up to
eight once the frame is over its budget, see "Overhead") and the class of its argument (a file's extension and a hash of its name,
never a path). A sampler thread reads it every 2 ms while the frame in
progress is older than twice the threshold (older than about three typical frames in `detail`), and the record carries a
histogram: the platform calls the thread was in, with the nearest guest caller, and the hottest guest function addresses.
A frame stuck in the game's own code makes no platform call: its samples say so (`in_guest`) and the chain is that of the last
call it made; with `NV_PROFILE=1` the profiler's shadow stack is used instead. Addresses are exe addresses; `nv-rs-dev hitches
--ledger FILE` names them from a TSV of address and name.

## Slow windows and the run summary

Every ten seconds the 99th percentile frame time of the window is compared with the p99 limit; over it, a `"kind":"perf.slow_window"`
record is written (`from_ms`, `to_ms`, `p50_ms`, `p99_ms`, `fps`, `cause_mix`: the cause of every frame in it over twice the median).
Windows that are mostly a loading screen are not judged.

At the end of every run, `run-summary-<unix>-<pid>.json` is written **in the folder of the JSON log**
(`~/.local/state/nv-rs/logs/`, `%LOCALAPPDATA%\nv-rs\logs` on Windows), one JSON object on one line, schema
`nv-rs-run-summary/1`: `id` (`<unix>-<pid>`, the unix time is the run's start and the same as in the log's own name), `ended`
(`exit`, `stop`, `panic`, `signal:TERM`/`INT`/`HUP`, or `running`), `log` (the JSON log's file name), `meta` (version, build time,
profile, GPU, driver, session, resolution), `duration_s`, `play_s`, `loading_s`, `frames` (frames that are not loading), `frame_ms`
(`avg`, `p50`, `p95`, `p99`, `max`, `fps`, `low1_fps` = 1000/p99, `low01_fps`), `over_pct` (percent over 16.7, 33.4, 50, 100 ms),
`hitches` (`count`: all slow frames; `in_play`: those that are not a load; `per_min`: `in_play` a minute; `slow_windows`;
`loads`: `n` and `ms`, the slow frames of loads and their wall time; `by_cause`), `worst` (five: `frame`, `run_ms`, `wall_ms`, `cause`, `trigger`, `top`: the
sampler's call and hot guest addresses), `peaks` (`rss_mb`, `heap_mb`, `vram_mb`, `gtt_mb`), `soft_stops`, `errors`, `warnings`,
`other_viewers`, `flags`, `verdict`.

The file is written on a clean exit, on a stop, in the panic hook, on SIGTERM, SIGINT and SIGHUP (the process then ends as the
signal would have ended it), and every 30 seconds with `"ended":"running"`, so a SIGKILL or a power cut leaves the last one.
`nv-rs-dev runs` marks a `running` summary whose process is gone as `unclean_exit`.

Flags: `hitches_per_min_high` (only hitches in steady play count: the slow frames of a load, with or without a loading screen,
are load time, reported under `hitches.loads` and never in the rate; a loading screen's frames are also in `loading_s` and
`loading_over_limit`), `p99_over_20ms` (the level is `NV_HITCH_FLAG_P99_MS`), `fps_below_target`, `loading_over_limit`,
`slow_windows`, `soft_stops`, `errors`, `stopped` (a stop or a panic), `killed` (a signal), `unclean_exit`, `noisy_machine` (another
`nv-viewer` process ran, Linux). Rates are not judged before 20 s of frames. `verdict` is one line: `OK: 143 fps, p99 7.1 ms, 0 hitches
(0.0/min)`, or `FLAGGED:`/`FAILED:` with the flags and the worst hitch.

## Reading the logs

- `nv-rs-dev runs [--last N] [--flagged] [--json]` lists the runs from their summaries: fps, p99, hitches a minute, flags, and the change
  against the run before. `nv-rs-dev runs show <id|latest>` prints one in full.
- `nv-rs-dev hitches <run.jsonl>` ranks the causes and triggers by the time they cost, lists the worst hitches with what the sampler
  found, the slow windows and a timeline in ten-second buckets (`--json` for a machine).
- The terminal gets the summary at exit and every minute: counts by cause, worst five with triggers, hitches a minute and the share of
  frames over 16.7, 33.4, 50 and 100 ms.
- MangoHud: `<log>.hitches.csv` has one line per hitch, `unix_ms,run_ms,frame,wall_ms,cause`. MangoHud's own CSV names its start time
  in its file name; add its `elapsed` column (nanoseconds) to that start to compare with `unix_ms`.

## What to send after a stutter

The run's `run-summary-*.json` and the JSON log it names (`log` in the summary), both from `~/.local/state/nv-rs/logs/`, and a note of
roughly when (minutes into the run) and where in the game. Both hold numbers, ids and call names only: no asset names (a file is its
extension and a hash), no paths, no game data.

## What feeds the counters, and what does not

| Counter | Fed from |
| --- | --- |
| `platform`, `platform_calls`, `slow_calls` | every platform call the main thread makes under the kernel or the device lock (`real_threads::locked_call`, `device_call`); a platform call that runs game code that calls another counts once |
| `kernel_wait`, `device_wait`, `kernel_wait_other_threads` | the contended path of the kernel and device locks only (an uncontended lock costs nothing); a main wait over 1 ms is an event `lock:<call the holder was in>` |
| `object_wait`, `sync_wait`, `render_wait` | the kernel-lock wait for objects, the lock-free event and semaphore waits, and the main thread's join with the drawing thread |
| `file`, `file_other_threads`, `file_reads`, `file_bytes`, `file_ops` | `CreateFile`, `ReadFile`, `WriteFile` of the platform layer |
| `sound` | the mixer's per-frame tick on the main thread |
| `draws` | each draw recorded by the wgpu backend (a count; the timing stays `NV_PERF`'s) |
| `pipeline`, `shader`, `texture`, `buffer`, `presents`, `gpu_wait`, `queue_thread` | spans of the wgpu backend: pipeline made on a draw, shader modules made for it, texture and buffer creation and upload, `Present`, read-backs and waits for the queue thread, the queue thread's submit |

Events (trigger context): `cell.change`, `world.change`, `screen.play|menu|loading` (the viewer reads them from guest memory), the game
trace's loading kinds (`menu.loading`, `load.thread`, `loading.begin|end`, `movie.open|close`), `save.write` (a save file written),
`slow:<call>` and `lock:<call>`. **Not available:** quest stages, combat start and door activation. The port intercepts none of those
functions (the same limit LOGGING.md states for the menus); a door shows as `cell.change` with a loading screen, and a quest stage or a
combat start needs a patch row in `engine-core`'s extend hooks, which is an `engine-core` change of its own.

With `NV_PROFILE=1` the sampler names the running guest functions of a thread stuck in the game's own code from the profiler's shadow stack
(the profiler's own cost applies). Without it, a stall in the game's code shows `in_guest` samples and the chain of the last platform call.

## Overhead

### The first measurement

Measured 2026-11-16 on the optimised build (`play-opt`) of this branch before the cost cuts below: `nv-rs-dev bench run newgame`
(start-up, new game, 20 s in the first room) in the nested headless window at 2560x1440, unlimited frame rate, real threads,
`NV_PERF` on, machine-wide bench lock, no other viewer (no run flagged noisy). `NV_HITCH=off` and the default (`on`) alternated,
four runs each; play-screen frames only.

| Run | `NV_HITCH=off` fps | p99 ms | 1% low | process CPU % | `on` fps | p99 ms | 1% low | process CPU % |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | 229.3 | 10.0 | 79 | 79.0 | 214.5 | 10.5 | 74 | 80.2 |
| 2 | 238.0 | 10.7 | 67 | 79.2 | 213.7 | 12.6 | 64 | 80.6 |
| 3 | 203.0 | 8.0 | 88 | 106.8 | 216.9 | 9.8 | 80 | 82.6 |
| 4 | 243.7 | 8.8 | 86 | 80.7 | 213.0 | 12.8 | 63 | 84.9 |
| Median | 233.6 | 9.4 | 82 | 80.0 | 214.1 | 11.5 | 69 | 81.6 |

That was about 0.39 ms a frame (4.28 ms against 4.67 ms mean frame time from the medians), about 8% of the fps against a 1%
target.

### Where it went

The call rate, from the recorder's own counters in the hitch records of those runs: a play frame in the first room makes about
10,100 outermost platform calls on the main thread and about 840 draws. Every such call cost two precise clock reads (`Instant::now`,
about 23 ns each here) and about fourteen stores, so the platform-call bookkeeping alone was about 0.7 ms a frame on a quiet core.
Spans elsewhere (files, textures, buffers, the sound tick, sync waits) paid the same clock pair, at far lower rates.

### What changed

- **A coarse clock.** The hot paths no longer read the precise clock. They read one word: the time that the sampler thread stores
  every millisecond (and each frame end stores). A span is the coarse time that passed during it. For a call shorter than a tick
  that is zero or one tick, as often as the call is long, so the frame's sum is an unbiased estimate: what a 1 ms sampling profiler
  would give, about 2 ms of error on a 30 ms frame, enough to name the cause. A slow call (2 ms or more) is still seen to the
  millisecond.
- **Armed timing for a slow frame.** Bit 0 of the same word is set while the frame in progress is over its budget (twice the
  typical frame, at least 8 ms). Then spans and platform calls read the precise clock and publish the whole guest chain, so a hitch
  is timed and attributed exactly from that point on; a call that began before keeps its function, its innermost caller and a coarse
  start.
- **Fewer stores.** An unarmed call publishes the function, the innermost caller and the start (no chain copy); the argument class
  is one word; the call count is the sampler's sequence word, which each call moves on anyway; the main thread's platform time is a
  single-writer load and store instead of a locked add; the outermost-call test reads the published slot instead of a thread-local
  flag.
- `NV_HITCH=off` stays one relaxed load and a branch on every path. The file name class (an FNV hash of the last name component)
  is computed only in `CreateFile`, a few times a play frame.

### Micro-benchmark

`cargo bench -p fork-hitch --bench hot_path` (std only, no harness; `NV_HITCH=off` for the switched-off path) times each hot path
over 5 million iterations, best of five, in nanoseconds a call. Same machine, same capped build both times; "before" is the
branch's previous commit with the same benchmark file.

| Path | before, `on` | after, `on` | `off` |
| --- | --- | --- | --- |
| `enter_call` and its drop (main thread) | 73.5 | 4.7 | 2.8 |
| `span` with a count (texture create) | 47.3 | 5.7 | 0.7 |
| `span_main` (sound tick, sync wait) | 47.5 | 1.6 | 0.7 |
| `add` (a draw) | 1.7 | 2.1 | 0.4 |
| `note_holder` | 0.5 | 0.5 | 0.2 |

The `off` column is mostly the benchmark loop's own call and argument passing. Against it the recorder now adds about 1.9 ns a
platform call (was about 70 ns): about 19 us for the 10,100 calls of a play frame, 0.45% of a 4.28 ms frame, against about 0.7 ms
before.

### The second measurement

Measured 2026-11-16 on the `play-opt` build of this branch with the cost cuts above, merged with `fork` of the day:
`nv-rs-dev bench run newgame --threads real --nested headless --resolution 2560x1440 --vram-limit-pct 50 --perf`, the same
scenario and settings as the first measurement. `NV_HITCH=off` and the default (`on`) alternated, four runs each, under the
bench lock; no run flagged noisy, every verdict a pass. Play-screen frames only.

| Run | `NV_HITCH=off` fps | p99 ms | 1% low | process CPU % | `on` fps | p99 ms | 1% low | process CPU % |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | 232.1 | 9.8 | 84 | 81.3 | 249.7 | 7.6 | 106 | 84.7 |
| 2 | 247.3 | 7.2 | 110 | 75.1 | 251.8 | 7.4 | 109 | 80.1 |
| 3 | 254.5 | 7.2 | 110 | 85.8 | 255.6 | 7.2 | 111 | 77.6 |
| 4 | 256.7 | 7.0 | 114 | 90.9 | 253.8 | 7.2 | 111 | 83.0 |
| Median | 250.9 | 7.2 | 110 | 83.6 | 252.8 | 7.3 | 110 | 81.5 |

With the recorder on, the median fps is 0.8% higher than with it off, the median p99 0.06 ms (0.8%) higher, and the
median 1% low the same. The difference is inside the run-to-run spread (the off runs alone span 232 to 257 fps), so the
recorder's cost is under the 1% target. The first measurement's 8% is gone.

To repeat it:

```sh
cd ~/Projects/JorisJonkers-dev/worktrees/nv-rs/hitch
export NV_DEV_TARGET=~/.cache/nv-rs/target-hitchovh   # the play-opt build of this branch
for i in 1 2 3 4; do
  NV_HITCH=off nv-rs-dev bench run newgame --threads real --nested headless --resolution 2560x1440 --perf
  nv-rs-dev bench run newgame --threads real --nested headless --resolution 2560x1440 --perf
done
```
