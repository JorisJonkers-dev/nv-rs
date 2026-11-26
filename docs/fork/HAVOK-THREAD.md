<!-- SPDX-License-Identifier: GPL-3.0-only -->
# Havok's thread and the `bhk` seam (nv-rs#944)

Fork-only. Havok is the first component to leave the game thread (ADR-0014 stage 2, ADR-0015).
The two changes here are the seam and the stepped-mode step:

- `engine_core::bhk` is the only way code outside Havok reaches a Havok field. `get`, `set`, `at`
  and `byte_add` are the wrappers the translations use today; when Havok's objects move to native
  layouts, those four functions change and their callers do not. `bhk::step` is the only way the
  game's `bhkWorld::Update` calls the SDK's step.
- With stepped threads, `bhk::step` posts the step to one `nv-havok` OS thread and joins it before
  returning. The poster keeps the step's place in the call order, its words, its x87 control word
  and its virtual clock; no other game thread runs while the job is in flight. A stop inside the
  step is resumed at that join with the calls the helper unwound through put back on the poster's
  call stack, so the stop names the same functions as an inline call. The `bhk_thread_stepped` test
  compares the stop's text and call stack in both paths. The helper uses its own slot of
  `HELPER_AREA`, so it takes no guest heap block. With real threads, a per-call diagnostic, a typed
  family running dual, or a helper already busy, the step stays on the calling thread. Real threads
  are untouched: the game's own Havok workers still run there.
- `NV_HAVOK_THREAD=0` keeps the step inline for an A/B run. It is read once at start-up; the default
  is on. It is a diagnostic, not a preset switch, so it makes the run a test run.

The census from nv-rs#943 (`docs/fork/COMPONENT-CENSUS.md`) names the Havok-owned fields. The
`bhk_seam` test reads this repository's sources and fails if a function outside Havok and `bhk`
uses `e.get`, `e.set`, `Ptr::at` or `Ptr::byte_add` on a Havok type. The `bhk_thread_stepped` test
builds a stand-in step and checks that the helper's call has the same words, a control word taken
from the poster and the same virtual clock as an inline call, that no job is left posted, that a
per-call diagnostic keeps the step inline, and that a stop in the step has the same text and call
stack in both paths. Both tests use made-up inputs. There is one helper per process, for the run
that first posts: a second run's post stops, as `post_draw`'s does.

## Gate left to the maintainer

No game runs were available when this changed. Run two stepped recordings on the maintainer's
machine, one with the helper and one with the inline control, and compare the streams byte for
byte. `NV_REPEATABLE` forces stepped threads whatever `NV_THREADS` says, so this is the same
comparison the draw thread uses (`docs/PLAYTESTING.md`, "Running and pacing"). The
`NV_HAVOK_THREAD=0` run is the reference (the step inline on the game thread); the `=1` run is the
candidate (the step on `nv-havok`):

```
export NV_REPEATABLE=300 NV_SOUND=out
NV_HAVOK_THREAD=1 NV_D3D9_RECORD=/tmp/havok-on.stream nv-rs-dev run
NV_HAVOK_THREAD=0 NV_D3D9_RECORD=/tmp/havok-off.stream nv-rs-dev run
cmp /tmp/havok-on.stream /tmp/havok-off.stream
```

Both ends of the comparison must be byte-identical, with `NV_D3D9_RECORD` written in each run.
`NV_SOUND_RECORD` can be compared the same way when the sound device is unchanged. A difference is
a stop: check the step's words, the x87 word and the clock hand-off before using the thread.

## Race campaign for Havok's real thread

The stepped reference stays the permanent mode for tests, recordings and traces (ADR-0015). Havok's
own threads become the default only after this campaign passes. Each phase keeps the evidence under
its label; a failure is triaged before the next phase starts.

1. **Stepped reference.** The two recordings above agree byte for byte for the same number of
   frames and the same scripted input. The `bhk` seam test and the Havok thread test pass. The
   `bhkworld` update tests, which keep diagnostics inline, pass.
2. **Shuffled schedules.** On the stepped host, run the same scene at seeds
   `NV_THREADS=shuffle:1` through `NV_THREADS=shuffle:20` (a seed reproduces an order, so a failure
   can be replayed). Run the same scene again with `NV_THREADS=real`, where the game's own Havok
   workers and the other components run side by side; the parser rejects a `real,shuffle:SEED`
   combination, so the two modes are separate runs. Run with `NV_RACE_CENSUS=/tmp/havok-races.tsv`
   where the census is needed and record every reported race between Havok's threads and the
   actors', scene and form readers. Fix a race in the `fixes` preset when the original has it too; a race the
   port adds is a stop.
3. **Soaks.** Run `nv-rs-dev scene soak --places docs/fork/scenes/soak-far-moves.toml --out /tmp/havok-soak --frames 300 --game-lock "$GAME" --label havok-real` and the same soak with `docs/fork/scenes/soak-rivet-ne.toml`. A pass is every place reached, exit status 0,
   zero stops and zero error lines; frame times from a soak are not a speed measurement. Repeat on
   Linux and Windows before calling a platform done.
4. **Distribution against the original.** Record several runs of the same route with
   `NV_THREADS=real`, where Havok uses the game's own workers; compare the states the game exposes
   (actor positions, ragdoll settling, collision results) as distributions over the runs, not as
   byte equality. The stepped recording with `NV_HAVOK_THREAD=1` remains the reference for anything
   that must not move.
5. **Decision.** Real Havok threads go on by default only when phases 1 to 4 pass and the
   distribution check shows no new tail. Until then the stepped helper above is the only change;
   the game's own Havok workers keep running as they do today in `NV_THREADS=real`.

The catalogue rows `real_threads_default`, `stage5_gates` and `race_soak_linux`
(`.fork/catalogue/features/threading.toml`) are updated by the lead when the campaign has evidence.
