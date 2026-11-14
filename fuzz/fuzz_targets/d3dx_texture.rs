#![no_main]

use libfuzzer_sys::fuzz_target;

// A DDS file as D3DX's loader reads it: its info, then the texture and the cube texture made for a device that
// takes every format of D3DX's table, at either precision.
fuzz_target!(|data: &[u8]| {
    let _ = d3dx_tex::get_image_info_from_file_in_memory(data);
    let formats: Vec<u32> = d3dx_tex::table_formats().collect();
    let caps = d3dx_tex::Caps {
        max_width: 4096,
        max_height: 4096,
        pow2: false,
        texture_formats: formats.clone(),
        cube_formats: formats,
    };
    for precision in [d3dx_tex::Precision::Single, d3dx_tex::Precision::Double] {
        let _ = d3dx_tex::create_texture_from_file_in_memory(data, &caps, precision);
        let _ = d3dx_tex::create_cube_texture_from_file_in_memory(data, &caps, precision);
    }
});
