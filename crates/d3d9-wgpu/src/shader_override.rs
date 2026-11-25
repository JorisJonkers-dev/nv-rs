// SPDX-License-Identifier: GPL-3.0-only
//! Shader overrides (port-added, not in the original; nothing is replaced unless a setting asks):
//! one of the game's pixel shaders, named as its shader package names it (`WATER033.pso`), drawn
//! with a WGSL program of the fork's own instead of the conversion of its bytecode, and with the
//! scene's depth to read.
//!
//! **What is asked.** A [`Spec`]: the shader's name, the WGSL and how many float constants it
//! reads. [`resolve`] finds every shader of that name in the game's packages (the winning package
//! of each name, as the game finds them, [`crate::precompile::find_packages`]) and keeps their
//! bytecodes' hashes ([`Override::shaders`]); nothing else of the packages is kept. The host calls
//! [`set`] at the start, before the device is made, with the overrides its settings ask for; the
//! device reads them once when it is made. With none set the device is what it was: nothing is
//! read, made or drawn otherwise, and every surface is made as before.
//!
//! **What the WGSL is.** The converted module's own frame (`crate::sm::wgsl::convert_replaced`):
//! the same bindings, the shader's inputs in the same private registers (`v0`.., `t0`.., as the
//! converter names them), the same samplers (`t0`/`s0`.. for those the game's shader declares),
//! the same outputs (`oC0`..) and the same steps after its body (the alpha test, the occlusion
//! count). In place of the converted body the module calls the override's `fn nv_main()`, which
//! writes the outputs. It reads:
//! - `nv_texcoord[n]`, `nv_color[n]`: the shader's inputs by meaning (`TEXCOORDn`, `COLORn`),
//!   whatever registers its profile put them in (0 for one it does not take);
//! - `nv_c(n)`: the float constant `c<n>` as the game set it (the first [`Override::registers`]
//!   are given to the draw, at least those the game's shader reads); `nv_const_<Name>()`, the
//!   first register of each float constant the shader's constant table names;
//! - `nv_sample_<Name>(uv)`: each 2D sampler the table names and the shader declares, sampled as
//!   its own `texld` would be (the fill mode and the sampler's bias);
//! - `nv_frag`: the pixel's `@builtin(position)` (its centre and its depth, 0 near, 1 far);
//! - [`DECLARATIONS`]: `nv_scene_depth()`, the distance along the view axis of what the scene's
//!   depth held at the pixel before the frame's first overridden draw on that depth surface (the
//!   far distance where nothing was drawn: the sky), and `nv_own_depth()`, the pixel's own.
//!
//! **How it is drawn.** An overridden draw takes a pipeline of its own, made on a thread of its
//! own the first time (until it is made, and wherever it cannot be used, the draw takes the game's
//! shader, as without the override), with bind group 3 holding the depth [`Gpu::snapshot`] made:
//! at the frame's first overridden draw on a depth surface the pass is ended, the depth's first
//! sample is turned into distances in a texture of the fork's own (one full-screen triangle), and
//! the pass goes on. It needs the frame's camera (its near and far planes, the new lighting's
//! words, `crate::scene_light::Params`), recorded where the world is drawn; until one is there the
//! game's shader draws. An overridden draw is never held with others or listed for a later start
//! (`crate::prepare`): the list stays what the game's draws make.
//!
//! The game's bytecode is never copied: the override is the fork's own program, and the packages
//! are only read for the names' hashes on the player's machine.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// What a setting asks to replace: a pixel shader by its package name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    /// The shader's name in the packages, as the game looks it up (`WATER033.pso`); any case.
    pub shader: String,
    /// The WGSL: defines `fn nv_main()` (module documentation).
    pub source: String,
    /// How many float constants (`c0` up) the WGSL reads at most.
    pub registers: u32,
    /// Who asks, for the log (`water.far_water`).
    pub why: String,
}

/// One override as the device takes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Override {
    /// The shader's name and who asks, for the log.
    pub name: String,
    /// The bytecode hashes of the shaders it replaces (`prepare::bytecode_hash`).
    pub shaders: Vec<u64>,
    pub source: String,
    pub registers: u32,
}

static SET: RwLock<Vec<Arc<Override>>> = RwLock::new(Vec::new());

/// Sets the overrides of the run (before the device is made: a device reads them once).
pub fn set(overrides: Vec<Override>) {
    *SET.write().unwrap_or_else(|e| e.into_inner()) = overrides.into_iter().map(Arc::new).collect();
}

/// The overrides set ([`set`]).
pub fn registered() -> Vec<Arc<Override>> {
    SET.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Whether any override is set: the frame's camera is then needed where the world is drawn.
pub fn any() -> bool {
    !SET.read().unwrap_or_else(|e| e.into_inner()).is_empty()
}

/// The bytecode hash the device keys a shader by, of a package's bytes: up to and including the
/// first end token, as the platform hands the device the shader the game creates from them
/// (`engine_platform`'s `CreatePixelShader`).
pub fn hash_of(bytecode: &[u8]) -> u64 {
    let mut end = bytecode.len() - bytecode.len() % 4;
    for (k, w) in bytecode.chunks_exact(4).enumerate() {
        if u32::from_le_bytes([w[0], w[1], w[2], w[3]]) == crate::sm::END {
            end = 4 * k + 4;
            break;
        }
    }
    crate::cache::hash(&bytecode[..end])
}

/// The overrides `specs` ask for, from the shader packages under `roots` (Data folders, lowest
/// priority first, as [`crate::precompile::find_packages`] takes them), and one line for the log
/// per spec. A spec whose shader no package has gives no override (and says so).
pub fn resolve(roots: &[PathBuf], specs: &[Spec]) -> (Vec<Override>, Vec<String>) {
    let (packages, _) = crate::precompile::find_packages(roots);
    let read: Vec<crate::sm::package::Package> = packages
        .iter()
        .filter_map(|p| std::fs::read(p).ok())
        .filter_map(|b| crate::sm::package::Package::parse(&b).ok())
        .collect();
    let mut out = Vec::new();
    let mut lines = Vec::new();
    for spec in specs {
        let mut shaders: Vec<u64> = read
            .iter()
            .flat_map(|p| p.shaders.iter())
            .filter(|s| s.name.eq_ignore_ascii_case(&spec.shader))
            .map(|s| hash_of(&s.bytecode))
            .collect();
        shaders.sort_unstable();
        shaders.dedup();
        let name = format!("{} ({})", spec.shader, spec.why);
        if shaders.is_empty() {
            lines.push(format!(
                "nv-rs: shader override {name}: no shader package has it (not replaced)"
            ));
            continue;
        }
        lines.push(format!(
            "nv-rs: shader override {name}: {} bytecode(s) in {} package(s) drawn with the fork's own shader",
            shaders.len(),
            read.len()
        ));
        out.push(Override {
            name,
            shaders,
            source: spec.source.clone(),
            registers: spec.registers.min(crate::sm::wgsl::FLOAT_CONSTANTS),
        });
    }
    (out, lines)
}

/// The declarations an override's WGSL reads (bind group 3), placed by the converter before it.
pub const DECLARATIONS: &str = "
struct NvView {
    near: f32,
    far: f32,
    width: f32,
    height: f32,
}
@group(3) @binding(0) var nv_depth: texture_2d<f32>;
@group(3) @binding(1) var<uniform> NV_VIEW: NvView;

// What the scene's depth held at this pixel before the frame's first overridden draw, as distance
// along the view axis (game units); the far distance where nothing was drawn.
fn nv_scene_depth() -> f32 {
    let c = clamp(vec2<i32>(nv_frag.xy), vec2<i32>(0), vec2<i32>(textureDimensions(nv_depth)) - 1);
    return textureLoad(nv_depth, c, 0).x;
}

// This pixel's own distance along the view axis.
fn nv_own_depth() -> f32 {
    return NV_VIEW.near * NV_VIEW.far / (NV_VIEW.far - nv_frag.z * (NV_VIEW.far - NV_VIEW.near));
}
";

/// The format of the distances ([`Gpu::snapshot`]).
const DISTANCE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;

/// The step that turns the depth's first sample into distances (`DEPTH_TYPE` replaced).
const COPY: &str = "
struct View {
    near: f32,
    far: f32,
    width: f32,
    height: f32,
}
@group(0) @binding(0) var depth_tex: DEPTH_TYPE;
@group(0) @binding(1) var<uniform> V: View;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let c = clamp(vec2<i32>(p.xy), vec2<i32>(0), vec2<i32>(textureDimensions(depth_tex)) - 1);
    let d = textureLoad(depth_tex, c, 0);
    let z = V.near * V.far / (V.far - d * (V.far - V.near));
    return vec4<f32>(z, 0.0, 0.0, 1.0);
}
";

/// The step's source for a depth surface with or without samples.
pub(crate) fn copy_source(multisampled: bool) -> String {
    COPY.replace(
        "DEPTH_TYPE",
        if multisampled {
            "texture_depth_multisampled_2d"
        } else {
            "texture_depth_2d"
        },
    )
}

/// The distance of a depth value (0 near, 1 far: Direct3D's projection), as the step makes it.
pub fn distance(depth: f32, near: f32, far: f32) -> f32 {
    near * far / (far - depth * (far - near))
}

/// The device's part: the layout of group 3, the step and the distances it made last.
pub(crate) struct Gpu {
    /// Group 3 of an overridden draw's pipeline: the distances and the view block.
    pub(crate) layout: wgpu::BindGroupLayout,
    /// The step's layout and pipeline, for a depth surface without and with samples.
    copy: [Option<(wgpu::BindGroupLayout, wgpu::RenderPipeline)>; 2],
    /// The distances' texture (its size).
    target: Option<(u32, u32, wgpu::Texture, wgpu::TextureView)>,
    /// Group 3 of the last snapshot.
    pub(crate) group: Option<wgpu::BindGroup>,
    /// The present and the depth surface of the last snapshot.
    pub(crate) taken: Option<(u64, u64)>,
}

impl Gpu {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shader override: scene depth"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                uniform_entry(1),
            ],
        });
        Gpu {
            layout,
            copy: [None, None],
            target: None,
            group: None,
            taken: None,
        }
    }

    /// The scene's depth `depth` (its first sample; `samples` per pixel, `width` by `height`) as
    /// distances through `near` and `far`, encoded into `encoder` (outside any pass), and group 3
    /// made for the draws that follow.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn snapshot(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        depth: &wgpu::TextureView,
        samples: u32,
        width: u32,
        height: u32,
        near: f32,
        far: f32,
    ) {
        let ms = samples > 1;
        let k = usize::from(ms);
        if self.copy[k].is_none() {
            let l = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("shader override: depth step"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: ms,
                        },
                        count: None,
                    },
                    uniform_entry(1),
                ],
            });
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("shader override: depth step"),
                source: wgpu::ShaderSource::Wgsl(copy_source(ms).into()),
            });
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&l)],
                immediate_size: 0,
            });
            let p = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("shader override: depth step"),
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: DISTANCE_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
            self.copy[k] = Some((l, p));
        }
        if !matches!(&self.target, Some((w, h, _, _)) if *w == width && *h == height) {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("shader override: scene distances"),
                size: wgpu::Extent3d {
                    width: width.max(1),
                    height: height.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DISTANCE_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.target = Some((width, height, texture, view));
        }
        let (_, _, _, target) = self.target.as_ref().expect("made");
        let (copy_layout, copy) = self.copy[k].as_ref().expect("made");
        // A block of its own each time (made with its words: no write waits on the queue).
        let words = [near, far, width as f32, height as f32];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let block = wgpu::util::DeviceExt::create_buffer_init(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("shader override: view"),
                contents: &bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        let input = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shader override: depth step"),
            layout: copy_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(depth),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: block.as_entire_binding(),
                },
            ],
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shader override: depth step"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(copy);
            pass.set_bind_group(0, &input, &[]);
            pass.draw(0..3, 0..1);
        }
        self.group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shader override: scene depth"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(target),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: block.as_entire_binding(),
                },
            ],
        }));
    }
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sm::{
        usage,
        wgsl::{self, Link, Semantic},
    };

    fn validate(source: &str) {
        let module = naga::front::wgsl::parse_str(source)
            .unwrap_or_else(|e| panic!("{}\n{source}", e.emit_to_string(source)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{e:?}\n{source}"));
    }

    fn package(shaders: &[(&str, &[u32])]) -> Vec<u8> {
        let mut out = vec![0u8; 12];
        out[4..8].copy_from_slice(&(shaders.len() as u32).to_le_bytes());
        for (name, tokens) in shaders {
            let mut n = [0u8; 256];
            n[..name.len()].copy_from_slice(name.as_bytes());
            out.extend_from_slice(&n);
            out.extend_from_slice(&((tokens.len() * 4) as u32).to_le_bytes());
            for t in *tokens {
                out.extend_from_slice(&t.to_le_bytes());
            }
        }
        out
    }

    fn tmp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("nv-shader-override-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("Shaders")).unwrap();
        d
    }

    fn spec(shader: &str) -> Spec {
        Spec {
            shader: shader.into(),
            source: "fn nv_main() { oC0 = vec4<f32>(1.0); }".into(),
            registers: 300,
            why: "test".into(),
        }
    }

    const A: [u32; 2] = [0xFFFF_0300, crate::sm::END];
    const B: [u32; 2] = [0xFFFF_0200, crate::sm::END];

    /// A package's bytes past the end token (padding) are not part of the shader the game makes:
    /// the key is the tokens' up to and including it, as the device's.
    #[test]
    fn the_key_stops_at_the_end_token() {
        let mut bytes: Vec<u8> = A.iter().flat_map(|t| t.to_le_bytes()).collect();
        assert_eq!(hash_of(&bytes), crate::prepare::bytecode_hash(&A));
        bytes.extend_from_slice(&[0xAB; 6]);
        assert_eq!(hash_of(&bytes), crate::prepare::bytecode_hash(&A));
    }

    /// A name is found in every package, in any case, each distinct bytecode once, and keyed as
    /// the device keys the shader the game makes of the same tokens.
    #[test]
    fn a_name_takes_every_package_s_bytecode_by_the_device_s_key() {
        let d = tmp("names");
        std::fs::write(
            d.join("Shaders/shaderpackage001.sdp"),
            package(&[("WATER033.pso", &A), ("other.pso", &B)]),
        )
        .unwrap();
        std::fs::write(
            d.join("Shaders/shaderpackage002.sdp"),
            package(&[("water033.PSO", &B)]),
        )
        .unwrap();
        std::fs::write(
            d.join("Shaders/shaderpackage003.sdp"),
            package(&[("WATER033.pso", &A)]),
        )
        .unwrap();
        let (got, lines) = resolve(std::slice::from_ref(&d), &[spec("WATER033.pso")]);
        assert_eq!(got.len(), 1);
        let mut want = vec![
            crate::prepare::bytecode_hash(&A),
            crate::prepare::bytecode_hash(&B),
        ];
        want.sort_unstable();
        assert_eq!(got[0].shaders, want);
        // Never more than the device has.
        assert_eq!(got[0].registers, crate::sm::wgsl::FLOAT_CONSTANTS);
        assert!(
            lines[0].contains("2 bytecode(s) in 3 package(s)"),
            "{lines:?}"
        );
        let _ = std::fs::remove_dir_all(d);
    }

    /// A shader no package has replaces nothing, and the log says so.
    #[test]
    fn a_missing_name_gives_no_override() {
        let d = tmp("missing");
        std::fs::write(
            d.join("Shaders/shaderpackage001.sdp"),
            package(&[("WATER000.pso", &A)]),
        )
        .unwrap();
        let (got, lines) = resolve(std::slice::from_ref(&d), &[spec("WATER033.pso")]);
        assert!(got.is_empty());
        assert!(lines[0].contains("no shader package has it"), "{lines:?}");
        let _ = std::fs::remove_dir_all(d);
    }

    /// A shader override's module (`crate::shader_override`): the shader's own bindings, inputs
    /// and outputs with the override's program in place of the converted body (none of the
    /// instructions: here no `discard` of the shader's `texkill`), valid with the registers in
    /// uniform blocks and in storage; never for a vertex shader.
    #[test]
    fn an_override_replaces_the_body_and_keeps_the_frame() {
        // The ps_2_x shader of `texture_instructions_convert_to_valid_wgsl`, ending in texkill.
        let t = [
            0xFFFF_0201,
            0x0200_001F,
            0x8000_0000,
            0xB00F_0000,
            0x0200_001F,
            0x9000_0000,
            0xA00F_0800,
            0x0100_0041,
            0xB00F_0000,
            0x0200_0001,
            0x800F_0800,
            0xB0E4_0000,
            crate::sm::END,
        ];
        let p = crate::sm::parse(&t).unwrap();
        let program = "fn nv_main() {\n    let d = nv_scene_depth() - nv_own_depth();\n    oC0 = textureSample(t0, s0, nv_texcoord[0].xy) * nv_c(11) + t0_in + vec4<f32>(d * nv_frag.x);\n}\n";
        for storage in [false, true] {
            let link = Link {
                varyings: vec![Semantic {
                    usage: usage::TEXCOORD,
                    index: 0,
                }],
                storage,
                ..Link::default()
            };
            let s = wgsl::convert_replaced(&p, &link, program).unwrap();
            validate(&s);
            assert!(s.contains("fn body() {\n    nv_main();\n}"), "{s}");
            assert!(s.contains("nv_frag = input.frag;"), "{s}");
            assert!(s.contains("nv_texcoord[0] = input.l0;"), "{s}");
            assert!(s.contains("@group(3) @binding(0) var nv_depth"), "{s}");
            assert!(s.contains("var t0: texture_2d<f32>"), "{s}");
            // The shader's own texkill is not converted (the alpha test after the body stays).
            assert!(!s.contains(".x < 0.0"), "{s}");
            assert!(s.contains("if (!ok) { discard; }"), "{s}");
            // The plain conversion keeps its body and has none of the override.
            let plain = wgsl::convert(&p, &link).unwrap();
            assert!(
                plain.contains(".x < 0.0") && !plain.contains("nv_"),
                "{plain}"
            );
        }
        let v = crate::sm::parse(&[0xFFFE_0300, crate::sm::END]).unwrap();
        assert!(wgsl::convert_replaced(&v, &Link::default(), program).is_err());
    }

    #[test]
    fn the_depth_step_reads_the_surface_it_is_made_for_and_validates() {
        for ms in [false, true] {
            let source = copy_source(ms);
            assert!(!source.contains("DEPTH_TYPE"));
            let module = naga::front::wgsl::parse_str(&source).unwrap();
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap();
        }
    }

    /// The distance is Direct3D's projection undone: the near plane at 0, the far at 1.
    #[test]
    fn a_depth_value_turns_into_its_distance() {
        let (n, f) = (5.0f32, 300_000.0f32);
        assert!((distance(0.0, n, f) - n).abs() < 1e-3);
        assert!((distance(1.0, n, f) - f).abs() / f < 1e-4);
        // The projection of 1000 units, back again.
        let z = 1000.0f32;
        let d = f / (f - n) * (1.0 - n / z);
        assert!((distance(d, n, f) - z).abs() < 0.5);
    }
}
