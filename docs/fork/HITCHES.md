# Hitch tracing (`fork-hitch`)

A flight recorder that is on by default in real play. When a frame is far over the usual one it writes one line to the JSON
log that says what the frame was spent on, what led up to it and which function was running; at the end of every run it
writes a small summary file with flags, so a stutter can be traced after the fact and a bad run is noticed without anyone
pointing at it. Fork-owned (`crates/fork-hitch`, std only apart from `fork-log`); the hot path is relaxed atomic adds and
one store per frame, nothing allocates there.

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
| `loading` | the screen is a loading screen, or a loading event is in the preceding 500 ms |
| `gc/alloc` | 10 000 page faults (about 40 MiB of fresh pages) or 32 MiB of resident growth in the frame, and the faults cost a fifth of it |
| `file_io` | file calls on the main thread hold a third of the frame |
| `pipeline_create`, `shader_compile` | pipeline or shader-module creation holds a third |
| `texture_upload` | texture and buffer creation and upload hold a third |
| `lock_contention` | waiting for the kernel lock holds a third |
| `render_wait`, `device_wait` | waiting for the render/drawing thread, or for the device lock and the GPU, holds a third |
| `game_logic` | none of those, and the main thread ran the game's code for over half the frame |
| `unknown` | the rest, usually time inside a platform call no bucket names (`platform_other_ms` in the record, and the sampler's call name) |

### The sampler

The main thread publishes, at each platform call, which call it is in, the chain of guest functions that made it and the class of
its argument (a file's extension and a hash of its name, never a path). A sampler thread reads it every 2 ms while the frame in
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
`hitches` (`count`, `per_min`, `slow_windows`, `by_cause`), `worst` (five: `frame`, `run_ms`, `wall_ms`, `cause`, `trigger`, `top`: the
sampler's call and hot guest addresses), `peaks` (`rss_mb`, `heap_mb`, `vram_mb`, `gtt_mb`), `soft_stops`, `errors`, `warnings`,
`other_viewers`, `flags`, `verdict`.

The file is written on a clean exit, on a stop, in the panic hook, on SIGTERM, SIGINT and SIGHUP (the process then ends as the
signal would have ended it), and every 30 seconds with `"ended":"running"`, so a SIGKILL or a power cut leaves the last one.
`nv-rs-dev runs` marks a `running` summary whose process is gone as `unclean_exit`.

Flags: `hitches_per_min_high`, `p99_over_20ms` (the level is `NV_HITCH_FLAG_P99_MS`), `fps_below_target`, `loading_over_limit`,
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

## Status

The recorder, the frame ring, the classification, the sampler, the run summary, slow windows and the tools are in. The counters the
frame breakdown is made of are fed from the places that know them (platform calls, locks, file reads, the wgpu backend's spans);
until a counter's site is wired it reads zero and a frame's time falls to `game_logic` or `unknown`. Overhead is measured with
`nv-rs-dev bench run` with `NV_HITCH=off` against the default, see the bench section when it lands.
