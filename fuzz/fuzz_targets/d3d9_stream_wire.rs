#![no_main]

use libfuzzer_sys::fuzz_target;

// A recorded call stream. The header (magic and command list hash) is put in front so the fuzzer works on the
// records, which are where the parsing is.
fuzz_target!(|data: &[u8]| {
    let mut bytes = d3d9_stream::wire::header().to_vec();
    bytes.extend_from_slice(data);
    if let Ok(commands) = d3d9_stream::wire::commands(&bytes) {
        for command in commands {
            let _ = command;
        }
    }
});
