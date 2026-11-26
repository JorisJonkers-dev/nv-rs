<!-- SPDX-License-Identifier: GPL-3.0-only -->
# The loader and garbage collector jobs (nv-rs#941)

Fork-only. The main loop's three per-frame calls — `IOManager::UpdateQueue` (`00c3dbf0`, which walks
the model loader's queued references and finishes queued loads), `GarbageCollector::Update`
(`00868850`) and `GarbageCollector::ClearTempEffects` (`00868d10`) — go through
`engine_core::loader_jobs`.

With stepped threads and `NV_LOADER_JOBS=1`, each call is posted to the stepped component helper (the
one Havok's step uses) and its poster joins before the next game call. The job runs on a view of the
same guest memory with the poster's thread id, TLS values, x87 control word and virtual clock, so the
garbage collector's critical section and the model loader's lock keep the same owner and order as in
an inline call. The join is the poster's: no game thread runs between a post and its place in the
call order, and the uploads the loader finished are done before the frame's draw. A stop in a job
unwinds at the join with the helper's stopped calls put back on the poster's stack. The helper area's
slots past the pool stay the Havok helper's and the drawing helper's; the loader and Havok jobs
share the one helper, so no slot is taken.

`NV_LOADER_JOBS` is read once at start-up. The default is off, so `vanilla` and `fixes` keep their
reference path. The variable is a test variable (`host::fixes::TEST_VARIABLES`, mirrored in
`game_profile::game_ini::TEST_SWITCHES`): a run that sets it is a test run, which without an
`NV_PRESET` runs vanilla, and a launcher's play run with it exported is taken as a test and reads no
`Fallout.ini` keys. Set `NV_PRESET` yourself when a comparison must pin the preset; the variable
then still decides whether the jobs run. With real threads, a per-call diagnostic, a typed family running dual, a busy
helper or a memory that cannot be shared, the call stays on the game thread. Real threads for these
jobs stay off by default until the race campaign below passes (ADR-0015). The job body forwards the
same words to the same translated function and adds no float arithmetic; typed code moved here later
must use f64 locals rounded at each f32 store (ADR-0014).

`crates/engine-core/tests/loader_job_stepped.rs` builds its own stand-ins and scratch memory. It
checks that the three calls and the upload stay in the same order, with the same words and control
word on both paths, that the jobs run on one helper OS thread that is not the poster's, that a job
already posted keeps the helper so the next call runs inline before the posted job's join, that no
job is left posted at the draw, that a diagnostic keeps the call inline, that a stop has the same
text and call stack on both paths, and that the Havok step uses the same helper thread. The rounded
f32 store is written by the test's stand-in, so it shows the two paths hand over the same bytes, not
that any translated float code is right.

## Stepped recording comparison

`NV_REPEATABLE` forces stepped threads, so this compares the same schedule with the jobs on and off.
The `=0` run is the reference, the `=1` run the candidate. Both streams must be byte-identical.

```
export NV_REPEATABLE=300 NV_SOUND=out
NV_LOADER_JOBS=1 NV_D3D9_RECORD=/tmp/loader-on.stream nv-rs-dev run
NV_LOADER_JOBS=0 NV_D3D9_RECORD=/tmp/loader-off.stream nv-rs-dev run
cmp /tmp/loader-on.stream /tmp/loader-off.stream
```

`NV_SOUND_RECORD=/tmp/loader-on.sound` and `=/tmp/loader-off.sound` in the same two runs gives the
same comparison for sound. A difference stops the change: check the call order, the x87 word and
the clock hand-off before using the helper.

## Race campaign before the switch goes on by default

Stepped mode stays the reference for tests, recordings and traces (ADR-0015). Keep each phase's
evidence under its label; triage a failure before starting the next phase.

1. **Stepped reference.** The two streams above agree byte for byte for the same frames and input;
   `loader_job_stepped` and the translated units' loader and collector tests pass.
2. **Shuffled schedules.** Run the same scene at `NV_THREADS=shuffle:1` through
   `NV_THREADS=shuffle:20` with `NV_LOADER_JOBS=1`. With `NV_THREADS=real` the jobs stay inline, so
   a real-thread run exercises nothing of them: real threads for these jobs are not implemented and
   are the work that comes before phases 3 to 5. Use `NV_RACE_CENSUS=/tmp/loader-races.tsv` where the census is needed and
   record every race between the loader's locks and the other components' readers. A race the
   original has belongs in `fixes`; one the port adds is a stop.
3. **Soaks.** Run `nv-rs-dev scene soak --places docs/fork/scenes/soak-far-moves.toml --out /tmp/loader-soak --frames 300 --game-lock "$GAME" --label loader-jobs` and the same with
   `docs/fork/scenes/soak-rivet-ne.toml`, with `NV_LOADER_JOBS=1` (stepped). A pass is
   every place reached, exit status 0, zero stops and zero error lines. Soak frame times are not a
   speed measurement. Repeat on Linux and Windows before calling a platform done.
4. **Distribution against the original.** Once real threads for these jobs exist, record several
   runs of the same route with `NV_THREADS=real` and compare the states the game exposes as
   distributions, not byte equality. The stepped recording above stays the reference for anything
   that must not move.
5. **Decision.** Real threads for these jobs go on by default only when phases 1 to 4 pass and no
   new tail appears. Until then `NV_LOADER_JOBS` stays off by default and the helper is opt-in.

## Cell-border and loading hitch measurements

Measure before and after on the same build, alternating the two settings (ABBA): `NV_LOADER_JOBS=0`
is before, `=1` after. The walk place streams cells in as a walking player does; a `cell.change`
inside a worldspace is play, while loading screens and world changes are loads (`docs/fork/HITCHES.md`).

```
nv-rs-scene bench --lock GAME.LOCK --viewer BIN --game DIR \
  --places docs/fork/scenes/bench-walk.toml --out /tmp/loader-after \
  --frames 300 --warmup 30 --passes 5
NV_LOADER_JOBS=1 NV_HITCH=on NV_HITCH_CSV=/tmp/loader-on.csv nv-rs-dev run --label loader-after
NV_LOADER_JOBS=0 NV_HITCH=on NV_HITCH_CSV=/tmp/loader-off.csv nv-rs-dev run --label loader-before
nv-rs-dev runs --json
nv-rs-dev hitches /path/to/run.jsonl
```

Record each ABBA pair (A and A' are `=0`, B and B' are `=1`):

| Run | `NV_LOADER_JOBS` | `hitches.loads` | worst `cell.change` frame | `by_cause` note |
| --- | --- | --- | --- | --- |
| A (before) | 0 | | | |
| B (after) | 1 | | | |
| B' (after) | 1 | | | |
| A' (before) | 0 | | | |

No game runs were available for this change, so the cells stay blank until the maintainer fills
them from `nv-rs-dev hitches`. Keep the run summaries' `hitches.loads` and `by_cause` and the worst
`cell.change` frames from each run. A gain outside the run-to-run spread is evidence; one inside it
is not. The catalogue rows
`real_threads_default`, `stage5_gates` and `race_soak_linux`
(`.fork/catalogue/features/threading.toml`) are updated by the lead when the campaign has evidence.
