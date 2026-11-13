#![no_main]

use libfuzzer_sys::fuzz_target;

// The decoder answers with return codes, not a Result of its own: only a panic, an overflow or an out-of-bounds
// access is a finding. The first byte picks the precision of the decoding thread and the read size.
fuzz_target!(|data: &[u8]| {
    let Some((first, rest)) = data.split_first() else {
        return;
    };
    let precision = if first & 1 == 0 { vorbis::Precision::Bits24 } else { vorbis::Precision::Bits53 };
    let chunk = 256usize << (first >> 1 & 3);
    let _ = vorbis::decode_all(rest.to_vec(), chunk, precision);
});
