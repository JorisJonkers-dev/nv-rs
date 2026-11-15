# What the fork changes in upstream-owned code

Upstream is an input the fork imports; the fork wins only on the paths of `.fork/overlay-paths`. Every other difference
is listed in `.fork/allowed-divergence` and, here, with its reason and the upstream change that would make it
unnecessary. A sync or a worker checks this list first: when upstream now does the thing, drop the fork's edit, the
list entry and the row below. Keep both lists as small as possible.

| Fork-only change | Why | Upstream change that makes it unnecessary |
|---|---|---|
| `engine-platform` answers in `fork_platform.rs`: file times of FindFirstFile, known folders, memory status, data folder; hooks in `files.rs`, `process.rs`, `dsound.rs`, `lib.rs`, `host/save_out.rs`, `host/lib.rs` | The game runs on Linux; upstream answers these only on Windows | Upstream implementing the same calls off Windows (it does not as of the 38-commit sync of 2026-11-15) |
| `engine-core/src/engine_unix.rs` and its hook in `engine.rs` | The real clock's start from the machine uptime on Unix | Upstream's own non-Windows clock |
| `engine-platform/src/files/files_host.rs`, `files.rs` (`mod files_host`), `host/cell_offsets.rs` | Game paths (backslashes) map to Linux paths | A path layer in upstream |
| `engine-platform/Cargo.toml`, `d3d9device.rs`, `d3d9resource.rs`, `d3dx.rs`, `d3d9-wgpu/src/loading.rs` | Native Linux build: cfg gates and d3d9-wgpu on every target | Upstream building these on Linux |
| `d3d9-wgpu/tests/frame.rs` | 8x multisampling tests ignored off Windows (lavapipe offers 4x; nv-linux#189) | Upstream skipping them when unsupported |
| `engine-core/src/engine.rs`, `typed/diff.rs`, `x87.rs`, `units/crt.rs` (test builds only) | Test speed (CI.md, "Upstream offers") | Upstream adopting the per-thread template and the control-word note |
| `engine-core/src/lib.rs`, `engine-havok/src/lib.rs`, `host/src/lib.rs`: appended `test_gate` | Real-data gate (TESTING.md); not overlay paths because upstream adds `mod` lines there | Upstream gating real-data tests itself |
| Overlay: `exe.rs`, `units/crt/{cvt,fdiv,math,printf,trig}.rs`, `hkalookatiksolver.rs`, `nvse/commands/math.rs`, `host/native_inflate.rs` | Test-file edits: real-data gate, math test split, tan/tanh ignored off Windows | Upstream splitting or gating those tests |
| `host/Cargo.toml`, `Cargo.toml`, `Cargo.lock`, `host/src/lib.rs`, `nvse/src/plugins.rs`, crates `jg` and `showoff` | Built-in JohnnyGuitar NVSE and ShowOff xNVSE identities and co-save | Upstream registering those plugins (it has not; JIP LN, UI Organizer and xNVSE are upstream's) |
| `README.md` | Fork banner | None; fork-owned text |
