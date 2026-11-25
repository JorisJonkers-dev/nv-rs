// SPDX-License-Identifier: GPL-3.0-only
//! Shader overrides (`d3d9_wgpu::shader_override`) through the whole backend on the GPU: an
//! overridden pixel shader's draw is drawn with the override's program, which reads the scene's
//! depth as distances (what was drawn before it) and its own; until the frame's camera is known
//! it draws the game's shader; an override for another shader changes no pixel; a late override's
//! draw waits for where the world is drawn, so it reads what the game drew after it, and blends.
//!
//! The scene and shaders are built here (no game file). A machine without a GPU adapter skips
//! with a message.

use d3d9_stream::{Backend, Cmd, Data};
use d3d9_wgpu::scene_light::{self, Params};
use d3d9_wgpu::shader_override::{self, Override};
use d3d9_wgpu::{Output, WgpuBackend};

const END: u32 = 0x0000_FFFF;
const DEVICE: u32 = 0x1000;
const BACK: u32 = 0x1100;
const RT: u32 = 0x1200;
const DS: u32 = 0x1210;
const VS: u32 = 0x1300;
/// The land's pixel shader (`mov oC0, c0`) and the water's (`mov oC0, c1`, the one replaced).
const PS_LAND: u32 = 0x1310;
const PS_WATER: u32 = 0x1311;
const DECL: u32 = 0x1400;
const A8R8G8B8: u32 = 21;
const D24S8: u32 = 75;
const SIZE: u32 = 64;
const NEAR: f32 = 10.0;
const FAR: f32 = 1000.0;
const LAND: f32 = 250.0;
const WATER: f32 = 100.0;

/// One device at a time (the overrides are the process's, read when a device is made).
static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn ins(op: u32, params: &[u32]) -> Vec<u32> {
    let mut v = vec![op | ((params.len() as u32) << 24)];
    v.extend_from_slice(params);
    v
}

fn words(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|w| w.to_le_bytes()).collect()
}

fn floats(f: &[f32]) -> Vec<u8> {
    f.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Direct3D's depth of a point `z` units ahead.
fn depth(z: f32) -> f32 {
    FAR / (FAR - NEAR) * (1.0 - NEAR / z)
}

/// `mov oC0, c<n>` (ps_3_0).
fn pixel_shader(n: u32) -> Vec<u32> {
    let mut ps = vec![0xFFFF_0300];
    ps.extend(ins(1, &[0x800F_0800, 0xA0E4_0000 | n]));
    ps.push(END);
    ps
}

/// The override of the water's shader: red the scene's distance behind, green its own, blue its
/// `c1.z`, each over the far distance.
const PROGRAM: &str = "fn nv_main() {\n    oC0 = vec4<f32>(nv_scene_depth() / 1000.0, nv_own_depth() / 1000.0, nv_c(1).z, 1.0);\n}\n";

fn run(b: &mut WgpuBackend, cmd: Cmd<Data<'_>>) {
    b.execute(cmd);
}

/// A backend (its device made with `overrides` set) with the scene's target and depth
/// (`samples` per pixel), a pass-through vertex shader and the two pixel shaders; none without an
/// adapter.
fn backend(samples: u32, overrides: Vec<Override>) -> Option<WgpuBackend> {
    shader_override::set(overrides);
    let mut b = WgpuBackend::new(Output::Offscreen);
    let present = words(&[SIZE, SIZE, A8R8G8B8, 1, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0]);
    let back = words(&[BACK]);
    b.execute(Cmd::CreateDevice {
        device: DEVICE,
        focus: 0,
        window: 0,
        behavior: 0x54,
        present: Data::Bytes(&present),
        back_buffers: Data::Bytes(&back),
        depth_stencil: 0,
    });
    shader_override::set(Vec::new());
    if b.device().is_none() {
        eprintln!("no GPU adapter: the shader overrides' tests are skipped");
        return None;
    }
    for (surface, is_depth, format) in [(RT, false, A8R8G8B8), (DS, true, D24S8)] {
        run(
            &mut b,
            Cmd::CreateSurface {
                device: DEVICE,
                surface,
                depth: is_depth,
                width: SIZE,
                height: SIZE,
                format,
                multisample: if samples > 1 { samples } else { 0 },
                quality: 0,
                flag: 0,
            },
        );
    }
    let mut vs = vec![0xFFFE_0200];
    vs.extend(ins(31, &[0x8000_0000, 0x900F_0000]));
    vs.extend(ins(1, &[0xC00F_0000, 0x90E4_0000]));
    vs.push(END);
    for (shader, pixel, tokens) in [
        (VS, false, vs),
        (PS_LAND, true, pixel_shader(0)),
        (PS_WATER, true, pixel_shader(1)),
    ] {
        let t = words(&tokens);
        run(
            &mut b,
            Cmd::CreateShader {
                device: DEVICE,
                shader,
                pixel,
                tokens: Data::Bytes(&t),
            },
        );
    }
    run(
        &mut b,
        Cmd::SetShader {
            device: DEVICE,
            pixel: false,
            shader: VS,
        },
    );
    let decl = [0u8, 0, 0, 0, 3, 0, 0, 0, 0xFF, 0, 0, 0, 17, 0, 0, 0];
    run(
        &mut b,
        Cmd::CreateDeclaration {
            device: DEVICE,
            declaration: DECL,
            elements: Data::Bytes(&decl),
        },
    );
    run(
        &mut b,
        Cmd::SetVertexDeclaration {
            device: DEVICE,
            declaration: DECL,
        },
    );
    run(
        &mut b,
        Cmd::SetRenderTarget {
            device: DEVICE,
            index: 0,
            surface: RT,
        },
    );
    run(
        &mut b,
        Cmd::SetDepthStencilSurface {
            device: DEVICE,
            surface: DS,
        },
    );
    for (vb, data) in [
        (VB_LAND, rectangle_vertices(1.0, -1.0, depth(LAND))),
        (VB_WATER, rectangle_vertices(1.0, 0.0, depth(WATER))),
    ] {
        run(
            &mut b,
            Cmd::CreateBuffer {
                device: DEVICE,
                buffer: vb,
                index: false,
                length: data.len() as u32,
                usage: 0,
                format: 0,
                pool: 1,
            },
        );
        run(
            &mut b,
            Cmd::UploadBuffer {
                buffer: vb,
                offset: 0,
                flags: 0,
                data: Data::Bytes(&data),
            },
        );
    }
    // No culling, depth test and writes on.
    for (state, value) in [(22, 1), (7, 1), (14, 1)] {
        run(
            &mut b,
            Cmd::SetRenderState {
                device: DEVICE,
                state,
                value,
            },
        );
    }
    Some(b)
}

/// The water's override, for its shader's bytecode.
fn water_override() -> Override {
    Override {
        name: "water (test)".into(),
        shaders: vec![shader_override::hash_of(&words(&pixel_shader(1)))],
        source: PROGRAM.into(),
        registers: 4,
        late: false,
        blend: false,
        patch: false,
    }
}

fn constant(b: &mut WgpuBackend, n: u32, c: [f32; 4]) {
    let regs = floats(&c);
    run(
        b,
        Cmd::SetConstants {
            device: DEVICE,
            kind: 3,
            start: n,
            count: 1,
            data: Data::Bytes(&regs),
        },
    );
}

/// The land's vertices (the whole target at [`LAND`]) and the water's (the top half at [`WATER`]).
const VB_LAND: u32 = 0x1500;
const VB_WATER: u32 = 0x1501;

/// A rectangle from `top` to `bottom` (clip-space y) across the target at depth `z`, as vertices.
fn rectangle_vertices(top: f32, bottom: f32, z: f32) -> Vec<u8> {
    let corners = [
        [-1.0, bottom],
        [-1.0, top],
        [1.0, top],
        [-1.0, bottom],
        [1.0, top],
        [1.0, bottom],
    ];
    let v: Vec<f32> = corners.iter().flat_map(|c| [c[0], c[1], z, 1.0]).collect();
    floats(&v)
}

/// Draws the rectangle in the vertex buffer `vb`.
fn rectangle(b: &mut WgpuBackend, vb: u32) {
    run(
        b,
        Cmd::SetStreamSource {
            device: DEVICE,
            stream: 0,
            buffer: vb,
            offset: 0,
            stride: 16,
        },
    );
    run(
        b,
        Cmd::DrawPrimitive {
            device: DEVICE,
            kind: 4,
            start: 0,
            count: 2,
        },
    );
}

/// One frame: the land over the whole target at [`LAND`], the camera's words (when `camera`),
/// the water over the top half at [`WATER`]; its pixels (B, G, R, A) and the frame presented.
fn frame(b: &mut WgpuBackend, n: u32, camera: bool) -> Vec<[u8; 4]> {
    run(
        b,
        Cmd::Clear {
            device: DEVICE,
            count: 0,
            rects: None,
            flags: 7,
            colour: 0xFF00_0000,
            z: 1.0f32.to_bits(),
            stencil: 0,
            model: 0,
        },
    );
    run(
        b,
        Cmd::SetShader {
            device: DEVICE,
            pixel: true,
            shader: PS_LAND,
        },
    );
    constant(b, 0, [0.5, 0.5, 0.5, 1.0]);
    rectangle(b, VB_LAND);
    if camera {
        let r = 37.5f32.to_radians().tan();
        let m = Params {
            near: NEAR,
            far: FAR,
            left: -r,
            right: r,
            bottom: -r,
            top: r,
            sun: [0.0, 0.8, -0.6],
            shadow: 0.0,
            strength: scene_light::STRENGTH,
        }
        .to_words();
        run(
            b,
            Cmd::SetTransform {
                device: DEVICE,
                state: scene_light::STATE,
                matrix: Data::from_words(&m),
            },
        );
    }
    run(
        b,
        Cmd::SetShader {
            device: DEVICE,
            pixel: true,
            shader: PS_WATER,
        },
    );
    constant(b, 1, [0.0, 1.0, 0.75, 1.0]);
    rectangle(b, VB_WATER);
    let d = b.device().expect("the device");
    let (_, _, px) = d.read_surface_pixels(RT).expect("the target's pixels");
    let px = px
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect();
    run(
        b,
        Cmd::Present {
            device: DEVICE,
            source: None,
            dest: None,
            window: 0,
            dirty: false,
            model: 0,
            frame: n + 1,
        },
    );
    px
}

/// The pixel at the middle of the top half (the water) and of the bottom half (the land).
fn water_and_land(px: &[[u8; 4]]) -> ([u8; 4], [u8; 4]) {
    let at = |x: u32, y: u32| px[(y * SIZE + x) as usize];
    (at(SIZE / 2, SIZE / 4), at(SIZE / 2, 3 * SIZE / 4))
}

/// The frames until the water is drawn with the override (its pipeline is made on a thread of
/// its own): the water's and the land's pixels then; none after a while.
fn overridden(b: &mut WgpuBackend) -> Option<([u8; 4], [u8; 4])> {
    for n in 0..400 {
        let (water, land) = water_and_land(&frame(b, n, true));
        // The game's shader gives green (c1 = 0, 1, 0.75); the override's red the land's distance.
        if water[2] > 0 {
            return Some((water, land));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    None
}

/// The override draws the water with its own program: the scene's distance behind it (the
/// land's), its own, and the constant it reads; the land, drawn with another shader, is the game's.
#[test]
fn an_overridden_shader_reads_the_scene_depth_and_its_own() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    for samples in [1, 4] {
        let Some(mut b) = backend(samples, vec![water_override()]) else {
            return;
        };
        let (water, land) = overridden(&mut b)
            .unwrap_or_else(|| panic!("{samples} samples: the override never drew"));
        let want = |v: f32| (v * 255.0).round() as i32;
        let near = |got: u8, v: f32| (i32::from(got) - want(v)).abs() <= 2;
        // B, G, R, A.
        assert!(
            near(water[2], LAND / FAR),
            "{samples} samples: behind {water:?}"
        );
        assert!(
            near(water[1], WATER / FAR),
            "{samples} samples: own {water:?}"
        );
        assert!(near(water[0], 0.75), "{samples} samples: c1.z {water:?}");
        assert_eq!(land, [128, 128, 128, 255], "{samples} samples: the land");
    }
}

/// Before the frame's camera is known the water takes the game's shader (the override needs the
/// near and far planes).
#[test]
fn without_the_frame_s_camera_the_game_s_shader_draws() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut b) = backend(1, vec![water_override()]) else {
        return;
    };
    for n in 0..20 {
        let (water, _) = water_and_land(&frame(&mut b, n, false));
        assert_eq!(water, [191, 255, 0, 255], "frame {n}");
    }
}

/// An override of a shader the frame does not draw changes no pixel of it.
#[test]
fn an_override_of_another_shader_changes_no_pixel() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let other = Override {
        shaders: vec![shader_override::hash_of(&words(&pixel_shader(7)))],
        ..water_override()
    };
    let (Some(mut plain), Some(mut with)) = (backend(4, Vec::new()), backend(4, vec![other]))
    else {
        return;
    };
    for n in 0..5 {
        assert_eq!(
            frame(&mut plain, n, true),
            frame(&mut with, n, true),
            "frame {n}"
        );
    }
}

/// The frame of a late override: the water first (the game draws the far water before the far
/// land), then the land, then the camera's words (where the world is drawn); its pixels.
fn frame_late(b: &mut WgpuBackend, n: u32) -> Vec<[u8; 4]> {
    run(
        b,
        Cmd::Clear {
            device: DEVICE,
            count: 0,
            rects: None,
            flags: 7,
            colour: 0xFF00_0000,
            z: 1.0f32.to_bits(),
            stencil: 0,
            model: 0,
        },
    );
    run(
        b,
        Cmd::SetShader {
            device: DEVICE,
            pixel: true,
            shader: PS_WATER,
        },
    );
    constant(b, 1, [0.0, 1.0, 0.75, 1.0]);
    rectangle(b, VB_WATER);
    run(
        b,
        Cmd::SetShader {
            device: DEVICE,
            pixel: true,
            shader: PS_LAND,
        },
    );
    constant(b, 0, [0.5, 0.5, 0.5, 1.0]);
    rectangle(b, VB_LAND);
    let r = 37.5f32.to_radians().tan();
    let m = Params {
        near: NEAR,
        far: FAR,
        left: -r,
        right: r,
        bottom: -r,
        top: r,
        sun: [0.0, 0.8, -0.6],
        shadow: 0.0,
        strength: scene_light::STRENGTH,
    }
    .to_words();
    run(
        b,
        Cmd::SetTransform {
            device: DEVICE,
            state: scene_light::STATE,
            matrix: Data::from_words(&m),
        },
    );
    let d = b.device().expect("the device");
    let (_, _, px) = d.read_surface_pixels(RT).expect("the target's pixels");
    let px = px
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect();
    run(
        b,
        Cmd::Present {
            device: DEVICE,
            source: None,
            dest: None,
            window: 0,
            dirty: false,
            model: 0,
            frame: n + 1,
        },
    );
    px
}

/// A late override's draw is drawn where the world is drawn: it reads the land the game drew
/// after it, and blends over it by the program's alpha; the game's own order would have hidden
/// the land under the water.
#[test]
fn a_late_override_reads_what_is_drawn_after_it_and_blends() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    for samples in [1, 4] {
        let late = Override {
            source: PROGRAM.replace(", 1.0);", ", 0.5);"),
            late: true,
            blend: true,
            ..water_override()
        };
        let Some(mut b) = backend(samples, vec![late]) else {
            return;
        };
        let mut got = None;
        for n in 0..400 {
            let (water, land) = water_and_land(&frame_late(&mut b, n));
            // The game's shader: green, opaque. The override: half its colour over the land.
            if water[1] != 255 {
                got = Some((water, land));
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let (water, land) = got.unwrap_or_else(|| panic!("{samples} samples: never drawn late"));
        let want = |v: f32| (0.5 * v * 255.0 + 0.5 * 128.0).round() as i32;
        let near = |got: u8, v: f32| (i32::from(got) - want(v)).abs() <= 3;
        // B, G, R, A.
        assert!(
            near(water[2], LAND / FAR),
            "{samples} samples: behind {water:?}"
        );
        assert!(
            near(water[1], WATER / FAR),
            "{samples} samples: own {water:?}"
        );
        assert!(near(water[0], 0.75), "{samples} samples: c1.z {water:?}");
        assert_eq!(land, [128, 128, 128, 255], "{samples} samples: the land");
    }
}

/// A patch override keeps the game's own shader and changes only the constant it patches: the
/// water's shader writes `c1` (0, 1, 0.75, 1); the patch turns its red to 1 and leaves the rest,
/// and the land, drawn with another shader, is the game's.
#[test]
fn a_patch_override_changes_only_the_constants_it_patches() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let patch = Override {
        source: "// nv: patch\nfn nv_patch_c(n: i32, v: vec4<f32>) -> vec4<f32> {\n    if (n == 1) { return vec4<f32>(1.0, v.yzw); }\n    return v;\n}\n".into(),
        patch: true,
        ..water_override()
    };
    for samples in [1, 4] {
        let Some(mut b) = backend(samples, vec![patch.clone()]) else {
            return;
        };
        let mut seen = None;
        for n in 0..400 {
            let (water, land) = water_and_land(&frame(&mut b, n, true));
            if water[2] > 0 {
                seen = Some((water, land));
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let (water, land) =
            seen.unwrap_or_else(|| panic!("{samples} samples: the patch never drew"));
        // B, G, R, A: blue 0.75, green 1 as the game's shader writes them, red 1 from the patch.
        assert_eq!(water, [191, 255, 255, 255], "{samples} samples: the water");
        assert_eq!(land, [128, 128, 128, 255], "{samples} samples: the land");
    }
}
