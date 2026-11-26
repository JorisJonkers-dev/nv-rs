### per-draw invariants
- Files: `crates/engine-typed/src/render/draw.rs`, `crates/engine-typed/src/draw/packed.rs`, `crates/engine-typed/src/shader/batch.rs`
- Upstream approach: each strip of a packed geometry reads the strip count, lengths pointer, base vertex, vertex count and device again; each of the draw paths runs the renderer's skinned test again (nv-rs#935).
- Fork approach: the strip loops read those words once before they begin, the skinned test's answer is kept per renderer and target with the words it reads in the key, and the device calls of a geometry share one hold of the device lock. Every call the original makes is still made, in the same order; a cache hit saves only the repeated probe.
- Decision: keep, offer back when upstream takes the same per-draw work off the main thread.
- Reason: the draw path is about 40% of the game main thread and the device thread waits for it (docs/fork/PERF.md); these are per-draw costs that do not change what is drawn.
- Evidence: the typed families' differential cases for `draw_packed`, `draw_shape`, `draw_partition`, `is_skinned` and `render_pass`, the skinned-probe unit test in `render/draw.rs`, and the device-half unit test in `engine-platform`'s device tests. The record-stream, pixel and `NV_PERF` ABBA gates stay with the maintainer.
