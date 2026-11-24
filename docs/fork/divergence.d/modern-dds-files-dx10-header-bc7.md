### Modern DDS files (DX10 header, BC7)
- Files: `crates/d3dx-tex/src/lib.rs`, `load.rs`, `fork_modern_dds.rs` (new), `crates/d3d9-wgpu/src/format.rs`, `crates/engine-platform/src/d3dx.rs`, `d3d9.rs`, `d3d9device.rs`
- Upstream approach: the D3DX translation refuses every DDS whose FourCC the 2010 library does not know (`DX10`, `ATI1`, `ATI2`), as the DLL does (`D3DXERR_INVALIDDATA`); the Direct3D 9 layer has no BC7 format
- Fork approach: with `NV_DDS_MODERN=1`, and only while wgpu draws, those files are read: the blocks of BC4, BC5 and BC7 (with or without the sRGB flag) go to the device as they are, with the file's own levels, as `ATI1`, `ATI2` and the private FourCC `BC7U` (wgpu's `Bc7RgbaUnorm`); the layer's format check and block sizes know them. Without the option nothing differs from upstream
- Decision: keep; drop when upstream reads DX10-header files
- Reason: remastered and modern texture sets ship BC7 and DX10 headers; the game cannot show them otherwise
- Evidence: nv-rs#750; `fork_modern_dds::tests`, `d3dx::tests_modern_dds`, `frame::a_bc7_texture_is_sampled_as_the_block_decodes`
