//! The fixed-function path the game still uses: a screen quad of pre-transformed vertices
//! (`D3DFVF_XYZRHW`, with `D3DFVF_DIFFUSE` and one texture coordinate pair) drawn with no shaders,
//! as the Bink movie player does. Only that shape is provided: it is drawn with a pair of
//! Shader Model 3 shaders made here (a pixel position to clip space, and the texture of stage 0,
//! modulated by the diffuse colour when the vertices have one), so the rest of the device sees an
//! ordinary draw. Lighting, transforms and the other stage states are not provided.

/// `D3DFVF_XYZRHW`, `D3DFVF_DIFFUSE`, `D3DFVF_TEX1`.
const XYZRHW: u32 = 0x004;
const DIFFUSE: u32 = 0x040;
const TEX1: u32 = 0x100;

const END: u32 = 0x0000_FFFF;

/// The vertex layout of a provided `fvf`: whether it carries a diffuse colour; `None` for any
/// other FVF.
pub(crate) fn supported(fvf: u32) -> Option<bool> {
    match fvf {
        f if f == XYZRHW | TEX1 => Some(false),
        f if f == XYZRHW | DIFFUSE | TEX1 => Some(true),
        _ => None,
    }
}

/// The declaration elements of a provided `fvf`: `(stream, offset, type, usage, index)`.
pub(crate) fn elements(diffuse: bool) -> Vec<(u16, u16, u8, u8, u8)> {
    // Types: FLOAT2 1, FLOAT4 3, D3DCOLOR 4. Usages: POSITION 0, TEXCOORD 5, COLOR 10.
    let mut e = vec![(0, 0, 3, 0, 0)];
    if diffuse {
        e.push((0, 16, 4, 10, 0));
    }
    e.push((0, if diffuse { 20 } else { 16 }, 1, 5, 0));
    e
}

/// The first vertex constants of the vertex shader: `c0` the scale of a pixel position to clip
/// space (`2 / width`, `-2 / height`) and `c1` its offset (`-1`, `1`, 0, 1).
pub(crate) fn constants(width: f32, height: f32) -> [f32; 8] {
    [2.0 / width, -2.0 / height, 0.0, 0.0, -1.0, 1.0, 0.0, 1.0]
}

/// The vertex shader: `vs_3_0`, the position's x and y by `c0` plus `c1`, z as it is, w 1; the
/// texture coordinates and the diffuse colour as they are.
pub(crate) fn vertex_shader(diffuse: bool) -> Vec<u32> {
    let mut t = vec![
        0xFFFE_0300,
        // dcl_position v0, dcl_texcoord0 v1 (and dcl_color0 v2).
        0x0200_001F,
        0x8000_0000,
        0x900F_0000,
        0x0200_001F,
        0x8000_0005,
        0x900F_0001,
    ];
    if diffuse {
        t.extend([0x0200_001F, 0x8000_000A, 0x900F_0002]);
    }
    // dcl_position o0, dcl_texcoord0 o1 (and dcl_color0 o2).
    t.extend([0x0200_001F, 0x8000_0000, 0xE00F_0000]);
    t.extend([0x0200_001F, 0x8000_0005, 0xE00F_0001]);
    if diffuse {
        t.extend([0x0200_001F, 0x8000_000A, 0xE00F_0002]);
    }
    t.extend([
        // mad o0.xy, v0, c0, c1
        0x0400_0004,
        0xE003_0000,
        0x90E4_0000,
        0xA0E4_0000,
        0xA0E4_0001,
        // mov o0.z, v0.z
        0x0200_0001,
        0xE004_0000,
        0x90AA_0000,
        // mov o0.w, c1.w
        0x0200_0001,
        0xE008_0000,
        0xA0FF_0001,
        // mov o1, v1
        0x0200_0001,
        0xE00F_0001,
        0x90E4_0001,
    ]);
    if diffuse {
        // mov o2, v2
        t.extend([0x0200_0001, 0xE00F_0002, 0x90E4_0002]);
    }
    t.push(END);
    t
}

/// The pixel shader: `ps_3_0`, the texture of sampler 0 at the texture coordinates, times the
/// diffuse colour when there is one.
pub(crate) fn pixel_shader(diffuse: bool) -> Vec<u32> {
    let mut t = vec![
        0xFFFF_0300,
        // dcl_texcoord0 v0 (and dcl_color0 v1), dcl_2d s0.
        0x0200_001F,
        0x8000_0005,
        0x900F_0000,
    ];
    if diffuse {
        t.extend([0x0200_001F, 0x8000_000A, 0x900F_0001]);
    }
    t.extend([
        0x0200_001F,
        0x9000_0000,
        0xA00F_0800,
        // texld r0, v0, s0
        0x0300_0042,
        0x800F_0000,
        0x90E4_0000,
        0xA0E4_0800,
    ]);
    if diffuse {
        // mul r0, r0, v1
        t.extend([0x0300_0005, 0x800F_0000, 0x80E4_0000, 0x90E4_0001]);
    }
    t.extend([
        // mov oC0, r0
        0x0200_0001,
        0x800F_0800,
        0x80E4_0000,
        END,
    ]);
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sm::{parse, wgsl};

    #[test]
    fn only_the_pre_transformed_quads_are_provided() {
        assert_eq!(supported(0x104), Some(false));
        assert_eq!(supported(0x144), Some(true));
        assert_eq!(supported(0x112), None);
        assert_eq!(supported(0x002), None);
    }

    #[test]
    fn the_elements_follow_the_vertex_layout() {
        assert_eq!(elements(false), vec![(0, 0, 3, 0, 0), (0, 16, 1, 5, 0)]);
        assert_eq!(elements(true)[2], (0, 20, 1, 5, 0));
    }

    #[test]
    fn a_pixel_position_maps_to_clip_space_with_y_up() {
        let c = constants(640.0, 480.0);
        let map = |x: f32, y: f32| (x * c[0] + c[4], y * c[1] + c[5]);
        assert_eq!(map(0.0, 0.0), (-1.0, 1.0));
        assert_eq!(map(640.0, 480.0), (1.0, -1.0));
        assert_eq!(map(320.0, 240.0), (0.0, 0.0));
    }

    #[test]
    fn both_shaders_read_and_convert_to_valid_wgsl() {
        for diffuse in [false, true] {
            let vs = parse(&vertex_shader(diffuse)).unwrap();
            let ps = parse(&pixel_shader(diffuse)).unwrap();
            assert!(wgsl::float_constants_read(&vs) >= 2);
            let vsig = wgsl::signature(&vs);
            let psig = wgsl::signature(&ps);
            assert_eq!(vsig.inputs.len(), 2 + usize::from(diffuse));
            assert_eq!(psig.inputs.len(), 1 + usize::from(diffuse));
            // The texture coordinates are TEXCOORD0 (usage 5, index 0), the colour COLOR0 (usage 10).
            let usages = |s: &wgsl::Signature| {
                s.inputs
                    .iter()
                    .map(|(_, m)| (m.usage, m.index))
                    .collect::<Vec<_>>()
            };
            let mut want = vec![(0, 0), (5, 0)];
            if diffuse {
                want.push((10, 0));
            }
            assert_eq!(usages(&vsig), want);
            assert_eq!(usages(&psig), want[1..]);
        }
    }
}
