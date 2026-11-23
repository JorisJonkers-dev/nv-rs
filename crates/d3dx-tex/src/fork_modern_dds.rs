// SPDX-License-Identifier: GPL-3.0-or-later
//! Modern DDS files, behind an option (nv-rs#750): the DX10-header files remastered and modern
//! texture sets use (BC4, BC5 and BC7, with or without the `_SRGB` flag), and the legacy `ATI1`
//! and `ATI2` FourCCs. The real D3DX9 refuses all of them (`D3DXERR_INVALIDDATA`), so the faithful
//! path of this crate does too; only a caller that passes the modern formats it can make
//! ([`crate::create_texture_from_file_in_memory_modern`]) gets them.
//!
//! What it makes is the file itself: the blocks are passed through to the device as they are (the
//! GPU decodes them), with the file's own levels and sizes. D3DX resamples sides that are not a
//! power of two and generates the missing levels by decoding and encoding the blocks; there is no
//! BC7 encoder here, so a file whose sides are not multiples of 4 is left to the caller
//! ([`crate::Untranslated::PixelFormat`]), and a file with fewer levels than the whole chain gives
//! a texture with those levels. BC7 has no Direct3D 9 format: it is made as the private FourCC
//! [`BC7`] (`BC7U`), which the Direct3D 9 layer on wgpu maps to `Bc7RgbaUnorm`. The `_SRGB`
//! variants are made as the same format: the game's shaders sample raw values, as for DXT.

use crate::dds::{Image, Level, RTYPE_CUBE, RTYPE_TEXTURE};
use crate::load::{Caps, Error, Surface, Texture, Untranslated};

/// The private `D3DFORMAT` of BC7 (there is no Direct3D 9 one): `MAKEFOURCC('B','C','7','U')`.
pub const BC7: u32 = u32::from_le_bytes(*b"BC7U");
/// `MAKEFOURCC('A','T','I','1')`: BC4 (one channel).
pub const ATI1: u32 = u32::from_le_bytes(*b"ATI1");
/// `MAKEFOURCC('A','T','I','2')`: BC5 (two channels).
pub const ATI2: u32 = u32::from_le_bytes(*b"ATI2");

/// The formats this module makes.
pub const FORMATS: [u32; 3] = [BC7, ATI1, ATI2];

/// Whether `f` is one of [`FORMATS`].
pub fn is_modern(f: u32) -> bool {
    FORMATS.contains(&f)
}

/// The bytes of one 4x4 block of a modern format.
fn block_bytes(f: u32) -> usize {
    if f == ATI1 {
        8
    } else {
        16
    }
}

fn word(d: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(d[at..at + 4].try_into().unwrap())
}

const DXGI_BC4_UNORM: u32 = 80;
const DXGI_BC5_UNORM: u32 = 83;
const DXGI_BC7_UNORM: u32 = 98;
const DXGI_BC7_UNORM_SRGB: u32 = 99;
const DX10: u32 = u32::from_le_bytes(*b"DX10");
/// `D3D10_RESOURCE_DIMENSION_TEXTURE2D`.
const DIMENSION_2D: u32 = 3;
/// `D3D10_RESOURCE_MISC_TEXTURECUBE`.
const MISC_CUBE: u32 = 4;

/// Reads `data` as a modern DDS file; `None` for anything else (a classic file, which D3DX's own
/// reader takes or refuses, a DX10 file of another format, an array, a volume, a header that is
/// not whole, or levels the data does not hold).
pub fn parse(data: &[u8]) -> Option<Image> {
    if data.len() < 128 || word(data, 0) != u32::from_le_bytes(*b"DDS ") || word(data, 4) != 124 {
        return None;
    }
    let (flags, height, width) = (word(data, 8), word(data, 12), word(data, 16));
    let (pf_size, pf_flags, fourcc) = (word(data, 76), word(data, 80), word(data, 84));
    if pf_size != 32 || pf_flags & 4 == 0 || width == 0 || height == 0 {
        return None;
    }
    // A volume (the depth flag with a depth, or the volume cap) is not made.
    if (flags & 0x80_0000 != 0 && word(data, 24) > 1) || word(data, 112) & 0x20_0000 != 0 {
        return None;
    }
    let caps2 = word(data, 112);
    let mut at = 128usize;
    let (format, faces) = match fourcc {
        ATI1 | ATI2 => {
            let faces = match caps2 & 0xfc00 {
                0xfc00 => 6,
                0 => 1,
                _ => return None,
            };
            (fourcc, faces)
        }
        DX10 => {
            if data.len() < 148 {
                return None;
            }
            let (dxgi, dimension, misc, array) = (
                word(data, 128),
                word(data, 132),
                word(data, 136),
                word(data, 140),
            );
            if dimension != DIMENSION_2D || array != 1 {
                return None;
            }
            at = 148;
            let format = match dxgi {
                DXGI_BC4_UNORM => ATI1,
                DXGI_BC5_UNORM => ATI2,
                DXGI_BC7_UNORM | DXGI_BC7_UNORM_SRGB => BC7,
                _ => return None,
            };
            (format, if misc & MISC_CUBE != 0 { 6 } else { 1 })
        }
        _ => return None,
    };
    // A mip count past the whole chain is read as the chain: the levels past it are 1x1 ones.
    let chain = 32 - width.max(height).leading_zeros();
    let mips = word(data, 28).max(1).min(chain);
    let stored = mips;
    let block = block_bytes(format);
    let mut levels = Vec::with_capacity((faces * stored) as usize);
    let mut left = (data.len() - at) as u64;
    for _ in 0..faces {
        let (mut w, mut h) = (width, height);
        for l in 0..mips {
            let pitch = w.div_ceil(4) as usize * block;
            let size = pitch as u64 * u64::from(h.div_ceil(4));
            if size > left {
                return None;
            }
            if l < stored {
                levels.push(Level {
                    width: w,
                    height: h,
                    depth: 1,
                    offset: at,
                    pitch,
                    slice: size as usize,
                });
            }
            at += size as usize;
            left -= size;
            w = (w >> 1).max(1);
            h = (h >> 1).max(1);
        }
    }
    Some(Image {
        width,
        height,
        depth: 1,
        mip_levels: mips,
        format,
        resource_type: if faces == 6 {
            RTYPE_CUBE
        } else {
            RTYPE_TEXTURE
        },
        faces,
        stored,
        levels,
        past_file: false,
    })
}

/// The texture of a modern file `img` (read from `data`): the file's levels as they are, for the
/// call's resource type `rtype`. `takes` are the modern formats the device makes.
pub(crate) fn create(
    img: &Image,
    data: &[u8],
    rtype: u32,
    caps: &Caps,
    takes: &[u32],
) -> Result<Texture, Error> {
    let not_provided = |why: String| Err(Error::NotProvided(Untranslated::PixelFormat, why));
    if rtype == RTYPE_CUBE && img.faces != 6 {
        return Err(Error::D3dx(0x8000_4005));
    }
    if rtype == RTYPE_CUBE && img.width != img.height {
        return not_provided(format!("a cube map of {}x{} faces", img.width, img.height));
    }
    if !takes.contains(&img.format) {
        return not_provided(format!(
            "DDS pixel format {:#x}: the device does not take it",
            img.format
        ));
    }
    if img.width % 4 != 0 || img.height % 4 != 0 {
        return not_provided(format!(
            "a {}x{} block-compressed texture (no encoder here to resample it)",
            img.width, img.height
        ));
    }
    if img.width > caps.max_width || img.height > caps.max_height {
        return not_provided(format!(
            "a {}x{} texture past the device's limits",
            img.width, img.height
        ));
    }
    if caps.pow2 && !(img.width.is_power_of_two() && img.height.is_power_of_two()) {
        return not_provided(format!(
            "a {}x{} texture on a device that wants powers of two",
            img.width, img.height
        ));
    }
    // A texture takes the first face of a cube file.
    let faces = if rtype == RTYPE_CUBE { 6 } else { 1 };
    let mut surfaces = Vec::with_capacity((img.stored * faces) as usize);
    for face in 0..faces {
        for l in 0..img.stored {
            let lv = &img.levels[(face * img.stored + l) as usize];
            surfaces.push(Surface {
                format: img.format,
                width: lv.width,
                height: lv.height,
                bytes: data[lv.offset..lv.offset + lv.slice].to_vec(),
            });
        }
    }
    Ok(Texture {
        levels: img.stored,
        faces,
        surfaces,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        create_cube_texture_from_file_in_memory, create_cube_texture_from_file_in_memory_modern,
        create_texture_from_file_in_memory, create_texture_from_file_in_memory_modern,
        get_image_info_from_file_in_memory, get_image_info_from_file_in_memory_modern, Precision,
        D3DXERR_INVALIDDATA,
    };

    fn put(d: &mut [u8], at: usize, v: u32) {
        d[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }

    /// A synthetic file: the 128-byte header with `fourcc` (and, for `DX10`, the 20-byte
    /// extended header with `dxgi`, `misc` and `array`), then `blocks` blocks of `block` bytes
    /// whose bytes count up (so a level's bytes say where they came from).
    fn file(
        w: u32,
        h: u32,
        mips: u32,
        fourcc: &[u8; 4],
        ext: Option<(u32, u32, u32)>,
        caps2: u32,
        data_bytes: usize,
    ) -> Vec<u8> {
        let mut d = vec![0u8; if ext.is_some() { 148 } else { 128 }];
        put(&mut d, 0, u32::from_le_bytes(*b"DDS "));
        put(&mut d, 4, 124);
        put(&mut d, 8, 0x0008_1007);
        put(&mut d, 12, h);
        put(&mut d, 16, w);
        put(&mut d, 28, mips);
        put(&mut d, 76, 32);
        put(&mut d, 80, 4);
        put(&mut d, 84, u32::from_le_bytes(*fourcc));
        put(&mut d, 112, caps2);
        if let Some((dxgi, misc, array)) = ext {
            put(&mut d, 128, dxgi);
            put(&mut d, 132, 3);
            put(&mut d, 136, misc);
            put(&mut d, 140, array);
        }
        d.extend((0..data_bytes).map(|i| (i % 251) as u8));
        d
    }

    fn modern_caps() -> Caps {
        Caps::default()
    }

    /// 8x8 BC7 with four levels (8x8, 4x4, 2x2 and 1x1: 4, 1, 1 and 1 blocks of 16 bytes).
    fn bc7_8x8() -> Vec<u8> {
        file(8, 8, 4, b"DX10", Some((98, 0, 1)), 0, 16 * 7)
    }

    #[test]
    fn a_dx10_bc7_file_is_made_as_its_own_blocks_with_the_option_on() {
        let d = bc7_8x8();
        let t = create_texture_from_file_in_memory_modern(
            &d,
            &modern_caps(),
            Precision::Single,
            &FORMATS,
        )
        .expect("a BC7 texture");
        assert_eq!((t.levels, t.faces, t.surfaces.len()), (4, 1, 4));
        let sizes: Vec<(u32, u32, usize)> = t
            .surfaces
            .iter()
            .map(|s| (s.width, s.height, s.bytes.len()))
            .collect();
        assert_eq!(sizes, [(8, 8, 64), (4, 4, 16), (2, 2, 16), (1, 1, 16)]);
        assert!(t.surfaces.iter().all(|s| s.format == BC7));
        // The blocks are the file's, byte for byte, level after level.
        let blocks: Vec<u8> = t.surfaces.iter().flat_map(|s| s.bytes.clone()).collect();
        assert_eq!(blocks, d[148..]);
        let info = get_image_info_from_file_in_memory_modern(&d).expect("the image info");
        assert_eq!(
            (info.width, info.height, info.mip_levels, info.format),
            (8, 8, 4, BC7)
        );
        assert_eq!(info.resource_type, RTYPE_TEXTURE);
    }

    /// With the option off (the faithful path) the same file is refused as D3DX refuses it,
    /// in the image info, the texture and the cube texture.
    #[test]
    fn the_faithful_path_still_refuses_what_d3dx_refuses() {
        let files = [
            bc7_8x8(),
            file(8, 8, 1, b"DX10", Some((83, 0, 1)), 0, 64),
            file(8, 8, 1, b"DX10", Some((80, 0, 1)), 0, 32),
            file(8, 8, 1, b"ATI1", None, 0, 32),
            file(8, 8, 1, b"ATI2", None, 0, 64),
        ];
        for d in &files {
            let refused: Result<(), crate::Error> = Err(crate::Error::D3dx(D3DXERR_INVALIDDATA));
            assert_eq!(
                get_image_info_from_file_in_memory(d).map(|_| ()),
                refused.clone()
            );
            assert_eq!(
                create_texture_from_file_in_memory(d, &modern_caps(), Precision::Single)
                    .map(|_| ()),
                refused.clone()
            );
            assert_eq!(
                create_cube_texture_from_file_in_memory(d, &modern_caps(), Precision::Single)
                    .map(|_| ()),
                refused
            );
        }
    }

    #[test]
    fn bc5_and_bc4_come_as_ati2_and_ati1_from_either_header() {
        for (dxgi, legacy, f, block) in [(83, b"ATI2", ATI2, 16usize), (80, b"ATI1", ATI1, 8)] {
            let dx10 = file(
                16,
                8,
                5,
                b"DX10",
                Some((dxgi, 0, 1)),
                0,
                block * (8 + 2 + 1 + 1 + 1),
            );
            let old = file(16, 8, 5, legacy, None, 0, block * (8 + 2 + 1 + 1 + 1));
            let mut made = Vec::new();
            for d in [&dx10, &old] {
                let t = create_texture_from_file_in_memory_modern(
                    d,
                    &modern_caps(),
                    Precision::Double,
                    &FORMATS,
                )
                .expect("a one or two channel texture");
                assert_eq!(t.levels, 5);
                let sizes: Vec<(u32, u32)> =
                    t.surfaces.iter().map(|s| (s.width, s.height)).collect();
                assert_eq!(sizes, [(16, 8), (8, 4), (4, 2), (2, 1), (1, 1)]);
                assert!(t.surfaces.iter().all(|s| s.format == f));
                made.push(
                    t.surfaces
                        .iter()
                        .flat_map(|s| s.bytes.clone())
                        .collect::<Vec<u8>>(),
                );
            }
            assert_eq!(made[0], made[1], "the same blocks from either header");
        }
    }

    #[test]
    fn the_srgb_flag_is_dropped_as_the_games_shaders_sample_raw_values() {
        let d = file(4, 4, 1, b"DX10", Some((99, 0, 1)), 0, 16);
        let t = create_texture_from_file_in_memory_modern(
            &d,
            &modern_caps(),
            Precision::Single,
            &FORMATS,
        )
        .unwrap();
        assert_eq!(t.surfaces[0].format, BC7);
    }

    #[test]
    fn a_cube_map_has_six_faces_each_with_its_own_levels() {
        // 8x8, two levels (64 + 16 bytes a face), six faces.
        let d = file(8, 8, 2, b"DX10", Some((98, MISC_CUBE, 1)), 0, 6 * 80);
        let t = create_cube_texture_from_file_in_memory_modern(
            &d,
            &modern_caps(),
            Precision::Single,
            &FORMATS,
        )
        .expect("a cube");
        assert_eq!((t.levels, t.faces, t.surfaces.len()), (2, 6, 12));
        assert_eq!(t.surfaces[1].bytes, d[148 + 64..148 + 80]);
        assert_eq!(t.surfaces[2].bytes, d[148 + 80..148 + 144]);
        // A texture call on a cube file takes its first face; a cube call on a flat file fails.
        let flat = create_texture_from_file_in_memory_modern(
            &d,
            &modern_caps(),
            Precision::Single,
            &FORMATS,
        )
        .unwrap();
        assert_eq!((flat.faces, flat.surfaces.len()), (1, 2));
        let one = bc7_8x8();
        assert_eq!(
            create_cube_texture_from_file_in_memory_modern(
                &one,
                &modern_caps(),
                Precision::Single,
                &FORMATS
            ),
            Err(crate::Error::D3dx(0x8000_4005))
        );
    }

    #[test]
    fn what_cannot_be_passed_through_is_left_to_the_caller() {
        let caps = modern_caps();
        let not_provided = |d: &[u8], takes: &[u32]| {
            matches!(
                create_texture_from_file_in_memory_modern(d, &caps, Precision::Single, takes),
                Err(crate::Error::NotProvided(Untranslated::PixelFormat, _))
            )
        };
        // Sides that are not multiples of 4 (D3DX would resample them; there is no encoder).
        assert!(not_provided(
            &file(6, 8, 1, b"DX10", Some((98, 0, 1)), 0, 64),
            &FORMATS
        ));
        // A device that does not take BC7.
        assert!(not_provided(&bc7_8x8(), &[ATI1, ATI2]));
        assert!(!not_provided(&bc7_8x8(), &[BC7]));
    }

    #[test]
    fn files_the_reader_does_not_make_are_refused_as_before() {
        let bad: Vec<(&str, Vec<u8>)> = vec![
            ("short header", {
                let mut d = vec![0u8; 100];
                d[..4].copy_from_slice(b"DDS ");
                d
            }),
            ("DX10 header cut off", {
                let mut d = bc7_8x8();
                d.truncate(140);
                d
            }),
            ("data cut off", {
                let mut d = bc7_8x8();
                d.pop();
                d
            }),
            ("an array", file(8, 8, 1, b"DX10", Some((98, 0, 2)), 0, 128)),
            (
                "BC1 in a DX10 header",
                file(8, 8, 1, b"DX10", Some((71, 0, 1)), 0, 32),
            ),
            (
                "a signed BC5",
                file(8, 8, 1, b"DX10", Some((84, 0, 1)), 0, 64),
            ),
            ("a 1D texture", {
                let mut d = bc7_8x8();
                put(&mut d, 132, 2);
                d
            }),
            ("a 3D texture", {
                let mut d = bc7_8x8();
                put(&mut d, 132, 4);
                d
            }),
            ("a volume", file(8, 8, 1, b"ATI2", None, 0x20_0000, 64)),
            ("some cube faces", file(8, 8, 1, b"ATI2", None, 0x400, 64)),
            ("a header size of 0", {
                let mut d = bc7_8x8();
                put(&mut d, 4, 0);
                d
            }),
        ];
        for (what, d) in bad {
            assert_eq!(parse(&d), None, "{what}");
            assert_eq!(
                get_image_info_from_file_in_memory_modern(&d).map(|_| ()),
                Err(crate::Error::D3dx(D3DXERR_INVALIDDATA)),
                "{what}"
            );
        }
    }

    /// A mip count past the chain is the chain; a count of 0 is one level.
    #[test]
    fn the_level_count_is_the_files_up_to_the_chain() {
        let d = file(8, 8, 9, b"DX10", Some((98, 0, 1)), 0, 16 * 7);
        assert_eq!(parse(&d).unwrap().stored, 4);
        let d = file(8, 8, 0, b"DX10", Some((98, 0, 1)), 0, 64);
        assert_eq!(parse(&d).unwrap().stored, 1);
    }

    /// Classic files are the same with the option on: D3DX's own reader answers.
    #[test]
    fn a_classic_file_is_the_same_with_the_option_on() {
        let d = file(8, 8, 4, b"DXT1", None, 0, 32 + 8 + 8 + 8);
        let caps = modern_caps();
        let a = create_texture_from_file_in_memory(&d, &caps, Precision::Single).unwrap();
        let b = create_texture_from_file_in_memory_modern(&d, &caps, Precision::Single, &FORMATS)
            .unwrap();
        assert_eq!(a, b);
        assert_eq!(
            get_image_info_from_file_in_memory(&d).unwrap(),
            get_image_info_from_file_in_memory_modern(&d).unwrap()
        );
    }
}
