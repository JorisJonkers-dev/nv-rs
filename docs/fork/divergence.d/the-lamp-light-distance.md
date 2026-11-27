### The lamp light distance
- Files: `crates/engine-core/src/lamp_distance.rs` (the scale, re-exported by `crates/engine-bs/src/units/bsshader/shadowscenelight.rs`), `crates/engine-typed/src/lights/scene_frustum.rs` (the native TestFrustumCull), `crates/engine/tests/typed_lights.rs`, `crates/engine/src/lib.rs`, `crates/host/src/fork_graphics.rs`
- Upstream approach: a point light fades out between the light fade distance (`fLightLODStartFade`, the Options menu's slider) and that plus `fLightLODRange` from the camera and is dropped beyond it, so lamps, street lights and lit signs stop lighting the ground a short way off
- Fork approach: `light.lamp_distance` (nv-rs#827) multiplies both fade distances, for every point light alike; the shadow fade distances are not touched. At its default of 1 the distances are the exe's, bit for bit
- Decision: keep
- Reason: the maintainer wants lamps and their light to show from further off at night, with nothing specific to one mod
- Presets: `original` and `performance` keep 1; `modern` takes 16, so at the game's default light fade (1000 + 500) no lamp of the 5x5 loaded cells is dropped (the fade runs from 16,000 to 24,000 units; at 4 it ran from 4,000 to 6,000)
- Evidence: `lamp_distance_scales_the_fade_and_one_keeps_the_game_s`, `test_frustum_cull_fades_with_the_distance`, `modern_lamps_light_their_ground_out_to_the_loaded_cells`, `the_lamp_distance_is_the_game_s_unless_chosen`; night lamp views under both presets (pull requests)
- Note: the native `TestFrustumCull` (upstream's c1abebb3c, 00b9e970) first read the game's distances and so dropped the lamps beyond them again (the ground light pools gone at night); it scales them the same way now, and its differential test runs a case at 8
