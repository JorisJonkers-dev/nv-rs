### backend-reference-peer

- Files: `crates/engine-core/Cargo.toml`, `Cargo.lock`, `crates/engine-core/src/mem.rs`, `crates/engine-core/src/typed/mod.rs`
- Upstream approach: upstream has no local decomp backend and no fourth `Diff` peer; its test builds load no native library and `Mem` does not expose its reservation's base address (checked against the files at `origin/fork` on 2026-11-26).
- Fork approach: `engine-core`'s test builds carry `typed/backend_peer.rs`, which loads the library built by `scripts/fork/decomp_backend.py`, gives it the engine's guest memory, routes its imports to `Engine::call`, and records a failure class for each mismatch. `Mem::guest_base()` supplies the shared reservation; `libloading` is a dependency of `engine-core` for this one use.
- Decision: keep; drop when upstream adds an equivalent local reference peer and a guest-memory accessor.
- Reason: every typed function is compared with the original code as well as with its translation (nv-rs#916, story 20), and the backend is built on the machine that runs the tests, never shipped (ADR-0020).
- Evidence: nv-rs#928; `scripts/fork/tests/test_backend_evidence.py` and the peer's unit tests in `typed/backend_peer.rs` build their inputs from scratch; `scripts/fork/tests/test_private_run.py` pins that every typed family's suite runs with the backend.
