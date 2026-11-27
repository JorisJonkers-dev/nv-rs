# Translating the world draw on worker threads (nv-rs#939): where the time is, and what blocks it

Fork-only. Measurement note for nv-rs#939 (parent #916, after #938). It records what the detached world draw costs
today, so the worker design is built against numbers, and why one step is not landed yet.

## What exists

The world draw is already translated to device calls off the game's main thread, as one job: `NV_DRAW_THREAD=1` posts
both halves of the world draw (`00b6ba20`, `00b6b930`) to the `nv-draw` helper with a value packet, and the device's
command stream is cut so that the helper's commands are spliced in at the cut (`engine_core::draw_thread`,
`engine_platform::d3d9draw`, docs/FIXES.md "World drawn on a second thread"). After #938 (PRs #996, #1001) no join
falls inside the world pass. There is one helper and one job at a time.

## Measured (2026-11-27, `nv-rs-dev scene bench --suite quick`, real threads, `NV_STAGE_TIME=1`)

The machine was shared with five other workers (load average 8.5 on 8 cores), so frame rates are not a result; the
ratios inside one run are. Switches of the second run: `--env NV_RENDER_THREAD=1 --env NV_DRAW_THREAD=1
--env NV_DRAW_THREAD_CHECK=1`.

| | serial draw (no switch) | draw thread |
|---|---|---|
| main, "world draw" stage (the cut, or the whole draw) | 5.8 ms | 0.9 ms |
| main, "after world" stage (the join wait) | 0.7 ms | 9.6 ms |
| the job on the helper (1200 posts) | - | 9.45 ms a frame |
| the packet's close at the join | - | 0.16 ms a frame |
| avg fps, goodsprings / novac | 64 / 64 | 56 / 57 |

- Every join is at `00b63a90`, the world accumulator's check, which `Main::Swap`'s draw sequence reaches right after
  the world pass: main waits for 9.6 of the job's 9.45 ms (join wait 11 520 ms over 1200 joins, job 11 341 ms). Main
  does about nothing between the post and the join, so nothing overlaps: the draw thread moves the draw's time from the
  "world draw" stage to the join, and the frame's critical path is the job.
- The job takes longer on the helper (9.45 ms) than the same draw takes on main (5.8 ms): the helper runs beside the
  render thread and main under load, takes the device lock per call, and starts with cold caches. Not yet split into
  causes.
- Even with `00b63a90` answered without a join (it only sums the batch renderers' group counts at +0x54), the next
  joins are `00874920` (refraction normals, water scenes) and `00875110` (the screens and overlay accumulate into the
  world accumulator), a few hundred microseconds of main work later.

## What follows for #939

The acceptance "main's draw share falls" is met by the cut (5.8 to 0.9 ms) but not "the frame gets shorter": the job is
the critical path. Only two things shorten it: (1) split the job across workers, (2) let main overlap more of the frame
with it (the rest of the draw sequence moved to the drawing side, `measure/draw-thread/PLAN-B.md`, and the accumulator
joins above removed).

Splitting across workers needs, none of which exists:

1. One value packet per worker (the pass objects, shader statics and Group C entries are own blocks of one packet; a
   chunk that begins a pass writes them, so chunks cannot share one), merged at the join in chunk order.
2. One device cut slot per worker: `DrawSide` holds one drawing side's copy of the device state, so the device model
   would hold N copies, each starting from the state the previous chunk leaves. That state is only known after the
   previous chunk ran, unless every chunk starts with a full state set-up (more commands than the serial stream, so the
   byte-identical gate cannot hold; the device's repeat filter would have to remove them at the splice).
3. Order-sensitive lists (alpha lists, shadow and light passes) stay serial.

This is the same cut a recorded draw item makes (#1004, PR #1011: `RecordNext`/`DrawItem` replay a draw from recorded
inputs). A draw item is exactly a chunk that carries its own inputs, so the workers should replay items rather than
run the typed draw code on N packets: #939's worker pool belongs on top of #1011's items, not beside it. #1011's
`NV_DRAW_ITEMS=check` still differs on the fork (100 906 of 799 424 draws), so the items are not ready to be the unit
of work.

## Order of work

1. Find why the job is slower on the helper than on main (device lock waits, `NV_PERF` thread busy) and fix it: that
   alone should turn the draw thread from a loss into a neutral change.
2. Remove the accumulator joins above (`00b63a90`'s count, `00874920`, `00875110`) so main overlaps the tail.
3. After #1011's check passes: replay items on a worker pool, one cut slot a worker.
