### Platform module list: the fork's modules and the log's last flush
- Files: `crates/engine-platform/src/lib.rs`
- Upstream approach: the module list has no fork modules; upstream's renderer stage 1 adds `d3d9items` and a device-state note, left out here (see `upstream-renderer-stage-1-and-lighting-switch-left-out`)
- Fork approach: the list adds `d3dxshader`, `fork_platform`, `fork_tables` and the `fork_sound` re-export (all off Windows only where noted) and `end_process` calls `fork_log::finish()`
- Decision: keep; the modules are fork-owned files that need a line in upstream's list
- Reason: the Linux platform answers, the shader compiler imports, the sound output and the log's summary cannot be reached without their module lines
- Evidence: `docs/fork/LINUX.md`, `docs/fork/LOGGING.md`; the crate does not build off Windows without them
