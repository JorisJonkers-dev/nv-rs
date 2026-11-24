### DDS files in A8B8G8R8 and X8B8G8R8
- Files: `crates/d3dx-tex/src/pixel.rs` (`encodes`, `decode_row`, `encode_row`, `box_integer`, `has_integer_box`), `crates/d3dx-tex/tests/fork_abgr.rs`
- Upstream approach: `d3dx-tex` leaves these two formats to the player's DLL (`NotProvided`)
- Fork approach: they are read and made like A8R8G8B8 and X8R8G8B8 with red and blue changed over: a device that takes them keeps them, on one that does not D3DX's measured pick (A8R8G8B8, X8R8G8B8) is made
- Decision: keep; offer upstream
- Reason: on Linux no DLL can answer, so mods' textures in these formats drew black (nv-rs#823)
- Evidence: `docs/regressions/2026-11-23-dds-a8b8g8r8-black.md`. Not compared with the Microsoft DLL's bytes: each level is compared with the same pixels stored in A8R8G8B8 or X8R8G8B8
