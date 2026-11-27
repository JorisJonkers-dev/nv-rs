# Progress pictures

The fork's progress bars, in the look of upstream's `docs/media/progress.svg`:

<p align="center">
  <img src="../media/fork-progress-engine.svg" alt="Progress of the engine port" width="100%">
  <img src="../media/fork-progress-extenders.svg" alt="Progress of the built-in script extenders" width="100%">
  <img src="../media/fork-progress-features.svg" alt="Progress of the engine's graphics features" width="100%">
</p>

The README shows the coverage catalogue's picture instead (`docs/fork/coverage/tally.svg`, docs/fork/CATALOGUE.md),
which counts tests by system.

| Picture | What it counts |
| --- | --- |
| `docs/media/fork-progress-engine.svg` | The engine port: FalloutNV.exe 1.4.0.525's game functions, split as upstream's bar splits them |
| `docs/media/fork-progress-native.svg` | Native 64-bit code: typed functions (ADR-0014 stage 1) and typed families with a stage-2 store |
| `docs/media/fork-progress-d3d9.svg` | Direct3D 9 on wgpu (Vulkan on Linux and Windows, Metal on macOS): device methods and render states |
| `docs/media/fork-progress-extenders.svg` | The built-in script extenders' commands: tested, built without a test, not built |
| `docs/media/fork-progress-features.svg` | Graphics plan: its capabilities, done, declared without a test, open |

Every number comes from the code and from reference lists read from pinned outside sources. No file holds a count kept
by hand. The pictures are drawn from the summary `docs/media/fork-progress.json` alone. The summary also lists every
name behind a count, such as the commands that are missing or untested, and the device methods that are not answered.

**Nobody edits these files.** After every push to `fork`, the workflow `Progress pictures` (`.github/workflows/fork-progress.yml`)
counts again, with no token. When a file changed, a second job makes a fresh checkout and runs none of the repository's
code. It copies exactly those six files, a fixed list in the workflow, and commits and pushes them with the App's token,
as `chore(progress): ... [skip ci]`.

## What each count is, and what proves it

**Engine port.** This is upstream's function ledger (`scripts/ledger`, run with `--tsv`), the same derivation as
`docs/LEDGER.md`: every function of the engine map gets a status from the code.
- *By hand*: a translation marker in an engine crate.
- *Start-up table* and *shut-down table*: a row of `crates/engine-core/data/static_initializers.tsv` or
  `atexit_destructors.tsv`.
- The total is the game functions. Platform and library code is left out, as on upstream's bar (69,459 when this was
  written).

**Native 64-bit code.**
- *Typed*: translated functions with a registered `// Typed from <addr>` version (the ledger's `typed` column), out of
  the hand translations.
- *Stage-2 store*: typed families declared with `stage2: true` (`pub static NAME: Family = Family { ... }`), out of all
  typed families. A stage-2 store means the family's objects can move to host memory.
- Only the lock-free maps move their objects by default (`NV_TYPED` unset is K2 for them, stage 1 for the other typed families), and no limit of the 32-bit address space is
  claimed from these numbers.

**Direct3D 9 on wgpu.** The full lists come from mingw-w64's `d3d9.h` and `d3d9types.h` at a pinned tag.
- A device method is *answered* when `engine-platform`'s `d3d9device.rs` registers its vtable slot.
- It is *carried to wgpu* when, in addition, a command of `d3d9-stream`'s `commands!` names it in its documentation and
  `d3d9-wgpu`'s backend handles that command.
- A render state is *mapped* when d3d9-wgpu's code (its tests left out) uses its constant (`rs::NAME` or `RS_NAME`) other
  than in a "not provided" warning.
- It is *warned* when every use is such a warning, and *ignored* when no code names it.
- This measures the Direct3D 9 layer on wgpu. It does not measure a renderer native to Vulkan.

**Script extenders.**
- The reference is the list of commands each extender registers. It is read from the extender's own source at a pinned
  commit (`scripts/fork/progress/src/pins.rs`):
  - xNVSE: the calls of `CommandTable::AddCommandsV*`;
  - JIP LN, JohnnyGuitar, ShowOff: their `REG_*` calls, with JIP's and ShowOff's opcode comments;
  - kNVSE: `REG_CMD` and `builder.Create`.
  - Code behind `#if _DEBUG` and the like is left out.
- lStewieAl's Tweaks and the TTW plugin have no public source. Their slot count comes from `.fork/capabilities.json`, and
  the picture says so.
- *Built*: the command is in the real command table (`.fork/capabilities.json`, written from the tables by
  `scripts/fork/release-files`; its test fails while the file is stale). Names are compared without case, as the game
  compares them.
- *Tested*: a test file of the crate that builds the command names it, by its exact name as a word or by its opcode as a
  hexadecimal literal, comments left out. A test file is a `*_tests.rs`, `tests.rs` or `tests/` file, or the inline
  `#[cfg(test)] mod`.
  A file that names more than 200 of one extender's commands counts as a list and tests none of them. The tests must
  pass for a pull request to merge; the tool does not run them again.

**Graphics plan.** (The engine features players and modders know, LOD drawing, V.A.T.S. or save games, are the
catalogue's `features` area: docs/fork/ENGINE-FEATURES.md.)
- The reference is the graphics plan's capability list, `docs/fork/graphics-features.tsv` (id, group, name). It changes
  only when the plan changes.
- What is built is declared in code: `crates/fork-gfx/src/atlas.rs` maps each built-in setting (`module.setting`) to the
  capability ids it implements. Its tests refuse a key that is not a setting and an id the plan does not list.
- *Done*: a setting declares the capability, and a test names that setting, either as `module.setting` or as the
  setting's quoted name in a test that also names its module.
- A new setting that implements a capability adds its row to `atlas.rs` in the same pull request.

## The checks on a pull request

The pipeline's job `Progress counts` (in `fork-ci.yml`, part of `Pipeline Complete`):
- runs the generator's tests;
- counts the merged tree twice, and fails when the two runs write different bytes or a reading fails, for example a
  pinned source that no longer parses;
- compares every count with the base branch's summary, the reference lists' lengths included, so a reader that
  suddenly reads a list short shows. A count that goes down fails the job, unless the pull request description has a
  line `Progress-Drop: <why>`. The drops of the upstream replay (this repository's own `sync/upstream`) are only
  reported;
- lists, in the run summary, the built commands that have no test.

## Running it

```sh
cargo run --release --manifest-path scripts/ledger/Cargo.toml -- --tsv /tmp/ledger.tsv && git checkout -- docs/LEDGER.md
tool="cargo run --release --manifest-path scripts/fork/progress/Cargo.toml --"
$tool fetch --sources /tmp/progress-sources      # the pinned sources, shallow and sparse
$tool generate --sources /tmp/progress-sources --ledger /tmp/ledger.tsv
$tool check --sources /tmp/progress-sources --ledger /tmp/ledger.tsv --baseline docs/media/fork-progress.json
```

Don't commit what `generate` writes: the workflow commits it after the merge. To move a pin, change its commit in
`pins.rs` (one pull request per pin). The counts it changes show in `Progress counts`.

## The progress App

The push to `fork` is made by a GitHub App of its own, `raul-progress-bot`:
- permissions: contents write and metadata read;
- installed on the six Raul repositories only;
- the only bypass actor of the ruleset "Fork line", and of no other rule or ruleset of nv-rs (on another repository only
  where `--bypass` names that ruleset, see below).

Its id is the Actions variable `PROGRESS_APP_ID` and its private key the Actions secret `PROGRESS_APP_PRIVATE_KEY`.
`scripts/fork/apply-ruleset.sh` keeps the bypass when it applies `.github/rulesets/fork-line.json`.

A repository owner sets it up once, with a browser logged in to GitHub:

```sh
scripts/fork/setup-progress-bot.py
```

The script shows each step and asks before it runs it. It skips what is already done, so it is safe to run again.

Other Raul repositories publish their tally reports (the feature catalogue: implemented and covered per area) the same
way, from their default branch. Where that branch has a ruleset that stops the push, the App becomes a bypass actor of
that ruleset only when asked, one repository at a time:

```sh
scripts/fork/setup-progress-bot.py --bypass loot-rs:Main
```
Without the App, the workflow still counts and shows the result in its summary, but pushes nothing.
