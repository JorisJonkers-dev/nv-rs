// SPDX-License-Identifier: GPL-3.0-only
//! A small PNG writer for the pictures a scene run takes, and a reader of the 32-bit BMP the
//! wgpu layer's frame readback writes. Pure, no game data.

use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::Write;

/// A top-down 32-bit BMP (B, G, R, X pixels, as `d3d9_wgpu::shot::bmp` writes it): width, height and
/// the pixels as R, G, B rows top first.
pub fn rgb_from_bmp(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let word = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
    };
    if bytes.get(0..2) != Some(b"BM") {
        return Err("not a BMP".into());
    }
    let (Some(offset), Some(width), Some(height)) = (word(10), word(18), word(22)) else {
        return Err("a truncated BMP header".into());
    };
    let bits = bytes.get(28..30).map(|b| u16::from_le_bytes([b[0], b[1]]));
    if bits != Some(32) {
        return Err("only 32-bit BMPs are read".into());
    }
    let rows = (height as i32).unsigned_abs();
    let top_down = (height as i32) < 0;
    let stride = width as usize * 4;
    let data = bytes
        .get(offset as usize..offset as usize + stride * rows as usize)
        .ok_or("a truncated BMP")?;
    let mut rgb = Vec::with_capacity(width as usize * rows as usize * 3);
    for r in 0..rows as usize {
        let row = if top_down { r } else { rows as usize - 1 - r };
        for px in data[row * stride..(row + 1) * stride].chunks_exact(4) {
            rgb.extend_from_slice(&[px[2], px[1], px[0]]);
        }
    }
    Ok((width, rows, rgb))
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// An 8-bit RGB PNG of `rgb` (rows top first).
pub fn encode_rgb(width: u32, height: u32, rgb: &[u8]) -> Result<Vec<u8>, String> {
    if rgb.len() != width as usize * height as usize * 3 {
        return Err(format!(
            "{} bytes are not a {width}x{height} RGB picture",
            rgb.len()
        ));
    }
    let mut raw = Vec::with_capacity(rgb.len() + height as usize);
    for row in rgb.chunks_exact(width as usize * 3) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = ZlibEncoder::new(Vec::new(), Compression::fast());
    z.write_all(&raw).map_err(|e| e.to_string())?;
    let idat = z.finish().map_err(|e| e.to_string())?;
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &idat);
    chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

/// The BMP the readback wrote, as a PNG.
pub fn bmp_to_png(bmp: &[u8]) -> Result<Vec<u8>, String> {
    let (w, h, rgb) = rgb_from_bmp(bmp)?;
    encode_rgb(w, h, &rgb)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    fn bmp(width: u32, height: i32, bgrx: &[u8]) -> Vec<u8> {
        let mut out = b"BM".to_vec();
        out.extend_from_slice(&(54 + bgrx.len() as u32).to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&54u32.to_le_bytes());
        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&(width as i32).to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&[0; 24]);
        out.extend_from_slice(bgrx);
        out
    }

    #[test]
    fn crc32_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn a_top_down_bmp_becomes_rgb_rows_in_order() {
        // 2x2: (B,G,R,X)
        let px = [1, 2, 3, 0, 4, 5, 6, 0, 7, 8, 9, 0, 10, 11, 12, 0];
        let (w, h, rgb) = rgb_from_bmp(&bmp(2, -2, &px)).unwrap();
        assert_eq!((w, h), (2, 2));
        assert_eq!(rgb, [3, 2, 1, 6, 5, 4, 9, 8, 7, 12, 11, 10]);
        // Bottom-up: the rows swap.
        let (_, _, up) = rgb_from_bmp(&bmp(2, 2, &px)).unwrap();
        assert_eq!(up, [9, 8, 7, 12, 11, 10, 3, 2, 1, 6, 5, 4]);
    }

    #[test]
    fn bad_bmps_are_refused() {
        assert!(rgb_from_bmp(b"xx").is_err());
        assert!(rgb_from_bmp(&bmp(2, -2, &[0; 8]))
            .unwrap_err()
            .contains("truncated"));
    }

    #[test]
    fn the_png_decodes_to_the_same_pixels() {
        let rgb: Vec<u8> = (0..4 * 3 * 3).map(|i| (i * 7) as u8).collect();
        let png = encode_rgb(4, 3, &rgb).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        // IHDR: 4x3, 8-bit RGB.
        assert_eq!(&png[16..24], &[0, 0, 0, 4, 0, 0, 0, 3]);
        assert_eq!(&png[24..29], &[8, 2, 0, 0, 0]);
        let len = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
        assert_eq!(&png[37..41], b"IDAT");
        let mut raw = Vec::new();
        ZlibDecoder::new(&png[41..41 + len])
            .read_to_end(&mut raw)
            .unwrap();
        let rows: Vec<u8> = raw
            .chunks(4 * 3 + 1)
            .flat_map(|r| {
                assert_eq!(r[0], 0);
                r[1..].to_vec()
            })
            .collect();
        assert_eq!(rows, rgb);
        assert_eq!(&png[png.len() - 12..png.len() - 8], &[0, 0, 0, 0]);
        assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
    }

    #[test]
    fn a_wrong_size_is_refused() {
        assert!(encode_rgb(2, 2, &[0; 5]).is_err());
    }
}
