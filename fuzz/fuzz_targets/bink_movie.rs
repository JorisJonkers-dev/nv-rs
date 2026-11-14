#![no_main]

use libfuzzer_sys::fuzz_target;

// A .bik file: the header and frame index, then every frame's audio packets and video data through the decoders.
// Sizes that only make the decoders allocate are capped so the run tests the code, not the allocator.
fuzz_target!(|data: &[u8]| {
    let Ok(movie) = bink::Movie::parse(data) else {
        return;
    };
    let mut audio: Vec<_> = movie
        .audio
        .iter()
        .map(|t| bink::AudioDecoder::new(u32::from(t.sample_rate), if t.flags & 0x2000 != 0 { 2 } else { 1 }, t.flags).ok())
        .collect();
    let mut video = (movie.width <= 1024 && movie.height <= 1024)
        .then(|| bink::Decoder::new(movie.width, movie.height, movie.revision, movie.video_flags));
    for i in 0..movie.frame_count().min(16) {
        let Ok(packet) = movie.packet(i) else {
            continue;
        };
        for (decoder, bytes) in audio.iter_mut().zip(&packet.audio) {
            if let Some(decoder) = decoder {
                let mut out = Vec::new();
                if bytes.len() >= 4 && u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) > (1 << 20) {
                    continue;
                }
                let _ = decoder.decode_packet(bytes, &mut out);
            }
        }
        if let Some(video) = video.as_mut() {
            let _ = video.decode(packet.video);
        }
    }
});
