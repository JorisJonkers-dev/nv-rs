### AnhNVSE's commands built in
- Files: `Cargo.toml` (member), `crates/host/Cargo.toml`, `crates/host/src/lib.rs` (`anh_nvse::install`), `crates/anh-nvse` (new)
- Upstream approach: upstream has no AnhNVSE; a script compiled with its opcodes fails at that line unless the plugin's DLL is loaded
- Fork approach: the two AnhNVSE commands the owner's mods call, `GetQuestCompletedAlt` (`0x3604`) and `Sv_PadStart` (`0x361b`), are registered at the opcodes those mods' compiled scripts carry, written from the public wiki pages. The plugin is not listed (`IsPluginInstalled "AnhNVSE"` stays 0), because its other commands are not known
- Decision: keep
- Reason: Main And Pause Menus Overhaul calls `GetQuestCompletedAlt` 66 times and `Sv_PadStart` for its clock and date. AnhNVSE's DLL stops in the plugin host before it registers a command (nv-rs#511)
- Evidence: the record and command tests in `crates/anh-nvse`
