// SPDX-License-Identifier: GPL-3.0-only
//! The dithered last step into 8 bits (nv-rs#830, the fork setting `textures.dither`). **Not 1:1
//! with FalloutNV.exe**: asked for by the maintainer.
//!
//! **Why.** The game's last pass (its HDR tone map, or the image-space passes with HDR off) writes
//! the frame into an 8-bit back buffer, so a smooth gradient (the night sky, fog, a dark wall) is
//! shown in steps of 1/255: visible bands. Keeping the game's own targets in 16 bits
//! (`textures.precise_targets`) does not remove them, because this last write still rounds.
//!
//! **What this does.** With the setting on, the back buffers are made 16 bits per channel
//! ([`crate::format::precise`]; the game still sees its own format), and at `Present` one pass
//! ([`SHADER`]) turns the 16-bit frame into an 8-bit one with an ordered offset per pixel: a
//! value between two 8-bit steps becomes the upper step with a chance equal to how far it lies
//! towards it ([`level`]), so the steps are spread into a fine, fixed grain whose average is the
//! true value. A value already on an 8-bit step stays exactly that step: content the game drew in
//! 8 bits (the menus, the HUD, a flat colour) comes out byte for byte the same. The offsets depend
//! only on the pixel's place ([`offset`]), never on the frame, so two runs give the same picture
//! and a still view does not shimmer. The window's frame, `NV_SHOT`'s captures and the scene
//! tool's pictures all take that 8-bit frame.

/// The 16-bit unsigned normalised value's top.
const TOP: u32 = 65535;

/// The pixel's offset in `0..65535`: a fixed scramble of its place (integers only, so the shader
/// and this function give the same number). Neighbouring pixels get unrelated offsets.
pub fn offset(x: u32, y: u32) -> u32 {
    let mut h = x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd816_3841);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    h % TOP
}

/// The 8-bit level a 16-bit channel `v` is shown as at pixel (`x`, `y`): `floor((v * 255 +
/// offset) / 65535)`. With the offset spread evenly over `0..65535`, the chance of the upper step
/// is the value's fraction between the two steps; a value on a step (`k * 257`) gives `k` for
/// every offset.
pub fn level(v: u16, x: u32, y: u32) -> u8 {
    ((u32::from(v) * 255 + offset(x, y)) / TOP) as u8
}

/// The pass from the 16-bit back buffer into the 8-bit frame: one texel read per pixel (same
/// size), [`level`] per channel, written as level / 255 (which every GPU stores as that level in
/// an 8-bit target, `crate::ramp::level`'s note).
pub(crate) const SHADER: &str = "
@group(0) @binding(0) var t: texture_2d<f32>;
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let c = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(c.x * 2.0 - 1.0, 1.0 - c.y * 2.0, 0.0, 1.0);
}
fn offset(p: vec2<u32>) -> u32 {
    var h = (p.x * 0x8da6b343u) ^ (p.y * 0xd8163841u);
    h = h ^ (h >> 16u);
    h = h * 0x7feb352du;
    h = h ^ (h >> 15u);
    h = h * 0x846ca68bu;
    h = h ^ (h >> 16u);
    return h % 65535u;
}
@fragment fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<u32>(pos.xy);
    let c = textureLoad(t, vec2<i32>(p), 0);
    let v = vec4<u32>(round(saturate(c) * 65535.0));
    let l = (v * 255u + vec4<u32>(offset(p))) / vec4<u32>(65535u);
    return vec4<f32>(l) / 255.0;
}
";

/// The bind group layout of [`SHADER`]: the 16-bit frame.
pub(crate) fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("dither"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        }],
    })
}

/// [`SHADER`]'s pipeline into an 8-bit frame of `format`.
pub(crate) fn pipeline(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("dither"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("dither"),
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
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// Draws `source` (the 16-bit back buffer) into `target` (an 8-bit texture of its size) through
/// [`SHADER`].
pub(crate) fn draw(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    layout: &wgpu::BindGroupLayout,
    source: &wgpu::TextureView,
    target: &wgpu::TextureView,
) {
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("dither"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(source),
        }],
    });
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("dither"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &group, &[]);
    pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_on_an_8_bit_step_is_shown_as_that_step_at_every_pixel() {
        for k in 0..=255u16 {
            for (x, y) in [
                (0, 0),
                (1, 0),
                (0, 1),
                (17, 911),
                (2559, 1439),
                (u32::MAX, 7),
            ] {
                assert_eq!(level(k * 257, x, y), k as u8, "step {k} at ({x}, {y})");
            }
        }
    }

    #[test]
    fn a_value_between_two_steps_averages_to_itself_over_a_patch() {
        // A quarter of the way from step 100 to 101, and three quarters of the way to 41.
        for (v, want) in [(100 * 257 + 64, 100.25f64), (40 * 257 + 193, 40.75)] {
            let (w, h) = (64u32, 64u32);
            let mut sum = 0u64;
            let mut seen = [false; 256];
            for y in 0..h {
                for x in 0..w {
                    let l = level(v, x, y);
                    sum += u64::from(l);
                    seen[usize::from(l)] = true;
                }
            }
            let mean = sum as f64 / f64::from(w * h);
            // The true value in 8-bit steps: v * 255 / 65535.
            let exact = f64::from(v) * 255.0 / 65535.0;
            assert!(
                (mean - exact).abs() < 0.02,
                "{v}: mean {mean}, exact {exact}"
            );
            assert!((exact - want).abs() < 0.01);
            // Only the two neighbouring steps are used.
            let used: Vec<usize> = (0..256).filter(|&i| seen[i]).collect();
            assert_eq!(used, [want as usize, want as usize + 1]);
        }
    }

    #[test]
    fn offsets_cover_the_range_evenly_and_do_not_follow_the_rows() {
        let mut bins = [0u32; 16];
        let mut same_as_left = 0;
        for y in 0..128 {
            for x in 0..128 {
                let o = offset(x, y);
                assert!(o < TOP);
                bins[(o / 4096).min(15) as usize] += 1;
                if x > 0 && offset(x - 1, y) / 4096 == o / 4096 {
                    same_as_left += 1;
                }
            }
        }
        // 16384 offsets in 16 bins: about 1024 each.
        for b in bins {
            assert!((900..1150).contains(&b), "{bins:?}");
        }
        // Neighbours are unrelated: about one in 16 shares its left neighbour's bin.
        assert!(same_as_left < 1400, "{same_as_left}");
    }

    #[test]
    fn every_16_bit_value_survives_the_shader_s_float_round_trip() {
        // The shader reads v / 65535 as a float and takes round(c * 65535) back: exact for every value.
        for v in 0..=u16::MAX {
            let c = f32::from(v) / 65535.0;
            assert_eq!(
                (c.clamp(0.0, 1.0) * 65535.0).round() as u32,
                u32::from(v),
                "{v}"
            );
        }
    }

    #[test]
    fn the_same_pixel_gets_the_same_offset_every_time() {
        assert_eq!(offset(123, 456), offset(123, 456));
        assert_ne!(offset(123, 456), offset(124, 456));
    }

    /// A device on this machine's first API with 16-bit normalised formats (none: skipped).
    fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter =
            crate::device::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
                apply_limit_buckets: false,
            }))
            .ok()?;
        if !adapter
            .features()
            .contains(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM)
        {
            return None;
        }
        crate::device::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::TEXTURE_FORMAT_16BIT_NORM,
            ..Default::default()
        }))
        .ok()
    }

    #[test]
    fn the_shader_gives_the_levels_this_module_works_out() {
        let Some((device, queue)) = device() else {
            eprintln!("no adapter with 16-bit normalised formats: skipped");
            return;
        };
        let (w, h) = (256u32, 4u32);
        // Row y: a 16-bit ramp over the whole range, each channel shifted, so steps, values
        // between steps and both ends are all drawn.
        let value =
            |x: u32, y: u32, c: u32| -> u16 { ((x * 256 + y * 61 + c * 1013) % 65536) as u16 };
        let mut bytes = Vec::new();
        for y in 0..h {
            for x in 0..w {
                for c in 0..4 {
                    bytes.extend_from_slice(&value(x, y, c).to_le_bytes());
                }
            }
        }
        let size = wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        };
        let texture = |format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let source = texture(
            wgpu::TextureFormat::Rgba16Unorm,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        );
        queue.write_texture(
            source.as_image_copy(),
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 8),
                rows_per_image: Some(h),
            },
            size,
        );
        // The frame's format in the device (B8G8R8A8): read back as blue, green, red, alpha.
        let format = wgpu::TextureFormat::Bgra8Unorm;
        let target = texture(
            format,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let layout = layout(&device);
        let pipeline = pipeline(&device, &layout, format);
        let mut encoder = device.create_command_encoder(&Default::default());
        draw(
            &device,
            &mut encoder,
            &pipeline,
            &layout,
            &source.create_view(&Default::default()),
            &target.create_view(&Default::default()),
        );
        let row = (w * 4).div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(row * h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(h),
                },
            },
            size,
        );
        queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the read-back");
        let data = buffer.slice(..).get_mapped_range().expect("mapped");
        for y in 0..h {
            for x in 0..w {
                for c in 0..4 {
                    let at = [2, 1, 0, 3][c as usize];
                    let got = data[(y * row + x * 4 + at) as usize];
                    let want = level(value(x, y, c), x, y);
                    assert_eq!(got, want, "pixel ({x}, {y}) channel {c}");
                }
            }
        }
    }
}
