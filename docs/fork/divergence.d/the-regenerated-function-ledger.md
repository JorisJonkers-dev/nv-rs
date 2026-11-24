### The regenerated function ledger
- Files: `docs/LEDGER.md` (generated; every number follows the tree it was generated from)
- Upstream approach: the committed ledger was generated from a tree of 2026-11-07 and reported 110 typed functions
- Fork approach: the ledger is regenerated with `cargo run --release --manifest-path scripts/ledger/Cargo.toml` at commit `40d31ae1`: 69,488 functions, 1,060 typed, 102,109 cited addresses; `scripts/ledger --check` passes
- Decision: drop when upstream regenerates the ledger
- Reason: the committed counts were far behind the engine crates (nv-rs#787)
- Evidence: the generator's output line "ledger: wrote docs/LEDGER.md (69488 functions, 102109 cited addresses)"
