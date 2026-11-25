### Animation manager seams (kNVSE)
- Files: `crates/engine-fallout-cell/src/units/fallout_shared/animation.rs` (`fn_0048fb50`, `fn_0048ffd0`), `crates/engine-core/src/extend.rs` (the seam rows `0x48FB50`, `0x48FFD0`), `crates/host` (the install call)
- Upstream approach: upstream does nothing here
- Fork approach: `~Animation` (`0048fb50`) asks a seam with the animation after it lets its controller manager go, and the animation's set-up (`0048ffd0`) after it stores its new manager; kNVSE built in (`crates/knvse`) forgets the sequences it made for that animation and gives their models' references back. The answer is not read; with no handler nothing happens, and the handler is registered only when kNVSE answers "installed" (off)
- Decision: keep
- Reason: a sequence belongs to one controller manager; when an actor's 3D or manager is rebuilt, a sequence kept for the old manager must not be started again (nv-rs#865)
- Evidence: `a_rebuilt_manager_or_torn_down_animation_forgets_its_sequences` (regression guard) and `a_kept_sequence_of_another_manager_is_not_answered` in `crates/knvse`; the seam assertions in `animation_destructor_tears_everything_down_in_order`, `animation_destructor_without_a_manager_skips_the_controllers` and `setup_creates_the_manager_loads_the_list_and_plays_the_idle`
