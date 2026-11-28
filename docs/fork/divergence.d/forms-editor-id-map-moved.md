### forms-editor-id-map-moved

- Files: `crates/engine-fallout-unplaced-a/src/units/unplaced/run_00470040.rs`, `crates/engine-fallout-unplaced-a/src/units/unplaced/run_00486c80.rs`, `crates/jip/src/commands/jip_fn_utility.rs`, `crates/jip/src/game_patches/gl_lights.rs`
- Upstream approach: the editor id map (`011c54c8`, a case insensitive string map over the same `NiTPointerMap` layout as the form map) is a guest object whose bucket table and item count every reader and writer reaches by address.
- Fork approach: the `formmap` family's stage-2 form covers this map too (`formmap:dual`, `formmap:moved`, `formmap:freed`), the same stub as the form map (see `forms-map-moved`): `create_registries` attaches its record before the constructor. The string-keyed `SetAt` (`00470200`), the destructor chain (`00486ca0` walking the buckets, `00486dc0` freeing the table), the jip editor id lookup and the jip light editor id setter go through `engine_core::typed::form_map`; the lookup and removal bodies (`00853130`, `00405430`, `00438af0`) were already record-aware. A string map with no record is the guest map they have always worked on.
- Decision: keep while the fork moves families out of guest memory ahead of upstream.
- Reason: nv-rs#951 (parent nv-rs#916), third array (PR 3).
- Evidence: `crates/engine/tests/typed_form_map.rs` (`the_editor_id_map_lives_alike_in_every_form`), `docs/fork/FORMMAP-STUB.md`.
