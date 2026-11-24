### Outdoor water mirrors the scene around it
- Files: `crates/engine-fallout-cell/src/units/fallout_shared/teswater.rs` (`group_reflects_outdoors`, `group_reflects_the_scene`, `setup_group_reflections`)
- Upstream approach: as the exe, outdoor water at the world space's water height takes the world reflection map (outdoors: the sky and the far land); a group's own reflection holds the sky, the land, the far land and the references its water references name as reflected, never the other loaded statics around it
- Fork approach: `water.placed_reflections` `high` (nv-rs#848) gives every outdoor group its own reflection, also at the world's water height, and fills it with the world scene graph (`*011deb7c`), as the exe's world reflection is filled with `bForceHighDetailReflections`; `low` and `game` are unchanged (at `game` every branch is the exe's)
- Decision: keep
- Reason: the maintainer wants every lake, river and pool to mirror what stands around it (a dam, canyon walls, buildings), in general, with nothing specific to one place or mod
- Evidence: `a_high_outdoor_group_reflection_holds_the_whole_scene`, `placed_outdoor_water_takes_its_own_reflection_when_chosen`; picture pairs below Hoover Dam, Lake Mead, Rivet City, the Potomac and the Colorado (pull request)
