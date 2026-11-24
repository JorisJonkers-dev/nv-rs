### Precise render targets
- Files: `crates/d3d9-wgpu/src/format.rs` (`precise_targets`, `precise`, `Upload::Half`, `downloaded`), `crates/d3d9-wgpu/src/device.rs` (`new_texture`, `new_surface`, `create_texture`, `create_surface`, the sampled copy, the resolve scratch, `read_level`, `get_render_target_data`, `gamma_resolved`), `crates/host/src/fork_graphics.rs`
- Upstream approach: every 8-bit colour format (`A8R8G8B8`, `X8R8G8B8`) is `Bgra8Unorm` on wgpu, render targets included
- Fork approach: the graphics setting `textures.precise_targets` (nv-rs#830, next start, off by default and under `original` and `modern`) makes the render targets of those two formats `Rgba16Float`; their uploads are widened to halves and their read-backs narrowed to the game's 8-bit bytes, so the game sees its own format; back buffers and textures are unchanged
- Decision: keep
- Reason: the maintainer wants what DXVK and the Fallout HDR Patch add for this game as engine settings; multi-pass gradients band in 8-bit targets
- Evidence: `eight_bit_colour_targets_are_made_precise_and_nothing_else`, `a_precise_target_gives_back_every_8_bit_value_it_was_given`, `halves_are_exact_where_they_can_be_and_rounded_to_the_nearest`, `precise_targets_are_off_unless_chosen`; pictures at 2560x1440 (pull request)
