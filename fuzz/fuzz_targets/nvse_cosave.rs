#![no_main]

use libfuzzer_sys::fuzz_target;

// The extender's save sidecar: the header, then every plugin's chunks and records read through the reader's own
// calls, with a cap on the steps so a file that never ends cannot hang the run.
fuzz_target!(|data: &[u8]| {
    let Ok(mut reader) = nvse::cosave::Reader::open(data) else {
        return;
    };
    for _ in 0..64 {
        let Ok(Some(_plugin)) = reader.next_plugin() else {
            break;
        };
        for _ in 0..64 {
            match reader.next_record() {
                Ok(Some((_kind, _version, length))) => {
                    let _ = reader.read_data(length.min(reader.left()));
                    let _ = (reader.read8(), reader.read16(), reader.read32(), reader.read64());
                }
                _ => break,
            }
        }
        reader.end_plugin();
    }
});
