#![no_main]

use libfuzzer_sys::fuzz_target;

// The decoder under plugins and archives. The first two bytes pick the size hint (none, or up to 1 MiB, so a
// hint that is wrong either way is tried); an error is a valid outcome.
fuzz_target!(|data: &[u8]| {
    let Some((hint, rest)) = data.split_first_chunk::<2>() else {
        return;
    };
    let hint = u16::from_le_bytes(*hint);
    let size_hint = (hint != 0).then(|| usize::from(hint) * 16);
    let _ = esm::inflate::zlib_decompress(rest, size_hint);
});
