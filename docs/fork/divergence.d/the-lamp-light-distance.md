### The lamp light distance
- Files: `crates/engine-bs/src/units/bsshader/shadowscenelight.rs` (`lamp_distance`), `crates/engine/src/lib.rs`, `crates/host/src/fork_graphics.rs`
- Upstream approach: a point light fades out between the light fade distance (`fLightLODStartFade`, the Options menu's slider) and that plus `fLightLODRange` from the camera and is dropped beyond it, so lamps, street lights and lit signs stop lighting the ground a short way off
- Fork approach: `light.lamp_distance` (nv-rs#827) multiplies both fade distances, for every point light alike; the shadow fade distances are not touched. At its default of 1 the distances are the exe's, bit for bit
- Decision: keep
- Reason: the maintainer wants lamps and their light to show from further off at night, with nothing specific to one mod
- Presets: `original` and `performance` keep 1; `modern` takes 16, so at the game's default light fade every lamp of the loaded cells lights its ground (4 dropped lamps past about 6,000 units)
- Evidence: `lamp_distance_scales_the_fade_and_one_keeps_the_game_s`, `test_frustum_cull_fades_with_the_distance`, `modern_lamps_light_their_ground_out_to_the_loaded_cells`, `the_lamp_distance_is_the_game_s_unless_chosen`; night lamp views under both presets (pull requests)
