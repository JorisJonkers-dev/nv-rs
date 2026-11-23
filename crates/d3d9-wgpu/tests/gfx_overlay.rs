// SPDX-License-Identifier: GPL-3.0-only
//! Fork-only. The graphics overlay's panel (nv-rs#592, nv-rs#730) drawn by egui-wgpu over a frame,
//! on this machine's first card (lavapipe in CI; none: skipped). `NV_OVERLAY_PICTURE=<dir>`
//! writes the pictures (`overlay-1280x720.bmp`, `overlay-2560x1440.bmp`) to look at.
//! `NV_OVERLAY_BENCH=1` also measures the panel's cost (ignored test).

use fork_gfx::{builtin, resolve, Inputs, Place, PlaceName, SettingsFile};
use fork_overlay::{egui, Panel, View};

fn device() -> Option<(wgpu::Device, wgpu::Queue, wgpu::Features)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter =
        d3d9_wgpu::gfx_overlay::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
            apply_limit_buckets: false,
        }))
        .ok()?;
    let features = adapter.features()
        & (wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS);
    let (d, q) =
        d3d9_wgpu::gfx_overlay::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: features,
            ..Default::default()
        }))
        .ok()?;
    Some((d, q, features))
}

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
const GREY: [u8; 4] = [128, 120, 112, 255];

/// A frame of [`GREY`] with the panel drawn over it at `scale`, read back B, G, R, A rows.
struct Frame {
    tex: wgpu::Texture,
    view: wgpu::TextureView,
    size: (u32, u32),
}

impl Frame {
    fn new(d: &wgpu::Device, size: (u32, u32)) -> Frame {
        let tex = d.create_texture(&wgpu::TextureDescriptor {
            label: Some("overlay test frame"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        Frame { tex, view, size }
    }

    fn clear(&self, e: &mut wgpu::CommandEncoder) {
        let c = |v: u8| f64::from(v) / 255.0;
        e.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: c(GREY[2]),
                        g: c(GREY[1]),
                        b: c(GREY[0]),
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }

    fn read(&self, d: &wgpu::Device, q: &wgpu::Queue, mut e: wgpu::CommandEncoder) -> Vec<u8> {
        let (w, h) = self.size;
        let row = (w * 4).div_ceil(256) * 256;
        let buffer = d.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(row * h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        e.copy_texture_to_buffer(
            self.tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        q.submit([e.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = d.poll(wgpu::PollType::wait_indefinitely());
        let data = buffer.slice(..).get_mapped_range().expect("mapped");
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            out.extend_from_slice(&data[(y * row) as usize..(y * row + w * 4) as usize]);
        }
        out
    }
}

/// The panel's fixed case: an interior cell, the Modern preset, one interior value of the file.
fn fixed_panel(panel: &mut Panel) -> impl FnMut(&egui::Context) + '_ {
    let specs = builtin::specs();
    let presets = builtin::presets();
    let file =
        SettingsFile::parse("version = 1\npreset = \"modern\"\n[interior.ao]\nstrength = 0.5\n");
    let place = Place {
        worldspace: None,
        interior: true,
        cell: Some(PlaceName::editor_id("GSDocMitchellHouse")),
    };
    move |ctx| {
        let resolved = resolve(
            &Inputs {
                specs: &specs,
                presets: &presets,
                file: Some(&file),
                override_preset: None,
                overrides: None,
            },
            &place,
        );
        let view = View {
            specs: &specs,
            presets: &presets,
            resolved: &resolved,
            started: None,
            file: Some(&file),
            place: &place,
        };
        let _ = panel.show(ctx, &view);
    }
}

/// The fixed panel drawn over a grey frame of `size` at `scale`, in its `frames`th frame (egui
/// places a window in its first frame and draws it from the second), read back.
fn draw(d: &wgpu::Device, q: &wgpu::Queue, size: (u32, u32), scale: f32, frames: u32) -> Vec<u8> {
    let frame = Frame::new(d, size);
    let mut painter = d3d9_wgpu::gfx_overlay::painter(d, FORMAT);
    let mut panel = Panel::default();
    let mut px = Vec::new();
    for _ in 0..frames {
        let mut e = d.create_command_encoder(&Default::default());
        frame.clear(&mut e);
        painter.draw(
            d,
            q,
            &mut e,
            &frame.view,
            size,
            scale,
            Vec::new(),
            fixed_panel(&mut panel),
        );
        px = frame.read(d, q, e);
    }
    px
}

/// The smallest rectangle (x0, y0, x1, y1) holding every pixel that is not the frame's grey.
fn changed(px: &[u8], w: u32) -> Option<(u32, u32, u32, u32)> {
    let mut r: Option<(u32, u32, u32, u32)> = None;
    for (i, p) in px.chunks_exact(4).enumerate() {
        if p[..3] != GREY[..3] {
            let (x, y) = (i as u32 % w, i as u32 / w);
            r = Some(r.map_or((x, y, x, y), |(a, b, c, d)| {
                (a.min(x), b.min(y), c.max(x), d.max(y))
            }));
        }
    }
    r
}

/// The panel is drawn over the frame, only in its own rectangle at the top left; inside it is the
/// dark plate and light text; the same input draws the same pixels; at scale 2 the panel is twice
/// the size. The pictures are the panel's goldens to look at (`NV_OVERLAY_PICTURE`).
#[test]
fn the_panel_is_drawn_over_the_frame_in_its_own_rectangle() {
    let Some((d, q, _)) = device() else { return };
    for (size, scale) in [((1280, 720), 1.0), ((2560, 1440), 2.0)] {
        let px = draw(&d, &q, size, scale, 3);
        if let Some(dir) = std::env::var_os("NV_OVERLAY_PICTURE") {
            let file =
                std::path::Path::new(&dir).join(format!("overlay-{}x{}.bmp", size.0, size.1));
            std::fs::write(file, d3d9_wgpu::shot::bmp(size.0, size.1, &px)).expect("picture");
        }
        let (x0, y0, x1, y1) = changed(&px, size.0).expect("the panel is drawn");
        let (pw, ph) = (x1 - x0 + 1, y1 - y0 + 1);
        assert!(
            x0 < 40 * scale as u32 && y0 < 40 * scale as u32,
            "{size:?}: at {x0},{y0}"
        );
        assert!(pw < size.0 * 3 / 4 && ph < size.1, "{size:?}: {pw}x{ph}");
        assert!(
            pw > 200 * scale as u32 && ph > 150 * scale as u32,
            "{size:?}: {pw}x{ph}"
        );
        // The plate is dark and there is light text on it.
        let at = |x: u32, y: u32| &px[((y * size.0 + x) * 4) as usize..][..3];
        let inside: Vec<&[u8]> = (y0..=y1)
            .step_by(2)
            .flat_map(|y| (x0..=x1).step_by(2).map(move |x| (x, y)))
            .map(|(x, y)| at(x, y))
            .collect();
        let dark = inside.iter().filter(|p| p.iter().all(|&c| c < 50)).count();
        let light = inside.iter().filter(|p| p.iter().all(|&c| c > 180)).count();
        assert!(
            dark * 2 > inside.len(),
            "{size:?}: plate {dark} of {}",
            inside.len()
        );
        assert!(light > 200, "{size:?}: text pixels {light}");
        assert_eq!(
            px,
            draw(&d, &q, size, scale, 3),
            "{size:?}: the same pixels again"
        );
    }
}

/// The cost of the open panel: the CPU's layout and tessellation, and the card's time for its
/// pass (timestamps), at 2560x1440 and scale 2, over 300 frames. Prints one line.
#[test]
#[ignore = "a measurement: NV_OVERLAY_BENCH=1 cargo test -p d3d9-wgpu --test gfx_overlay -- --ignored --nocapture"]
fn the_open_panel_costs() {
    let Some((d, q, features)) = device() else {
        return;
    };
    let size = (2560, 1440);
    let frame = Frame::new(&d, size);
    let mut painter = d3d9_wgpu::gfx_overlay::painter(&d, FORMAT);
    let mut panel = Panel::default();
    let timed = features.contains(
        wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS,
    );
    let set = timed.then(|| {
        d.create_query_set(&wgpu::QuerySetDescriptor {
            label: None,
            ty: wgpu::QueryType::Timestamp,
            count: 2,
        })
    });
    let resolve_buf = d.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let read_buf = d.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let (mut cpu, mut gpu, mut n) = (Vec::new(), Vec::new(), 0);
    for i in 0..330 {
        let mut e = d.create_command_encoder(&Default::default());
        frame.clear(&mut e);
        if let Some(s) = &set {
            e.write_timestamp(s, 0);
        }
        let t = std::time::Instant::now();
        let stats = painter.draw(
            &d,
            &q,
            &mut e,
            &frame.view,
            size,
            2.0,
            Vec::new(),
            fixed_panel(&mut panel),
        );
        let recorded = t.elapsed();
        if let Some(s) = &set {
            e.write_timestamp(s, 1);
            e.resolve_query_set(s, 0..2, &resolve_buf, 0);
            e.copy_buffer_to_buffer(&resolve_buf, 0, &read_buf, 0, 16);
        }
        q.submit([e.finish()]);
        if timed {
            read_buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            let _ = d.poll(wgpu::PollType::wait_indefinitely());
            let ticks = {
                let m = read_buf.slice(..).get_mapped_range().expect("mapped");
                let a = u64::from_le_bytes(m[0..8].try_into().unwrap());
                let b = u64::from_le_bytes(m[8..16].try_into().unwrap());
                b.saturating_sub(a)
            };
            read_buf.unmap();
            if i >= 30 {
                gpu.push(ticks as f64 * f64::from(q.get_timestamp_period()) / 1e6);
            }
        } else {
            let _ = d.poll(wgpu::PollType::wait_indefinitely());
        }
        if i >= 30 {
            cpu.push(recorded.as_secs_f64() * 1e3);
            n = stats.vertices;
        }
    }
    let median = |v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v.get(v.len() / 2).copied().unwrap_or(f64::NAN)
    };
    println!(
        "overlay open 2560x1440 scale 2: cpu (layout, tessellation, upload, record) median {:.3} ms, card median {:.3} ms, {n} vertices",
        median(&mut cpu),
        median(&mut gpu)
    );
}
