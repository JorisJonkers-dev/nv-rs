// SPDX-License-Identifier: GPL-3.0-only
//! The panel's drawing: an egui context with the port's font and the overlay's look, and
//! egui-wgpu's renderer recording into the window's frame. Made when the overlay first opens; a
//! closed overlay has none, and costs the caller one check.

use crate::font::{truetype, Glyph};
use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId, TextStyle};

/// The text size in points: two screen pixels to a font pixel at a scale of 1.
pub const TEXT_POINTS: f32 = 16.0;

/// What one frame of the panel takes: the time spent laying it out and tessellating it, and the
/// triangles it draws (for the cost line of the bench and the tests).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub layout_us: u64,
    pub vertices: usize,
    pub indices: usize,
}

pub struct Painter {
    ctx: egui::Context,
    renderer: egui_wgpu::Renderer,
    pub format: wgpu::TextureFormat,
    started: std::time::Instant,
    /// The frames drawn (tests).
    pub drawn: u64,
}

/// The overlay's context: the port's font for every text style, a readable dark look, no
/// animations (a change shows in the very next frame).
pub fn context(ascii: &[Glyph; 95]) -> egui::Context {
    let ctx = egui::Context::default();
    let mut fonts = FontDefinitions::empty();
    fonts.font_data.insert(
        "nv-rs".to_owned(),
        std::sync::Arc::new(FontData::from_owned(truetype(ascii))),
    );
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts.families.insert(family, vec!["nv-rs".to_owned()]);
    }
    ctx.set_fonts(fonts);
    let mut style = (*ctx.global_style()).clone();
    for (s, size) in [
        (TextStyle::Small, TEXT_POINTS),
        (TextStyle::Body, TEXT_POINTS),
        (TextStyle::Button, TEXT_POINTS),
        (TextStyle::Monospace, TEXT_POINTS),
        (TextStyle::Heading, TEXT_POINTS * 1.5),
    ] {
        let family = if s == TextStyle::Monospace {
            FontFamily::Monospace
        } else {
            FontFamily::Proportional
        };
        style.text_styles.insert(s, FontId::new(size, family));
    }
    style.animation_time = 0.0;
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.slider_width = 200.0;
    let mut v = egui::Visuals::dark();
    v.window_fill = Color32::from_rgba_unmultiplied(20, 22, 24, 236);
    v.panel_fill = v.window_fill;
    v.override_text_color = Some(Color32::from_rgb(222, 222, 216));
    style.visuals = v;
    ctx.set_global_style(style);
    ctx
}

impl Painter {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, ascii: &[Glyph; 95]) -> Painter {
        let renderer = egui_wgpu::Renderer::new(
            device,
            format,
            egui_wgpu::RendererOptions {
                // The same pixels on every card (the panel's golden pictures).
                dithering: false,
                ..Default::default()
            },
        );
        Painter {
            ctx: context(ascii),
            renderer,
            format,
            started: std::time::Instant::now(),
            drawn: 0,
        }
    }

    pub fn context(&self) -> &egui::Context {
        &self.ctx
    }

    /// Lays out the panel with `ui` over a `size` frame at `scale` pixels to a point (`events`:
    /// the input since the last frame) and records it into `view`, over what the frame holds.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        size: (u32, u32),
        scale: f32,
        events: Vec<egui::Event>,
        ui: impl FnMut(&egui::Context),
    ) -> Stats {
        let t = std::time::Instant::now();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(size.0 as f32 / scale, size.1 as f32 / scale),
            )),
            time: Some(self.started.elapsed().as_secs_f64()),
            max_texture_side: Some(device.limits().max_texture_dimension_2d as usize),
            events,
            focused: true,
            ..Default::default()
        };
        self.ctx.set_pixels_per_point(scale);
        let mut out = self.ctx.run_pass(input, ui);
        let jobs = self.ctx.tessellate(out.shapes, out.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [size.0, size.1],
            pixels_per_point: out.pixels_per_point,
        };
        let layout_us = t.elapsed().as_micros() as u64;
        for (id, deltas) in &out.textures_delta.set {
            for delta in deltas {
                self.renderer.update_texture(device, queue, *id, delta);
            }
        }
        let extra = self
            .renderer
            .update_buffers(device, queue, encoder, &jobs, &screen);
        debug_assert!(extra.is_empty(), "the panel has no paint callbacks");
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("graphics overlay"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
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
                })
                .forget_lifetime();
            self.renderer.render(&mut pass, &jobs, &screen);
        }
        for id in &out.textures_delta.free {
            self.renderer.free_texture(id);
        }
        out.textures_delta.clear();
        self.drawn += 1;
        let (vertices, indices) = jobs.iter().fold((0, 0), |(v, i), j| match &j.primitive {
            egui::epaint::Primitive::Mesh(m) => (v + m.vertices.len(), i + m.indices.len()),
            egui::epaint::Primitive::Callback(_) => (v, i),
        });
        Stats {
            layout_us,
            vertices,
            indices,
        }
    }
}

#[cfg(test)]
mod tests {
    /// At the panel's text size every character takes one cell of 6 by 8 font pixels, two screen
    /// pixels each, and its glyph is drawn from whole pixels: a full 5 by 7 block is 10 by 14.
    #[test]
    fn text_is_laid_out_in_whole_cells() {
        let ascii = [[0b11111u8; 7]; 95];
        let ctx = super::context(&ascii);
        let mut out = ctx.run_pass(Default::default(), |ctx| {
            let g = ctx.fonts_mut(|f| {
                f.layout_no_wrap(
                    "AbcA".into(),
                    egui::FontId::proportional(super::TEXT_POINTS),
                    egui::Color32::WHITE,
                )
            });
            assert_eq!(g.size(), egui::vec2(48.0, 16.0));
            let glyphs: Vec<_> = g.rows.iter().flat_map(|r| &r.row.glyphs).collect();
            for (i, gl) in glyphs.iter().enumerate() {
                assert_eq!(gl.pos.x, 12.0 * i as f32);
                assert_eq!(gl.advance_width, 12.0);
                assert_eq!(gl.uv_rect.size, egui::vec2(10.0, 14.0));
                assert_eq!(gl.uv_rect.offset.x, 0.0);
            }
        });
        out.textures_delta.clear();
    }

    fn block<F: std::future::Future>(f: F) -> F::Output {
        let mut f = std::pin::pin!(f);
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        loop {
            if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
                break v;
            }
        }
    }

    /// A frame of a label drawn on this machine's first card (none: skipped): the panel's pass is
    /// recorded and submitted without a validation error, and the numbers count its triangles.
    #[test]
    fn a_frame_is_recorded_on_a_card() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let Ok(adapter) = block(instance.request_adapter(&Default::default())) else {
            return;
        };
        let Ok((device, queue)) = block(adapter.request_device(&Default::default())) else {
            return;
        };
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 256,
                height: 128,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        let mut p = super::Painter::new(&device, format, &[[0b11111; 7]; 95]);
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        // egui sizes a new area in its first frame and shows it from the second.
        let mut stats = super::Stats::default();
        for _ in 0..2 {
            let mut e = device.create_command_encoder(&Default::default());
            stats = p.draw(
                &device,
                &queue,
                &mut e,
                &view,
                (256, 128),
                1.0,
                Vec::new(),
                |ctx| {
                    egui::Area::new(egui::Id::unique("label")).show(ctx, |ui| ui.label("AB"));
                },
            );
            queue.submit([e.finish()]);
        }
        assert!(block(scope.pop()).is_none());
        assert_eq!(p.drawn, 2);
        assert!(stats.vertices >= 8 && stats.indices >= 12, "{stats:?}");
    }
}
