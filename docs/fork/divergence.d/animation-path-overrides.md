### Animation-path overrides (kNVSE)
- Files: `crates/engine-fallout-cell/src/units/fallout_shared/animation.rs` (`animation_start_group`), `crates/engine-core/src/extend.rs` (`Owner::Knvse`, the seam row `0x4948C0`), `crates/knvse` (new)
- Upstream approach: upstream does nothing here; `Animation::StartGroup` (`004948c0`) always takes the sequence the animation's sequence map holds for the group
- Fork approach: after the test of group 0xff, `StartGroup` asks a seam with the animation, the group and the slot; kNVSE built in (`crates/knvse`) answers the sequence of an animation-path override for that actor, person and group, which is then started in place of the map's. With no handler, or no override, the exe's path runs unchanged; the handler is registered only when kNVSE answers "installed" (off)
- Decision: keep
- Reason: `SetActorAnimationPath` is called 965 times by a large modded profile (Enhanced Movement); kNVSE's DLL patches the game's animation code, which the plugin host does not apply, so the overrides are built natively (nv-rs#865)
- Evidence: `start_group_without_an_override_is_the_exes_path` (regression guard), `start_group_starts_the_overrides_sequence`; the store and handler tests in `crates/knvse`
