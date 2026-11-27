### lockfree-moved-by-default

- Files: `crates/engine-core/src/typed/mod.rs`, `crates/engine-typed/src/lockfree/mod.rs`, `docs/ENGINE_CRATE.md`, `docs/adr/0014-typed-migration-in-two-stages.md`, `crates/engine/tests/typed_lockfree.rs`, `crates/engine-typed/src/lib.rs`, `crates/engine-typed/src/lockfree/store.rs`, `.fork/private-integrity.sha256` (re-pinned, the thresholds file changed)
- Upstream approach: upstream's `NV_TYPED` switch leaves every family at stage 1 unless it is named, and `all` is the default, which is stage 1 only because no family moves by default (checked against the file at `origin/fork` on 2026-11-27).
- Fork approach: `MOVED_BY_DEFAULT` lists the families that move by default and `default_mode` gives each its stage-2 form (lockfree: K2). A family named alone, or by `all`, takes that default; `NV_TYPED=lockfree` (stage 1), `lockfree:moved` (K1), `lockfree:dual` and `lockfree:off` stay as the rollbacks. Recorded nv-trace runs keep every family off.
- Decision: keep while the fork moves families by default ahead of upstream; drop when upstream does the same.
- Reason: the lockfree family is the one with stage 2 built, and extenders reach its bytes only through the host-memory forms (nv-rs#946, parent nv-rs#916).
- Evidence: `a_family_that_moves_by_default_is_in_its_stage_2_form` in `typed/mod.rs`; `forms::the_family_moves_by_default`, `the_bytes_untyped_code_reads_stay_as_stage_1_has_them` and `the_family_in_k2_holds_under_real_threads` in `crates/engine/tests/typed_lockfree.rs`.
