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

/// [`compare`] with the pixels left out where two runs of the same variant (`a` and `a_again`) already differ by more
/// than [`THRESHOLD`]: those are the run-to-run noise, so what remains is what the variants differ by. Also the
/// percent of pixels left out.
pub fn compare_without_noise(
    a: &Picture,
    a_again: &Picture,
    b: &Picture,
) -> Result<(Difference, f64), String> {
    if (a.0, a.1) != (b.0, b.1) || (a.0, a.1) != (a_again.0, a_again.1) {
        return Err("the pictures differ in size".into());
    }
    let (mut sum, mut max, mut changed, mut kept, mut masked) = (0u64, 0u8, 0u64, 0u64, 0u64);
    for ((pa, pn), pb) in
        a.2.chunks_exact(3)
            .zip(a_again.2.chunks_exact(3))
            .zip(b.2.chunks_exact(3))
    {
        if pa.iter().zip(pn).any(|(x, y)| x.abs_diff(*y) > THRESHOLD) {
            masked += 1;
            continue;
        }
        kept += 1;
        let mut worst = 0u8;
        for (x, y) in pa.iter().zip(pb) {
            let d = x.abs_diff(*y);
            sum += u64::from(d);
            worst = worst.max(d);
        }
        max = max.max(worst);
        changed += u64::from(worst > THRESHOLD);
    }
    let pixels = kept.max(1);
    Ok((
        Difference {
            mean: sum as f64 / (pixels * 3) as f64,
            max,
            changed_pct: 100.0 * changed as f64 / pixels as f64,
        },
        100.0 * masked as f64 / (kept + masked).max(1) as f64,
    ))
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

/// The light on a picture's ground, for the light balance's check (nv-rs#768): the linear relative luminance (sRGB
/// decoded, Rec. 709 weights, 0 to 1) of the lower [`GROUND_BAND`] of the picture, where the standard day views show
/// ground and walls. `sunlit` is the mean of the band's brightest [`SUNLIT_SHARE`], `shade` of its darkest
/// [`SHADE_SHARE`], `band` of all of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    pub band: f64,
    pub sunlit: f64,
    pub shade: f64,
}

/// The share of the picture's height, from the bottom, that [`light`] reads.
pub const GROUND_BAND: f64 = 0.45;
/// The brightest share of the band counted as sunlit, and the darkest counted as shade.
pub const SUNLIT_SHARE: f64 = 0.4;
pub const SHADE_SHARE: f64 = 0.2;

/// An 8-bit sRGB value as linear light.
fn linear(v: u8) -> f64 {
    let c = f64::from(v) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// [`Light`] of a picture. A picture without a pixel in its band is an `Err`.
pub fn light(p: &Picture) -> Result<Light, String> {
    let (w, h, rgb) = (p.0 as usize, p.1 as usize, &p.2);
    let table: Vec<f64> = (0..=255u8).map(linear).collect();
    let first = h - (h as f64 * GROUND_BAND).round() as usize;
    let mut values: Vec<f64> = rgb[first * w * 3..h * w * 3]
        .chunks_exact(3)
        .map(|px| {
            0.2126 * table[px[0] as usize]
                + 0.7152 * table[px[1] as usize]
                + 0.0722 * table[px[2] as usize]
        })
        .collect();
    if values.is_empty() {
        return Err("a picture without a ground band".into());
    }
    values.sort_by(f64::total_cmp);
    let n = values.len();
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
    let shade_end = ((n as f64 * SHADE_SHARE) as usize).max(1);
    let sunlit_start = (n - ((n as f64 * SUNLIT_SHARE) as usize).max(1)).min(n - 1);
    Ok(Light {
        band: mean(&values),
        sunlit: mean(&values[sunlit_start..]),
        shade: mean(&values[..shade_end]),
    })
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
    fn pixels_that_differ_between_two_runs_are_left_out_of_the_variant_difference() {
        let a = (3, 1, vec![0, 0, 0, 10, 10, 10, 50, 50, 50]);
        let again = (3, 1, vec![0, 0, 0, 10, 10, 10, 90, 90, 90]); // the third pixel is unstable
        let b = (3, 1, vec![0, 0, 0, 60, 60, 60, 0, 0, 0]); // differs in the second (real) and third (noise)
        let (d, masked) = compare_without_noise(&a, &again, &b).unwrap();
        assert!((masked - 100.0 / 3.0).abs() < 1e-9);
        assert_eq!(d.max, 50);
        assert_eq!(d.changed_pct, 50.0, "one of the two kept pixels changed");
    }

    #[test]
    fn sizes_must_match_and_junk_is_refused() {
        assert!(compare(&(1, 1, vec![0; 3]), &(2, 1, vec![0; 6])).is_err());
        assert!(decode_png(b"nope").is_err());
    }

    #[test]
    fn the_light_of_the_ground_band_is_linear_and_split_into_sunlit_and_shade() {
        // 1 x 20: the top 11 rows (sky) are white, the lower 9 (the band, 45%) hold 0, 128 and 255 in thirds.
        let mut px = vec![255u8; 11 * 3];
        for v in [0u8, 0, 0, 128, 128, 128, 255, 255, 255] {
            px.extend_from_slice(&[v; 3]);
        }
        let l = light(&(1, 20, px)).unwrap();
        let mid = linear(128);
        assert!((mid - 0.2158).abs() < 1e-3, "{mid}");
        assert!((l.band - (3.0 * mid + 3.0) / 9.0).abs() < 1e-9);
        // The brightest 40% (3 of 9 values): white; the darkest 20% (1 of 9): black.
        assert!((l.sunlit - 1.0).abs() < 1e-9 && l.shade == 0.0);
        // Twice the light in linear terms is not twice the 8-bit value.
        let a = light(&(1, 2, vec![255, 255, 255, 50, 50, 50])).unwrap();
        let b = light(&(1, 2, vec![255, 255, 255, 100, 100, 100])).unwrap();
        assert!(b.band / a.band > 3.0);
    }
}
