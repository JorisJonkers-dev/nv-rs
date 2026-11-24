### The camera path teardown with a child that is gone
- Files: `crates/engine-fallout-forms/src/units/fallout_shared/bgscamerapath.rs` (`fn_0058a450`)
- Upstream approach: the camera path's destructor clears each child's parent (`005f4bb0`) before it tests the child, as the exe does; a child slot that is no longer a camera path (`0058b630` answers null) is written at address 0x30
- Fork approach: a null child is skipped (no parent write, no delete); every other child is unlinked and deleted as before
- Decision: keep
- Reason: with a large load order the game stopped as it quit (`TES::~TES` > `TESDataHandler::ClearData` > `BGSCameraPathManager::DestroyRootPathArrayForms`), nv-rs#837
- Evidence: `destructor_skips_a_child_that_is_no_longer_a_camera_path`, `destructor_unlinks_from_the_root_array_and_deletes_children`
