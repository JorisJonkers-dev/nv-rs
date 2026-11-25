# Benchmark runs

Benchmarks of the game are made by the Rust scene tool, `nv-rs-scene bench` (also `nv-rs-dev bench` and
`nv-rs-dev scene bench`): one engine start visits the places of a built-in suite (`quick`, `mojave`, `soak`, `capital`) on the real
clock under the exclusive game lock, with a warm-up and five passes whose median is kept, and writes `summary.json`,
`suite.md` and the numbers per place. The guide, the suites, the baselines and the noise figures are in
[SCENE.md](SCENE.md), section "Bench". `nv-rs-dev bench clear-halt` removes the memory guard's halt file
(`nv-rs-scene guard status` says whether runs are halted).

## The memory guard (every start)

A bench run once started while the card's memory was nearly full; the driver then refused the desktop's own command
submissions and the session crashed. So every engine start of the tool (`run`, `soak`, `sweep`, `bench`, `replay`):

- runs in a `gamerun` scope with a hard `MemoryMax` and no swap (`--mem`, `--no-cap`);
- is refused before it starts when the halt file exists, or when the card's free VRAM or GTT cannot hold the run's budget (a
  quarter of the VRAM, 0.4 of the GTT: `NV_GUARD_GTT_SHARE` sets another share, at most 0.5) plus a margin kept for the desktop (2048
  and 1024 MiB); the engine gets the VRAM budget as
  `NV_GPU_BUDGET_MB` and refuses allocations past it;
- is watched every 100 ms: it is stopped when its own memory on the card (the DRM clients of its processes, read from
  `/proc/<pid>/fdinfo`) reaches 90% of its cap, or when the card comes within the desktop's margin of full whoever filled
  it; another run on the card does not count against it. Where the system gives no `fdinfo`, the card's use over its use at
  the start counts, as before;
- is stopped, once a second, when the session is in trouble: a GPU reset or an out-of-memory command submission in the kernel
  log, the desktop's compositor gone, or the engine's own refusal of an allocation past its budget;
- halts every later run when the guard stopped it (the halt file `~/.cache/nv-rs/bench.halt`, fixed, no override) until
  `nv-rs-scene guard clear-halt`.

The code is `crates/fork-scene/src/guard.rs`; its tests use a fake card and a fake kernel log.

## What was retired

The Python harness (`scripts/fork/bench/`, `nv-rs-dev bench run|suite|list|report|analyze|watch|record|replay`, the nested
window session and the real-input driver) is gone. Its scenarios that played the game with real keys and the mouse (a new game
through character creation, Doc Mitchell's house from the first start, combat, save and Continue, the sneak check, the
20-minute walk) have no Rust counterpart: the tool places the player itself. Places, far moves, walking (`path`) and interiors
(`cell`) are covered. The release test's steps that needed the real-input scenarios are done by hand (RELEASE-TEST.md). Old
measurements in other documents name `nv-rs-dev bench run <scenario>`: they are records of what was measured then.
