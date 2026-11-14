#![no_main]

use libfuzzer_sys::fuzz_target;

// The launcher's plugins.txt reader and the subrecord splitter of one record's data.
fuzz_target!(|data: &[u8]| {
    let _ = esm::load_order::parse_plugins_txt(data);
    let _ = esm::record::parse_subrecords(data, 0);
});
