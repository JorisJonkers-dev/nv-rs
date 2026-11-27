### forms-array-moved

- Files: `crates/engine-core/src/typed/mod.rs`, `crates/engine-typed/src/forms/mod.rs`, `crates/engine-typed/src/forms/tesform.rs`, `crates/engine-fallout-pathfinding/src/units/fallout_shared_pathfinding/navmeshinfomap.rs`, `docs/ENGINE_CRATE.md`
- Upstream approach: the form registry array (`011c54c4`) is a guest `NiTLargeArray` handled by the translations of the large-array bodies; the forms family is stage 1 only.
- Fork approach: the family has a stage-2 form for that one array (`forms:dual`, `forms:moved`, `forms:freed`): its element block lives in the slab, the object stays a K1 stub, and the large-array bodies it shares (`004705e0`, `004701d0`, `00470110`, `00470140`, `00486b80`, `0061bd20`, `00863db0`, `00864240`, `00864280`) are typed in the family. The nav mesh count reads the array's slot through `form_array::slot`.
- Decision: keep while the fork moves families out of guest memory ahead of upstream.
- Reason: nv-rs#951 (parent nv-rs#916), first step.
- Evidence: `crates/engine/tests/typed_form_array.rs`.
