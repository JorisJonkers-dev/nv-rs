#![no_main]

use libfuzzer_sys::fuzz_target;

// Only the absence of panics, arithmetic overflow and out-of-bounds access is
// asserted. A parse error is a valid outcome for arbitrary input.
fuzz_target!(|data: &[u8]| {
    if let Ok(plugin) = esm::Plugin::from_bytes(data.to_vec()) {
        for entry in plugin.records() {
            let _ = plugin.subrecords(entry);
        }
    }
});
