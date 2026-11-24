### The moon highlight
- Files: `crates/d3d9-wgpu/src/sm/wgsl.rs` (`specular_reads`), `crates/d3d9-wgpu/src/sun_shadow.rs` (`SL.highlight`), `crates/engine-fallout-env/src/units/fallout_shared_sky/sun.rs` (`AmbientShares::highlight`), `crates/host/src/fork_graphics.rs`
- Upstream approach: the game's lit shaders make the highlight from the light's colour, at the game's strength
- Fork approach: in the sun-shadow variants of those shaders, `light.moon_specular` (nv-rs#821) scales that one read at night. At its default of 1, and in shaders without the variant, the result is the game's
- Decision: keep
- Reason: the maintainer's `modern` preset wants a slight moon reflection on glossy materials
- Evidence: `the_highlight_strength_is_on_the_highlight_read_alone`, `only_the_highlights_read_of_the_sun_colour_is_found`, `the_moon_highlight_is_for_the_night_and_the_moon_only`; the standard views under `original` against v0.23.0 (pull request)
