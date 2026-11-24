// SPDX-License-Identifier: GPL-3.0-only
//! A small window for a run that draws off screen, so the run has an entry in the dock.
//!
//! Off by default: `NV_RUN_WINDOW=1` (`gamerun` exports it) asks for it ([`wanted`]). The window is
//! 320x180, opens without taking the focus and is minimised at once, named by the run's label
//! ([`crate::identity`]). The run draws exactly as before: the mirror owns a surface and a device of
//! its own and reads nothing of the game's, so its pictures cannot change. For now it shows a flat
//! colour taken from the label (so two runs differ at a glance); the run's own picture is not
//! copied into it yet.

use crate::{Window, WindowRequest};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Width and height of the mirror, in pixels.
pub const SIZE: (u32, u32) = (320, 180);

/// Whether the environment asks for the window: `NV_RUN_WINDOW` set to anything but `0` or empty.
pub fn wanted() -> bool {
    enabled(std::env::var("NV_RUN_WINDOW").ok().as_deref())
}

/// [`wanted`] for a given value (pure, for tests).
pub fn enabled(value: Option<&str>) -> bool {
    value.is_some_and(|v| !v.is_empty() && v != "0")
}

/// The colour of a label's window: its bytes hashed (FNV-1a) into three channels, kept dark enough
/// to read as a placeholder. Without a label a neutral grey.
pub fn colour_of(label: Option<&str>) -> [f64; 3] {
    let Some(l) = label else {
        return [0.12, 0.12, 0.12];
    };
    let mut h: u32 = 0x811c_9dc5;
    for b in l.bytes() {
        h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
    }
    let c = |shift: u32| 0.10 + f64::from((h >> shift) & 0xff) / 255.0 * 0.35;
    [c(0), c(8), c(16)]
}

/// The open window and the thread that keeps it painted. Dropping it stops the thread and closes
/// the window.
pub struct Mirror {
    window: Window,
    stop: Arc<AtomicBool>,
    /// Frames shown, and whether the painting thread has ended (no adapter, or stopped).
    shown: Arc<(AtomicU64, AtomicBool)>,
}

impl Mirror {
    /// How many frames the window has shown.
    pub fn frames(&self) -> u64 {
        self.shown.0.load(Ordering::Relaxed)
    }

    /// Whether the painting thread has ended (the machine has no adapter for the window, or the
    /// window is closed).
    pub fn finished(&self) -> bool {
        self.shown.1.load(Ordering::Relaxed)
    }

    /// The window's id in this layer.
    pub fn id(&self) -> isize {
        self.window.id()
    }
}

impl Drop for Mirror {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Opens the window for the run `label` (title `title`, as the game would name its own) and starts
/// painting it. An error says why there is no window; the run goes on without one.
pub fn open(label: Option<&str>, title: &str) -> Result<Mirror, String> {
    let request = WindowRequest {
        title: title.to_owned(),
        width: SIZE.0,
        height: SIZE.1,
        decorations: true,
        visible: true,
        fullscreen: false,
        activate: false,
    };
    let window = Window::open(request, None)?;
    let id = window.id();
    crate::set_minimised(id, true);
    let stop = Arc::new(AtomicBool::new(false));
    let colour = colour_of(label);
    let flag = stop.clone();
    let shown = Arc::new((AtomicU64::new(0), AtomicBool::new(false)));
    let counter = shown.clone();
    std::thread::Builder::new()
        .name("run window".into())
        .spawn(move || {
            if let Err(why) = paint(id, colour, &flag, &counter.0) {
                fork_log::warn!(target: "window", "the run's window shows no picture: {why}");
            }
            counter.1.store(true, Ordering::Relaxed);
        })
        .map_err(|e| format!("thread: {e}"))?;
    Ok(Mirror {
        window,
        stop,
        shown,
    })
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    struct Noop;
    impl std::task::Wake for Noop {
        fn wake(self: Arc<Self>) {}
    }
    let waker = std::task::Waker::from(Arc::new(Noop));
    let mut cx = std::task::Context::from_waker(&waker);
    let mut f = std::pin::pin!(f);
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

/// Clears the window's surface to `colour` about twice a second until `stop`.
fn paint(id: isize, colour: [f64; 3], stop: &AtomicBool, frames: &AtomicU64) -> Result<(), String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let surface = crate::surface_for(&instance, id)?;
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: false,
        compatible_surface: Some(&surface),
        apply_limit_buckets: false,
    }))
    .map_err(|e| format!("adapter: {e}"))?;
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("run window"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        experimental_features: Default::default(),
        trace: Default::default(),
    }))
    .map_err(|e| format!("device: {e}"))?;
    let caps = surface.get_capabilities(&adapter);
    let format = *caps.formats.first().ok_or("the surface has no format")?;
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: SIZE.0,
        height: SIZE.1,
        present_mode: wgpu::PresentMode::Fifo,
        color_space: wgpu::SurfaceColorSpace::Auto,
        desired_maximum_frame_latency: 2,
        alpha_mode: wgpu::CompositeAlphaMode::Auto,
        view_formats: vec![],
    };
    surface.configure(&device, &config);
    while !stop.load(Ordering::Relaxed) {
        match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                let view = t
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let mut encoder =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
                drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: colour[0],
                                g: colour[1],
                                b: colour[2],
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                }));
                queue.submit([encoder.finish()]);
                queue.present(t);
                frames.fetch_add(1, Ordering::Relaxed);
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                surface.configure(&device, &config);
            }
            _ => {}
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_is_off_unless_asked_for() {
        assert!(!enabled(None));
        assert!(!enabled(Some("")));
        assert!(!enabled(Some("0")));
        assert!(enabled(Some("1")));
    }

    #[test]
    fn a_label_gives_its_own_dark_colour() {
        let a = colour_of(Some("gate14-mojave-ttw"));
        assert_eq!(a, colour_of(Some("gate14-mojave-ttw")));
        assert_ne!(a, colour_of(Some("bench-views-lake-mead")));
        assert!(a.iter().all(|c| (0.10..=0.45).contains(c)));
        assert_eq!(colour_of(None), [0.12, 0.12, 0.12]);
    }
}
