### Sky light, true moon and starlight
- Files: `crates/engine-fallout-env/src/units/fallout_shared_sky/sun.rs` (`sky_light`, `SkyFill`, `moon_tilt_bits`), `crates/engine-fallout-env/src/units/fallout_shared_sky/moon.rs` (the moon's path takes `moon_tilt_bits`), `crates/d3d9-wgpu/src/scene_light.rs`, `crates/d3d9-wgpu/src/sun_shadow.rs` and `crates/d3d9-wgpu/src/sm/wgsl.rs` (sky fill in the lit shaders and the apply step), `crates/host/src/fork_graphics.rs`
- Upstream approach: a flat ambient colour on every lit surface; the moon's path and light as the exe has them
- Fork approach: the settings `light.ambient_night`, `sun`, `moon`, `sky_dome`, `shade_fill` and `moon_max_height` (nv-rs#772) give light from the sky by surface direction, a moon light by the game's phase, fade and height, a starlight fill, and a lower moon path for the drawn moon and its light together. At their defaults (the `original` preset) the code paths are the exe's
- Decision: keep; offer upstream the seam if it wants sky light of its own
- Reason: the fill cut of nv-rs#769 left shade and night without readable shapes
- Evidence: `docs/regressions/2026-11-23-modern-too-dark.md`; the 11 standard views under `original` against v0.23.0: 10 identical, night Goodsprings differs by at most 16 of 255 on under 0.001% of pixels, as two runs of v0.23.0 differ from each other (pull request)
