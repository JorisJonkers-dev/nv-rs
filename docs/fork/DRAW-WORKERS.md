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
- The job takes longer on the helper (9.45 ms) than the same draw takes on main (5.8 ms): the cause is not known
  (suspects: the device lock taken per call beside the render thread, load on a shared machine, cold caches); it has
  not been split.
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

## Step 2: the accumulator joins after the world pass (nv-rs#939)

Census (read in the typed draw, `draw::drivers`, `draw::cut`, `draw::detach`, and the translations main runs there), for
the three joins the measurement named.

| Join | What main reads or writes after the cut | What the posted draw also touches | Conversion |
| --- | --- | --- | --- |
| `00b63a90` (the accumulator's check, `main_p2` `00870bd0`) | the 16-bit count of register group 6's holder (renderer +0x54, holder +2), summed over the renderers | the cut took every register group holder's words (`detach::candidates`, slots +0x3c to +0x74) and left zeros in guest memory, so main would read 0 where the serial order reads the count. The draw never reads or empties group 6 (`drivers::draw_group` draws 0, 1, 2, 3, 9, 10; `draw_types` with no group walks the renderer's own pass groups) | group 6 is not taken (`detach::MAIN_ONLY_GROUPS`): its holder, list and renderers stay in guest memory, main's. **The join is gone** |
| `00874920` (refraction normals, only when the check is non-zero) | `00b64570`: draws group 8 and group 6 (`00b639e0`, `draw_types`) into the normals texture and empties them (`00b9a490`); writes the accumulator's camera (+8) and restores it; render target and renderer clear | `draw_types` writes the accumulator's cursor arrays (+0x17c to +0x188) on every call, the same arrays the draw writes; the renderer's target group and mode words (+0x200 to +0x208, +0x888) | **kept**. Making the cursor arrays an own block (`cut::own_cursors`, the cursor accessors through the packet) was tried: the cut stopped in the quick suite's frame 121 under `NV_DRAW_THREAD=poison` (`read of unmapped memory at 73f90136` in `00873200`), where the unchanged code runs through; and the renderer's target words stay shared, the known limit of the water scenes. Not landed |
| `00875110` (the rest of the draw) | accumulates the first-person screens and the overlay node into the world accumulator (`this+0x88`, `00b6c0d0`, `ACCUMULATE_SCENE`) and draws them | registering a pass can add a pass type to the renderers' pass map (+0x10 to +0x18), grow the pass group array (+0x08) and change the active pass list, all of which the job reads and none of which is an own block; the list words main fills are new ones (the cut left zeros), which the join keeps (`give_lists_back`: main's non-zero word wins) | **kept**. Removing it needs the pass map, the group array and the active list as own state with a merge that keeps main's added types; not done |

Result (2026-11-28, `nv-rs-dev scene bench --suite quick`, 1500 frames, recorded with `NV_REPEATABLE=1500 NV_SOUND=out`,
origin/fork at the merge of #1067 as the baseline; each end run twice, control byte-identical):

- No switches and `NV_DRAW_DETACH=1`: byte-identical to the baseline (525 444 566 bytes).
- The stepped draw thread (`NV_THREADS=stepped NV_DRAW_THREAD=1 NV_DRAW_THREAD_CHECK=1`), with and without
  `NV_DRAW_DETACH=1`: byte-identical to the baseline's (525 815 156 bytes; 1201 posts, 1200 joins). The join counter:
  baseline `joins by point 00b63a90:1200`; now `00875110:1200`, none at `00b63a90`.
- `NV_DRAW_THREAD=poison`: no stop, the same stream as the unpoisoned run.
- Real threads (`NV_RENDER_THREAD=1 NV_DRAW_THREAD=1 NV_DRAW_THREAD_CHECK=1 NV_STAGE_TIME=1`, 3000 posts, ABBA, two runs
  each): no stop, `misses 0`, `written 0`. Main's wait at the join: baseline 26 000 and 25 874 ms over 3000 joins
  (8.67 and 8.62 ms), now 26 201 and 26 079 ms (8.73 and 8.69 ms); the job 25 678 and 25 546 ms against 26 201 and
  26 079 ms. The wait does not fall: the join only moved from `00b63a90` to `00875110`, a few hundred microseconds of main
  work later, and the job stays the frame's critical path. The step removes a join that blocked nothing else; the gain
  comes with the next one.

