### Draw items with the detached draw
- Files: `crates/engine-core/src/mem.rs`, `crates/engine-typed/src/draw/drivers.rs`, `crates/engine-typed/src/render/stage.rs`, `crates/d3d9-wgpu/tests/frame.rs`
- Upstream approach: slaterain/nv-rs#1108 keeps the world's still lit draws as records and draws them as items (`NV_DRAW_ITEMS`, off by default)
- Fork approach: upstream's records sit inside the fork's packet guard and own-block reads; a draw is kept only while no value packet is open; the heap counters and the fork's managed-level tests stay beside upstream's additions
- Decision: keep; drop each difference when upstream has the same code
- Reason: the fork's detached draw and own blocks must keep working with the switch on or off (docs/fork/DRAW-ITEMS.md)
- Evidence: docs/fork/DRAW-ITEMS.md: off byte-identical, stepped drawing thread with items on byte-identical to items off
