# component-census: which component touches which component's fields

Issue #943. The generated table in [`docs/fork/COMPONENT-CENSUS.md`](../../docs/fork/COMPONENT-CENSUS.md)
is the input for declaring ownership regions and for the order in which families leave guest
memory. The table is written by the command, never by hand.

## Inputs

- `components.tsv`: the declarations. One row per component, with its owner thread, its owning
  region, the path prefixes whose code the component owns and the type prefixes whose fields it
  owns. Paths are matched by longest prefix, types by first prefix; a mixed crate can therefore be
  split by unit folder. The `source` column names the document behind each declaration.
- `crates/`: the Rust sources. The command counts typed field accesses of the form
  `Type::m_field`, for example `e.get(node, NiNode::m_usSize)` and
  `e.set(node, NiNode::m_usSize)`. A `set` call is a write; every other access is a read. Accesses
  within one component, and accesses to types with no declared owner, are not cross-component and
  are reported as skipped.

Only typed field accesses are counted. They are source sites, not runtime frequencies: the count
is how many places in the source touch the field, including the test code beside a translation in
the same file. The runtime touch census (`NV_TOUCH`, `docs/ENGINE_CRATE.md`) is a separate, private
measurement of accesses while the game runs.

## Run

From the repository root, through the shared build queue:

```sh
flock ~/.cache/nv-workers/cargo.lock ~/.local/bin/capped 10G \
  env CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=$HOME/.cache/cargo-targets/nv-rs-gw \
  cargo run --manifest-path research/component-census/Cargo.toml
```

The command writes `docs/fork/COMPONENT-CENSUS.md`. `--out <file>` writes elsewhere and
`--root <dir>` scans another tree; both are for tests. `--check` writes nothing and fails when the
report in the tree differs from the generated one. The command's output is the test seam: the
tests in `src/main.rs` and `tests/cli.rs` build their own declarations and sources from scratch,
and assert the generated rows, counts, example functions and `--check` result.

The tool is a standalone crate with no dependencies, in the pattern of `scripts/ledger` and
`research/families`. Its path is fork-owned (`.fork/overlay-paths`).
