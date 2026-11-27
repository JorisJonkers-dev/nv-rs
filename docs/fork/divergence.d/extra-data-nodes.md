### extra-data-nodes

- Files: `crates/engine-typed/src/extra_data/mod.rs`
- Upstream approach: the extra data node classes (`BSExtraData` subclasses) are handled by their translations.
- Fork approach: the family `extradata` registers typed bodies for the node classes' constructors, destructors, `Compare`, `Copy`, `InitItem` and accessors whose differential case passes (`nodes::registered`); the rest wait in `nodes::UNVERIFIED`.
- Decision: keep while the fork moves families out of guest memory ahead of upstream.
- Reason: nv-rs#952 (parent nv-rs#916).
- Evidence: `crates/engine/tests/typed_extra_nodes.rs`.
