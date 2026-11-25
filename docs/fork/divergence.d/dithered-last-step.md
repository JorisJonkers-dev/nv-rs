### Dithered last step into 8 bits
- Files: `crates/d3d9-wgpu/src/dither.rs` (new), `crates/d3d9-wgpu/src/device.rs` (`make_back_buffers`, `dithered_frame`, `present`, `open`), `crates/d3d9-wgpu/src/format.rs` (`dither`), `crates/fork-gfx/src/builtin.rs`, `crates/host/src/fork_graphics.rs`
- Upstream approach: the back buffers are `Bgra8Unorm`, copied as they are into the window's frame and the captures
- Fork approach: the graphics setting `textures.dither` (nv-rs#830, next start, off by default and under `original`, on under `modern`) makes the back buffers `Rgba16Unorm` and turns each frame into 8 bits with a fixed per-pixel offset (`floor((v * 255 + offset) / 65535)`) before the window, `NV_SHOT` and the scene tool read it; values already on an 8-bit step are unchanged
- Decision: keep
- Reason: the maintainer wants smooth gradients (night sky, fog) without the 8-bit bands; the bands stay with 16-bit targets alone because the last write into the back buffer rounds
- Evidence: `a_value_on_an_8_bit_step_is_shown_as_that_step_at_every_pixel`, `a_value_between_two_steps_averages_to_itself_over_a_patch`, `offsets_cover_the_range_evenly_and_do_not_follow_the_rows`, `the_shader_gives_the_levels_this_module_works_out`, `the_last_step_is_dithered_only_when_chosen`; sky pictures at 2560x1440 (pull request)
