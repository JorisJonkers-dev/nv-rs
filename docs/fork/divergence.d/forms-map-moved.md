### forms-map-moved

- Files: `crates/engine-typed/src/forms/tesform.rs`, `crates/engine-typed/src/dispatch/refs.rs`, `crates/engine-typed/src/save/refs.rs`, `crates/engine-typed/src/lib.rs`, `crates/engine-fallout-forms/src/units/fallout_shared/run_00486a60.rs`, `crates/engine-fallout-forms/src/units/fallout_shared/globalfunc.rs`, `crates/engine-fallout-pathfinding/src/units/fallout_shared_pathfinding/navmeshinfomap.rs`, `crates/jip/src/cosave.rs`, `crates/jip/src/commands/ln_fn_game_data.rs`, `docs/ENGINE_CRATE.md`
- Upstream approach: the form map (`011c54c0`) is a guest `NiTPointerMap` whose bucket table every reader reads by its address.
- Fork approach: the `formmap` family has a stage-2 form for that one map (`formmap:dual`, `formmap:moved`, `formmap:freed`): its bucket table lives in the slab and the map object stays a K1 stub (vtable, bucket count and items stay, +8 holds poison, the item count +0xc is the spare word holding the record's handle, its value in the record). The shared bodies (`00853130` and `00a1e380`, `00844700`, `00405430`, `00438af0`, the constructor body of `00486990` and `00486fa0`, the destructor `00486a60`) and the readers by bytes (`004b9ba0`, `006b7f20`, the jip co-save's id map walk and `GetAllForms`) go through `engine_core::typed::form_map`; a map with no record is the guest map they have always worked on.
- Decision: keep while the fork moves families out of guest memory ahead of upstream.
- Reason: nv-rs#951 (parent nv-rs#916), second array.
- Evidence: `crates/engine/tests/typed_form_map.rs`, `docs/fork/FORMMAP-STUB.md`.
