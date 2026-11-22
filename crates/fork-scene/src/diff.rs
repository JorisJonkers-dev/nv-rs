// SPDX-License-Identifier: GPL-3.0-only
//! Picture difference for the determinism check and the variant compare: a reader of the PNGs this
//! crate writes (8-bit RGB, one IDAT, filter type 0) and the numbers between two pictures.

use flate2::read::ZlibDecoder;
use std::io::Read;

/// A decoded picture: width, height, R, G, B rows top first.
pub type Picture = (u32, u32, Vec<u8>);

/// Reads a PNG written by [`crate::png::encode_rgb`].
pub fn decode_png(bytes: &[u8]) -> Result<Picture, String> {
    if bytes.get(..8) != Some(b"\x89PNG\r\n\x1a\n") {
        return Err("not a PNG".into());
    }
    let (mut at, mut size, mut idat) = (8usize, None, Vec::new());
    while at + 8 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let kind = &bytes[at + 4..at + 8];
        let data = bytes.get(at + 8..at + 8 + len).ok_or("a truncated PNG")?;
        match kind {
            b"IHDR" => {
                if data.len() != 13 || data[8..13] != [8, 2, 0, 0, 0] {
                    return Err("only 8-bit RGB PNGs written by this tool are read".into());
                }
                size = Some((
                    u32::from_be_bytes(data[0..4].try_into().unwrap()),
                    u32::from_be_bytes(data[4..8].try_into().unwrap()),
                ));
            }
            b"IDAT" => idat.extend_from_slice(data),
            _ => {}
        }
        at += 12 + len;
    }
    let (w, h) = size.ok_or("no IHDR")?;
    let mut raw = Vec::new();
    ZlibDecoder::new(&idat[..])
        .read_to_end(&mut raw)
        .map_err(|e| e.to_string())?;
    let stride = w as usize * 3;
    if raw.len() != (stride + 1) * h as usize {
        return Err("the PNG's data is not its size".into());
    }
    let mut rgb = Vec::with_capacity(stride * h as usize);
    for row in raw.chunks_exact(stride + 1) {
        if row[0] != 0 {
            return Err("only unfiltered PNG rows are read".into());
        }
        rgb.extend_from_slice(&row[1..]);
    }
    Ok((w, h, rgb))
}

/// How two pictures of the same size differ.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Difference {
    /// Mean absolute difference over all channel values, 0 to 255.
    pub mean: f64,
    /// The largest single channel difference.
    pub max: u8,
    /// Percent of pixels with any channel differing by more than `THRESHOLD`.
    pub changed_pct: f64,
}

/// A channel difference above this counts the pixel as changed.
pub const THRESHOLD: u8 = 8;

pub fn compare(a: &Picture, b: &Picture) -> Result<Difference, String> {
    if (a.0, a.1) != (b.0, b.1) {
        return Err(format!(
            "the pictures differ in size: {}x{} and {}x{}",
            a.0, a.1, b.0, b.1
        ));
    }
    let (mut sum, mut max, mut changed) = (0u64, 0u8, 0u64);
    for (pa, pb) in a.2.chunks_exact(3).zip(b.2.chunks_exact(3)) {
        let mut worst = 0u8;
        for (x, y) in pa.iter().zip(pb) {
            let d = x.abs_diff(*y);
            sum += u64::from(d);
            worst = worst.max(d);
        }
        max = max.max(worst);
        changed += u64::from(worst > THRESHOLD);
    }
    let pixels = u64::from(a.0) * u64::from(a.1);
    Ok(Difference {
        mean: sum as f64 / (pixels * 3).max(1) as f64,
        max,
        changed_pct: 100.0 * changed as f64 / pixels.max(1) as f64,
    })
}

/// The absolute difference of two pictures of one size as a picture, each channel times 4 (clipped),
/// to see where two runs differ.
pub fn difference_picture(a: &Picture, b: &Picture) -> Result<Picture, String> {
    if (a.0, a.1) != (b.0, b.1) {
        return Err("the pictures differ in size".into());
    }
    let rgb =
        a.2.iter()
            .zip(&b.2)
            .map(|(x, y)| x.abs_diff(*y).saturating_mul(4))
            .collect();
    Ok((a.0, a.1, rgb))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::png::encode_rgb;

    #[test]
    fn a_written_png_reads_back() {
        let rgb: Vec<u8> = (0..4 * 2 * 3).map(|i| i as u8).collect();
        let (w, h, back) = decode_png(&encode_rgb(4, 2, &rgb).unwrap()).unwrap();
        assert_eq!((w, h, back), (4, 2, rgb));
    }

    #[test]
    fn equal_pictures_differ_by_nothing() {
        let p = (2, 1, vec![1, 2, 3, 4, 5, 6]);
        let d = compare(&p, &p).unwrap();
        assert_eq!((d.mean, d.max, d.changed_pct), (0.0, 0, 0.0));
    }

    #[test]
    fn the_numbers_follow_the_pixels() {
        let a = (2, 1, vec![0, 0, 0, 0, 0, 0]);
        let b = (2, 1, vec![0, 0, 3, 0, 0, 30]);
        let d = compare(&a, &b).unwrap();
        assert_eq!(d.max, 30);
        assert!((d.mean - 33.0 / 6.0).abs() < 1e-9);
        assert_eq!(
            d.changed_pct, 50.0,
            "only the second pixel is over the threshold"
        );
    }

    #[test]
    fn sizes_must_match_and_junk_is_refused() {
        assert!(compare(&(1, 1, vec![0; 3]), &(2, 1, vec![0; 6])).is_err());
        assert!(decode_png(b"nope").is_err());
    }
}
