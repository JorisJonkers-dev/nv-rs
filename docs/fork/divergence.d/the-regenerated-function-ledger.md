### The regenerated function ledger
- Files: `docs/LEDGER.md` (generated; every number follows the tree it was generated from)
- Upstream approach: the committed ledger was generated from a tree of 2026-11-07 and reported 110 typed functions
- Fork approach: the ledger is regenerated with `cargo run --release --manifest-path scripts/ledger/Cargo.toml` on `origin/fork`: 69,488 functions, 47,733 translated, 1,092 typed, 5,941 with an incomplete marker, 102,120 cited addresses; `scripts/ledger --check` passes. The generated totals now also have a per-function state, reference level and failure class, and the tool writes a frame-ordered queue (nv-rs#921)
- Decision: drop when upstream regenerates the ledger and carries the per-function state and queue
- Reason: the committed counts were far behind the engine crates (nv-rs#787), and a work queue needs counts and states that follow the markers (nv-rs#916)
- Evidence: the generator's output line "ledger: wrote docs/LEDGER.md (69488 functions, 102120 cited addresses)"; the unit tests in `scripts/ledger/src/main.rs` for state transitions and queue ordering
