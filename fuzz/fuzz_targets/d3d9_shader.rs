#![no_main]

use d3d9_wgpu::sm;
use libfuzzer_sys::fuzz_target;

// Shader bytecode, as a package (the game's shader file) and as bare tokens: parsed, then converted to WGSL.
// naga validating the output is a free oracle, but only "no panic" is asserted: the converter refuses what it
// cannot express, and the answer to a program linked to nothing is not the device's.
fuzz_target!(|data: &[u8]| {
    if let Ok(package) = sm::package::Package::parse(data) {
        let _ = sm::check_package(&package);
    }
    if let Ok(tokens) = sm::tokens(data) {
        if let Ok(program) = sm::parse(&tokens) {
            if let Ok(source) = sm::wgsl::convert(&program, &sm::wgsl::Link::default()) {
                let _ = sm::validate(&source);
            }
        }
    }
});
