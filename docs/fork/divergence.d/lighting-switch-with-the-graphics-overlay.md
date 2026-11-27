### Lighting switch with the graphics overlay
- Files: `crates/d3d9-wgpu/src/device.rs`, `crates/d3d9-wgpu/src/scene_light.rs`, `crates/d3d9-wgpu/src/lib.rs`, `crates/host/src/lib.rs`
- Upstream approach: slaterain/nv-rs#1099 lets Shift+F11 switch the sun shadows and the ambient occlusion while playing, with a lighting thread making their pipelines and maps ahead; its occlusion has one quality and ignores the sun's lookup
- Fork approach: upstream's switch and thread kept as they are, joined with the fork's `ao.quality` (pipelines carry the quality they were made at, a stale one is dropped), the sun's lookup variant of the apply pass (a key flag, both variants made ahead), the graphics overlay's live switches (`scene_light::set_wanted`, in the fork-owned `host/src/fork_graphics.rs`), the video memory budget (the remade depth surface is charged) and `fork_log` in place of `eprintln!`
- Decision: keep; drop each piece when upstream has the same feature (quality, sun lookup, budget, overlay switch)
- Reason: both sides' features must work together; the fork's inline pipeline code and lazy thin pipeline were removed in favour of upstream's single implementation (docs/fork/LIGHTING-SWITCH.md)
- Evidence: docs/fork/LIGHTING-SWITCH.md, the pull request's recordings (feature off byte-identical) and the GPU tests of `scene_light` and `sun_shadow`
