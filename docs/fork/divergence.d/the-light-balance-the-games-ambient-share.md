### The light balance: the game's ambient share
- Files: `crates/engine-fallout-env/src/units/fallout_shared_sky/sun.rs` (`set_light_colors` takes the share; `AmbientShare`, `AmbientShares`, `want_ambient_share`), `crates/host/src/lib.rs` (`want_ambient_share` with `fork_graphics::ambient_shares` at start)
- Upstream approach: the sun light takes the sky's ambient colour (the weather's outdoors, the cell's indoors) whole every frame, as the exe does; checked in `sun.rs` at the base
- Fork approach: the graphics settings' `light.ambient_exterior` and `light.ambient_interior` (nv-rs#768) hand the light that share of the sky's ambient colour; the sky's own colours are not changed. At 1 (the default and the `original` preset) the code is the exe's, call for call
- Decision: keep; offer upstream the seam if it wants a fill-light setting for its own lighting
- Reason: the maintainer's `modern` preset (nv-rs#591) adds shade and the light from the sky; the game's flat fill light on top of it washes the picture out
- Evidence: `the_ambient_share_at_one_is_the_exes_code`, `the_ambient_share_scales_the_lights_ambient_only`, `an_ambient_share_that_is_not_usable_counts_as_one` (`sun.rs`), `the_light_balance_reaches_the_sun_lights_shares` (`host::fork_graphics`); the standard views under `original` against the build before, difference 0 (pull request)
