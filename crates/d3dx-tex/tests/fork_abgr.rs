// SPDX-License-Identifier: GPL-3.0-only
//! DDS files in A8B8G8R8 and X8B8G8R8 (nv-rs#823): D3DX reads them and, on a device that does not
//! take them, makes A8R8G8B8 and X8R8G8B8 textures with the same channel values; a device that
//! takes them keeps them. Either way the texture must hold the colours of the same pixels stored
//! in A8R8G8B8 or X8R8G8B8, on every level.

use d3dx_tex::{create_cube_texture_from_file_in_memory, create_texture_from_file_in_memory};
use d3dx_tex::{Caps, Precision, Texture};

const A8R8G8B8: u32 = 21;
const X8R8G8B8: u32 = 22;
const A8B8G8R8: u32 = 32;
const X8B8G8R8: u32 = 33;
const ARGB_MASKS: [u32; 4] = [0xff_0000, 0xff00, 0xff, 0xff00_0000];
const ABGR_MASKS: [u32; 4] = [0xff, 0xff00, 0xff_0000, 0xff00_0000];
const XRGB_MASKS: [u32; 4] = [0xff_0000, 0xff00, 0xff, 0];
const XBGR_MASKS: [u32; 4] = [0xff, 0xff00, 0xff_0000, 0];

/// A 32-bit DDS file of `faces` faces with one stored level.
fn dds(w: u32, h: u32, faces: u32, flags: u32, masks: [u32; 4], pixels: &[u8]) -> Vec<u8> {
    let mut f = vec![0u8; 128];
    let mut put = |at: usize, v: u32| f[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put(0, u32::from_le_bytes(*b"DDS "));
    put(4, 124);
    put(8, 0x100f);
    put(12, h);
    put(16, w);
    put(20, w * 4);
    put(28, 1);
    put(76, 32);
    put(80, flags);
    put(88, 32);
    for (i, m) in masks.into_iter().enumerate() {
        put(92 + 4 * i, m);
    }
    put(108, 0x1000);
    if faces == 6 {
        put(112, 0xfe00);
    }
    f.extend_from_slice(pixels);
    f
}

/// Pixels as (r, g, b, a), every 8-bit value in every channel and at every place of the dither.
fn colours(w: u32, h: u32, faces: u32) -> Vec<[u8; 4]> {
    (0..w * h * faces)
        .map(|i| {
            let v = i.wrapping_mul(37) as u8;
            [v, v.wrapping_add(85), v.wrapping_mul(3), v ^ 0xa5]
        })
        .collect()
}

fn stored(c: &[[u8; 4]], order: impl Fn([u8; 4]) -> [u8; 4]) -> Vec<u8> {
    c.iter().flat_map(|p| order(*p)).collect()
}

fn argb([r, g, b, a]: [u8; 4]) -> [u8; 4] {
    [b, g, r, a]
}

fn abgr([r, g, b, a]: [u8; 4]) -> [u8; 4] {
    [r, g, b, a]
}

fn texture(file: &[u8], faces: u32, p: Precision) -> Texture {
    let caps = Caps::default();
    let made = if faces == 6 {
        create_cube_texture_from_file_in_memory(file, &caps, p)
    } else {
        create_texture_from_file_in_memory(file, &caps, p)
    };
    made.expect("the file makes a texture")
}

const PRECISIONS: [Precision; 2] = [Precision::Single, Precision::Double];

#[test]
fn an_a8b8g8r8_file_makes_the_texture_of_the_same_colours() {
    // A whole chain from a power of two, a side D3DX resamples, and a cube.
    for (w, h, faces) in [(256, 8, 1), (6, 10, 1), (8, 8, 6)] {
        let c = colours(w, h, faces);
        let from_abgr = dds(w, h, faces, 0x41, ABGR_MASKS, &stored(&c, abgr));
        let from_argb = dds(w, h, faces, 0x41, ARGB_MASKS, &stored(&c, argb));
        for p in PRECISIONS {
            let got = texture(&from_abgr, faces, p);
            assert!(got.surfaces.iter().all(|s| s.format == A8R8G8B8));
            assert_eq!(got, texture(&from_argb, faces, p), "{w}x{h}, {faces} faces");
        }
    }
}

#[test]
fn an_x8b8g8r8_file_makes_the_opaque_texture_of_the_same_colours() {
    for (w, h, faces) in [(256, 8, 1), (6, 10, 1), (8, 8, 6)] {
        let c = colours(w, h, faces);
        // The unused byte of the file holds noise; the texture's is 0, as X8R8G8B8's encoder writes.
        let from_xbgr = dds(w, h, faces, 0x40, XBGR_MASKS, &stored(&c, abgr));
        let clean = stored(&c, |p| {
            let [b, g, r, _] = argb(p);
            [b, g, r, 0]
        });
        let from_xrgb = dds(w, h, faces, 0x40, XRGB_MASKS, &clean);
        for p in PRECISIONS {
            let got = texture(&from_xbgr, faces, p);
            assert!(got.surfaces.iter().all(|s| s.format == X8R8G8B8));
            assert_eq!(got, texture(&from_xrgb, faces, p), "{w}x{h}, {faces} faces");
        }
    }
}

#[test]
fn every_channel_value_survives_every_dither_threshold() {
    // Four rows of 256: each value meets each of the dither's sixteen places in the first level.
    let (w, h) = (256u32, 16u32);
    let c: Vec<[u8; 4]> = (0..w * h)
        .map(|i| {
            let v = (i % w) as u8;
            [v, v, v, v]
        })
        .collect();
    let file = dds(w, h, 1, 0x41, ABGR_MASKS, &stored(&c, abgr));
    for p in PRECISIONS {
        let got = texture(&file, 1, p);
        assert_eq!(got.surfaces[0].bytes, stored(&c, argb));
    }
}

/// The caps of a device that takes the two formats, as the adapter tables of some cards do.
fn caps_taking_them() -> Caps {
    let mut caps = Caps::default();
    for f in [A8B8G8R8, X8B8G8R8] {
        caps.texture_formats.push(f);
        caps.cube_formats.push(f);
    }
    caps
}

fn made(file: &[u8], faces: u32, caps: &Caps, p: Precision) -> Texture {
    if faces == 6 {
        create_cube_texture_from_file_in_memory(file, caps, p)
    } else {
        create_texture_from_file_in_memory(file, caps, p)
    }
    .expect("the file makes a texture")
}

/// `t`'s surfaces with red and blue changed over, in format `f`.
fn swapped(t: &Texture, f: u32) -> Texture {
    let mut t = t.clone();
    for s in &mut t.surfaces {
        s.format = f;
        for p in s.bytes.chunks_exact_mut(4) {
            p.swap(0, 2);
        }
    }
    t
}

#[test]
fn a_device_that_takes_a8b8g8r8_keeps_it_with_the_same_colours() {
    let caps = caps_taking_them();
    for (w, h, faces) in [(256, 8, 1), (6, 10, 1), (1, 16, 1), (8, 8, 6)] {
        let c = colours(w, h, faces);
        let from_abgr = dds(w, h, faces, 0x41, ABGR_MASKS, &stored(&c, abgr));
        let from_argb = dds(w, h, faces, 0x41, ARGB_MASKS, &stored(&c, argb));
        for p in PRECISIONS {
            let want = swapped(&made(&from_argb, faces, &caps, p), A8B8G8R8);
            assert_eq!(
                made(&from_abgr, faces, &caps, p),
                want,
                "{w}x{h}, {faces} faces"
            );
        }
    }
}

#[test]
fn a_device_that_takes_x8b8g8r8_keeps_it_with_the_same_colours() {
    let caps = caps_taking_them();
    for (w, h, faces) in [(256, 8, 1), (6, 10, 1), (1, 16, 1), (8, 8, 6)] {
        let c: Vec<[u8; 4]> = colours(w, h, faces)
            .into_iter()
            .map(|[r, g, b, _]| [r, g, b, 0])
            .collect();
        let from_xbgr = dds(w, h, faces, 0x40, XBGR_MASKS, &stored(&c, abgr));
        let from_xrgb = dds(w, h, faces, 0x40, XRGB_MASKS, &stored(&c, argb));
        for p in PRECISIONS {
            let want = swapped(&made(&from_xrgb, faces, &caps, p), X8B8G8R8);
            assert_eq!(
                made(&from_xbgr, faces, &caps, p),
                want,
                "{w}x{h}, {faces} faces"
            );
        }
    }
}
