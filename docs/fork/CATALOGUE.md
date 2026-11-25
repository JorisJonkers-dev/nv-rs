# Coverage catalogue

The catalogue in `.fork/catalogue` lists what the engine reimplements and what the fork adds. For each item it says
whether it is built, and which tests prove it. [tally](https://github.com/JorisJonkers-dev/tally) reads it. Its file
format is tally's [docs/FORMAT.md](https://github.com/JorisJonkers-dev/tally/blob/main/docs/FORMAT.md).

## Areas

| Area | Groups | Items | What "implemented" means | Evidence `tally check` compares |
| --- | --- | ---: | --- | --- |
| `extenders` | xNVSE, JIP LN, JohnnyGuitar, ShowOff, kNVSE, lStewieAl's Tweaks, TTW plugin | 2,468 | the built-in copy answers the command | `.fork/capabilities.json` |
| `d3d9` | device methods, render states | 222 | a method is carried to wgpu (partial: only answered); a state is mapped | `progress catalogue --evidence` |
| `gfx` | the graphics plan (`docs/fork/graphics-features.tsv`) | 92 | a built-in setting declares it in `crates/fork-gfx/src/atlas.rs` | `progress catalogue --evidence` |
| `engine` | every function of FalloutNV.exe in the ledger | 69,488 | translated, or run from the start-up and shut-down tables | the ledger (`scripts/ledger --tsv`) |

The engine's functions are an area of their own and never the headline. The aim is a tagged test for every function
over time. Platform and runtime-library code is n/a.

## Files

- `ref/*.tsv` and `sources.toml` are **generated** by `progress catalogue` (scripts/fork/progress). The extender and
  Direct3D 9 lists are read from the sources pinned in `scripts/fork/progress/src/pins.rs`, the same sources the
  progress pictures use. `sources.toml` records the SHA-256 of each list and of every upstream file it was read
  from. The graphics list comes from the plan, and the engine list from the ledger. The pull-request check fails when
  one of these files differs from what the generator writes. Do not edit them by hand. To move a pin, change
  `pins.rs` and run `task catalogue`.
- `<area>/<group>.toml` are **kept by hand**. They say what is built: `items_from` lists every row as an item with
  the group's default status, and an `[[item]]` states each exception. When a pull request builds a command,
  setting or method, it changes that item's status in the same pull request, because `tally check` fails when the
  evidence and the catalogue disagree.
- `tally_ids_gen.rs` is tally's generator, copied by `tally ids --generator .fork/catalogue/tally_ids_gen.rs`.
  `crates/fork-catalogue`'s build.rs includes it.

## Tagging tests

A test in a fork-owned crate calls `covers` with the typed ids of the items it truly runs and checks:

```rust
use fork_catalogue::{covers, ids};

#[test]
fn melee_and_ranged_follow_the_weapon_type() {
    covers(&[ids::extenders::showoff::ISWEAPONMELEE, ids::extenders::showoff::ISWEAPONRANGED]);
    // ...
}
```

The crate needs `fork-catalogue = { path = "../fork-catalogue" }` under `[dev-dependencies]`, and its directory
must be in the scan paths of `.fork/catalogue/tally.toml`. An id that does not exist does not compile. Do not tag
tests that only check registration, opcode or name tables. A test that names more than ten ids needs a
`// covers-table: <reason>` comment.

Tests in files upstream owns are not edited. They are tagged from `.fork/catalogue/covers.d/*.tsv`, one file per
extender or topic, rows of `test<TAB>covers`: `test` is `<file>::<function>`, `covers` the full ids, separated by
spaces. A row names only the commands the test calls and whose answer or effect it asserts. A command that is only
called to set up the case is left out. `tally check` fails when the function is not a `#[test]` in that file.

## States and reports

tally's states are missing, implemented, tagged, passing, and covered (a tagged test passed on Linux, Windows and
macOS). An item that needs game data is gated, and n/a or deferred items are left out of the count. The
pull-request check prints the report by group in the run summary.

## Results

Every component job (`.github/workflows/fork-build.yml`) and the macOS job keep their `cargo test` output as an
artifact (`testlog-<os>-<job index>`; the macOS job's is `testlog-macos`). On a push to `fork`, the schedule and a manual run, when every component runs, the
Pipeline job "Catalogue results" reads them all with `tally ingest --libtest` (the system comes from the file name)
and runs `tally check` with those results and a ratchet: the covered and tagged counts may not drop below the
report of the commit before. Pull requests run only the components a change reaches, so they keep the check without
results. On a push the job renders `docs/fork/coverage/FEATURES.md`, `tally-report.json` (areas and groups) and
`tally.svg`, and the job "Catalogue reports" commits those three files as raul-progress-bot. Nobody edits them.

Covered needs a pass on macOS too. On pull requests the macOS job tests only `fork-window`, `d3d9-wgpu` and
`engine-platform`. On a push to `fork`, the schedule and a manual run it also tests the packages the catalogue tags
(`nvse`, `jip`, `jg`, `showoff`, `ttw-nvse`, `fork-catalogue`, `fork-gfx`) into `test-macos-tagged.log` in the same
artifact. A macOS job of its own, "macOS (tagged engine tests)", runs on the same events the engine's test files that
`covers.d/engine_typed.tsv` names, into `test-macos-engine.log`, and "Linux (tagged engine tests)" does the same on
Linux into `test-linux-engine.log`, because the component jobs run the engine's test files on Windows only.
"Catalogue results" fails when any of these logs is missing, so a lost upload cannot pass the ratchet.

## Running it locally

`task catalogue` builds the ledger table, fetches the pins, rewrites the generated files, writes the evidence to
`target/catalogue/` and runs `tally check`. Download the tally binary named in `.github/workflows/fork-ci.yml`
first, and check it against its SHA-256 there.
