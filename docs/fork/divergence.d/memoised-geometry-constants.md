### Memoised geometry constants
- Files: `crates/engine-typed/src/shader/geometry.rs`, `crates/engine-typed/src/shader/accumulator.rs`
- Upstream approach: every geometry's constant set-up computes the inverse of its transform and its world-view product again, and reads the camera and transform words on each draw (nv-rs#937).
- Fork approach: the inverse and the world-view product are kept in direct-mapped thread-local caches, keyed by the engine and by every word read before they are made (the transform's words and the camera's or view's words). A draw with an unchanged transform and camera reuses both. The set-ups, the device calls and the constant-map uploads still run in the exe's order; only the repeated computation is skipped.
- Decision: keep, offer back when upstream takes per-draw constant work off the main thread.
- Reason: the draw path is about 40% of the game main thread and `setup_geometry_constants` is about 12% of it (docs/fork/PERF.md); these are repeated computations that do not change what is drawn.
- Evidence: the unit tests `the_inverse_is_kept_for_the_next_draw_with_the_same_transform` and `the_world_view_product_is_kept_for_the_next_draw`, and the typed differential cases `setup_geometry_constants`, `world_view_constants` and `inverse_4x4`. The record-stream, pixel and `NV_PERF` ABBA gates stay with the maintainer.
