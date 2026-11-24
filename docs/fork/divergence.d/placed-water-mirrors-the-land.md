### Placed water mirrors the land
- Files: `crates/engine-fallout-cell/src/units/fallout_shared/teswater.rs` (`placed_reflections`, `group_reflects_outdoors`, `lod_water_group_map`), `crates/engine/src/lib.rs`, `crates/host/src/fork_graphics.rs`
- Upstream approach: as the exe, outdoor water away from the world space's water height (Lake Mead, the Colorado) takes the sky reflection map only and mirrors no land; the group's own reflection is drawn only indoors
- Fork approach: `water.placed_reflections` (nv-rs#844) `low` or `high` lets such groups draw the game's own group reflection outdoors (setup `004e9d40`, finish `004eb220`, as indoors) and sample it; the far water takes the map of the highest such group below the viewer. At its default `game` every branch is the exe's
- Decision: keep
- Reason: the maintainer wants lakes and rivers to mirror the land, buildings and far view, with nothing specific to one place or mod
- Evidence: `placed_outdoor_water_takes_its_own_reflection_when_chosen`, `placed_outdoor_water_samples_its_group_s_map_when_chosen`, `far_water_takes_the_reflection_of_the_group_below_the_viewer`, `placed_water_reflects_the_sky_only_unless_chosen`; Lake Mead views under both presets (pull request)
