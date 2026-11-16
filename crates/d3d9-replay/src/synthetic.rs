//! A made-up scene as a wire file: no game data, for the tests and for checking the harness.
//!
//! A pass-through-with-a-matrix vertex shader, a textured pixel shader and a flat one, one
//! vertex and one index buffer of quads, a few textures, and per frame a clear, then draws that
//! each set a matrix, a texture and now and then a blend state, so the stream has the shape of a
//! frame (constants, binds, a few pipelines) without any of the game.

use d3d9_stream::{wire, Cmd, Data};

const DEVICE: u32 = 0x1000;
const BACK: u32 = 0x1100;
const RT: u32 = 0x1200;
const DS: u32 = 0x1210;
const VS: u32 = 0x1300;
const PS_TEX: u32 = 0x1310;
const PS_FLAT: u32 = 0x1311;
const DECL: u32 = 0x1400;
const VB: u32 = 0x1500;
const IB: u32 = 0x1501;
const TEXTURE: u32 = 0x2000;
const LEVEL: u32 = 0x3000;
const A8R8G8B8: u32 = 21;
const D24S8: u32 = 75;
const END: u32 = 0x0000_FFFF;

/// What the made-up scene is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scene {
    pub frames: u32,
    /// Draws in each frame.
    pub draws: u32,
    pub textures: u32,
    /// The render target's size.
    pub width: u32,
    pub height: u32,
}

impl Scene {
    /// A few frames, quick to replay anywhere.
    pub fn small() -> Scene {
        Scene {
            frames: 4,
            draws: 24,
            textures: 4,
            width: 64,
            height: 64,
        }
    }
}

fn ins(op: u32, params: &[u32]) -> Vec<u32> {
    let mut v = vec![op | ((params.len() as u32) << 24)];
    v.extend_from_slice(params);
    v
}

fn bytes(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|w| w.to_le_bytes()).collect()
}

fn floats(f: &[f32]) -> Vec<u8> {
    f.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// `vs_2_0`: `m4x4 oPos, v0, c0; mov oT0, v1`.
pub fn vertex_shader() -> Vec<u32> {
    let mut t = vec![0xFFFE_0200];
    t.extend(ins(31, &[0x8000_0000, 0x900F_0000]));
    t.extend(ins(31, &[0x8000_0005, 0x900F_0001]));
    t.extend(ins(20, &[0xC00F_0000, 0x90E4_0000, 0xA0E4_0000]));
    t.extend(ins(1, &[0xE00F_0000, 0x90E4_0001]));
    t.push(END);
    t
}

/// `ps_2_0`: `texld r0, t0, s0; mov oC0, r0`.
pub fn textured_pixel_shader() -> Vec<u32> {
    let mut t = vec![0xFFFF_0200];
    t.extend(ins(31, &[0x8000_0000, 0xB003_0000]));
    t.extend(ins(31, &[0x9000_0000, 0xA00F_0800]));
    t.extend(ins(66, &[0x800F_0000, 0xB0E4_0000, 0xA0E4_0800]));
    t.extend(ins(1, &[0x800F_0800, 0x80E4_0000]));
    t.push(END);
    t
}

/// `ps_3_0`: `mov oC0, c0`.
pub fn flat_pixel_shader() -> Vec<u32> {
    let mut t = vec![0xFFFF_0300];
    t.extend(ins(1, &[0x800F_0800, 0xA0E4_0000]));
    t.push(END);
    t
}

/// The scene as a wire file (header and records), its setup first: [`setup_cmds`] counts it.
pub fn stream(s: &Scene) -> Vec<u8> {
    let mut out = wire::header().to_vec();
    let mut put = |c: Cmd<Data<'_>>| wire::encode(&c, &mut out);
    let present = bytes(&[s.width, s.height, A8R8G8B8, 1, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0]);
    let back = bytes(&[BACK]);
    put(Cmd::CreateDevice {
        device: DEVICE,
        focus: 0,
        window: 0,
        behavior: 0x54,
        present: Data::Bytes(&present),
        back_buffers: Data::Bytes(&back),
        depth_stencil: 0,
    });
    for (surface, depth, format) in [(RT, false, A8R8G8B8), (DS, true, D24S8)] {
        put(Cmd::CreateSurface {
            device: DEVICE,
            surface,
            depth,
            width: s.width,
            height: s.height,
            format,
            multisample: 0,
            quality: 0,
            flag: 0,
        });
    }
    for (shader, pixel, tokens) in [
        (VS, false, vertex_shader()),
        (PS_TEX, true, textured_pixel_shader()),
        (PS_FLAT, true, flat_pixel_shader()),
    ] {
        let t = bytes(&tokens);
        put(Cmd::CreateShader {
            device: DEVICE,
            shader,
            pixel,
            tokens: Data::Bytes(&t),
        });
    }
    // FLOAT3 position and FLOAT2 texture coordinates in stream 0, then D3DDECL_END.
    let decl = [
        0u8, 0, 0, 0, 2, 0, 0, 0, 0, 0, 12, 0, 1, 0, 5, 0, 0xFF, 0, 0, 0, 17, 0, 0, 0,
    ];
    put(Cmd::CreateDeclaration {
        device: DEVICE,
        declaration: DECL,
        elements: Data::Bytes(&decl),
    });
    // One quad for each draw of a frame, 20-byte vertices, 16-bit indices.
    let quads = s.draws.max(1);
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for q in 0..quads {
        let x = (q % 16) as f32 / 8.0 - 1.0;
        let y = (q / 16 % 16) as f32 / 8.0 - 1.0;
        let z = 0.5 - (q as f32) / (quads as f32) * 0.4;
        for (dx, dy, u, v) in [
            (0.0, 0.0, 0.0, 0.0),
            (0.12, 0.0, 1.0, 0.0),
            (0.12, 0.12, 1.0, 1.0),
            (0.0, 0.12, 0.0, 1.0),
        ] {
            vertices.extend(floats(&[x + dx, y + dy, z, u, v]));
        }
        let b = (q * 4) as u16;
        for i in [0u16, 1, 2, 0, 2, 3] {
            indices.extend_from_slice(&(b + i).to_le_bytes());
        }
    }
    put(Cmd::CreateBuffer {
        device: DEVICE,
        buffer: VB,
        index: false,
        length: vertices.len() as u32,
        usage: 0,
        format: 0,
        pool: 1,
    });
    put(Cmd::UploadBuffer {
        buffer: VB,
        offset: 0,
        flags: 0,
        data: Data::Bytes(&vertices),
    });
    put(Cmd::CreateBuffer {
        device: DEVICE,
        buffer: IB,
        index: true,
        length: indices.len() as u32,
        usage: 0,
        format: 101,
        pool: 1,
    });
    put(Cmd::UploadBuffer {
        buffer: IB,
        offset: 0,
        flags: 0,
        data: Data::Bytes(&indices),
    });
    for t in 0..s.textures {
        let level = bytes(&[LEVEL + t]);
        put(Cmd::CreateTexture {
            device: DEVICE,
            texture: TEXTURE + t,
            levels: Data::Bytes(&level),
            width: 16,
            height: 16,
            usage: 0,
            format: A8R8G8B8,
            pool: 1,
        });
        let texels: Vec<u8> = (0..16 * 16 * 4)
            .map(|i| (i as u32 * 7 + t * 31) as u8)
            .collect();
        put(Cmd::UploadSurface {
            surface: LEVEL + t,
            row: 64,
            rows: 16,
            data: Data::Bytes(&texels),
        });
    }
    for (state, value) in [(22u32, 1u32), (7, 1)] {
        put(Cmd::SetRenderState {
            device: DEVICE,
            state,
            value,
        });
    }
    put(Cmd::SetRenderTarget {
        device: DEVICE,
        index: 0,
        surface: RT,
    });
    put(Cmd::SetDepthStencilSurface {
        device: DEVICE,
        surface: DS,
    });
    put(Cmd::SetShader {
        device: DEVICE,
        pixel: false,
        shader: VS,
    });
    put(Cmd::SetVertexDeclaration {
        device: DEVICE,
        declaration: DECL,
    });
    // The frames.
    for f in 0..s.frames {
        put(Cmd::Clear {
            device: DEVICE,
            count: 0,
            rects: None,
            flags: 7,
            colour: 0xFF20_2020,
            z: 1.0f32.to_bits(),
            stencil: 0,
            model: 0,
        });
        put(Cmd::SetStreamSource {
            device: DEVICE,
            stream: 0,
            buffer: VB,
            offset: 0,
            stride: 20,
        });
        put(Cmd::SetIndices {
            device: DEVICE,
            buffer: IB,
        });
        for d in 0..s.draws {
            // A translation and a scale in the matrix, by draw.
            let (tx, ty) = ((d % 5) as f32 * 0.01, (f % 3) as f32 * 0.01);
            let m = floats(&[
                0.9, 0.0, 0.0, tx, //
                0.0, 0.9, 0.0, ty, //
                0.0, 0.0, 1.0, 0.0, //
                0.0, 0.0, 0.0, 1.0,
            ]);
            put(Cmd::SetConstants {
                device: DEVICE,
                kind: 0,
                start: 0,
                count: 4,
                data: Data::Bytes(&m),
            });
            let flat = d % 7 == 6;
            put(Cmd::SetShader {
                device: DEVICE,
                pixel: true,
                shader: if flat { PS_FLAT } else { PS_TEX },
            });
            if flat {
                let c = floats(&[0.2, 0.8, 0.2, 1.0]);
                put(Cmd::SetConstants {
                    device: DEVICE,
                    kind: 3,
                    start: 0,
                    count: 1,
                    data: Data::Bytes(&c),
                });
            } else if s.textures > 0 {
                put(Cmd::SetTexture {
                    device: DEVICE,
                    sampler: 0,
                    texture: TEXTURE + d % s.textures,
                });
            }
            if d % 3 == 0 {
                // Alpha blending on for every third draw, off after it.
                for (state, value) in [(27u32, u32::from(d % 6 == 0)), (19, 5), (20, 6)] {
                    put(Cmd::SetRenderState {
                        device: DEVICE,
                        state,
                        value,
                    });
                }
            }
            put(Cmd::DrawIndexedPrimitive {
                device: DEVICE,
                kind: 4,
                base_vertex: 0,
                min_index: 4 * d,
                vertices: 4,
                start_index: 6 * d,
                count: 2,
            });
        }
        put(Cmd::Present {
            device: DEVICE,
            source: None,
            dest: None,
            window: 0,
            dirty: false,
            model: 0,
            frame: f + 1,
        });
    }
    out
}

/// How many commands [`stream`] has before its first frame.
pub fn setup_cmds(s: &Scene) -> u64 {
    // The device, two surfaces, three shaders, the declaration, two buffers with an upload each,
    // each texture with its upload, two states, two surfaces bound, a shader and the declaration.
    1 + 2 + 3 + 1 + 4 + 2 * u64::from(s.textures) + 2 + 2 + 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::Replay;

    #[test]
    fn the_stream_reads_back_with_its_frames() {
        let s = Scene::small();
        let bytes = stream(&s);
        let r = Replay::parse(&bytes, setup_cmds(&s)).unwrap();
        assert_eq!(r.frames.len() as u32, s.frames);
        assert!(r.draws.iter().all(|&d| d == s.draws));
        // The setup ends where the first frame's clear begins.
        assert!(matches!(r.cmds[r.setup], Cmd::Clear { .. }));
    }

    #[test]
    fn the_shaders_convert_to_wgsl() {
        use d3d9_wgpu::sm;
        for tokens in [
            vertex_shader(),
            textured_pixel_shader(),
            flat_pixel_shader(),
        ] {
            let p = sm::parse(&tokens).expect("parses");
            let sig = sm::wgsl::signature(&p);
            assert!(!sig.outputs.is_empty() || p.stage == sm::Stage::Pixel);
        }
    }
}
