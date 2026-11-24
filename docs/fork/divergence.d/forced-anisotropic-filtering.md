### Forced anisotropic filtering
- Files: `crates/d3d9-wgpu/src/state.rs` (`forced_anisotropy`, `SamplerKey::forced`), `crates/host/src/fork_graphics.rs`
- Upstream approach: each sampler is made from the game's sampler states (anisotropic at the level of `iMaxAnisotropy` in `FalloutPrefs.ini` where the game asks for it)
- Fork approach: the graphics setting `textures.anisotropy` (nv-rs#829) raises every sampler that filters a mipmapped texture linearly to anisotropic filtering at the chosen level or the game's, whichever is higher; at its default (`game`) the sampler keys are the game's states as before. `modern` takes `x16`
- Decision: keep
- Reason: the maintainer wants what DXVK adds for this game as engine settings; at the game's Low and Medium (level 1) ground, roads and walls blur at grazing angles
- Evidence: `forced_anisotropy_raises_mipmapped_linear_samplers_only`, `forced_anisotropy_setting_is_clamped`, `the_anisotropy_choice_reaches_the_samplers_level`, `the_presets_say_what_they_set`; crops at 2560x1440 (pull request)
