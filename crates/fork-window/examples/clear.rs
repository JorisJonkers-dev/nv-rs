//! Opens a window and shows cleared frames through wgpu on Vulkan.
//!
//!   cargo run -p fork-window --example clear            until the window is closed or Escape
//!   cargo run -p fork-window --example clear -- --smoke 30 frames, then exit 0
//!   ... --smoke --hold 5 --fullscreen      keep showing frames 5 s after the 30th; borderless over a monitor
//!   ... --smoke --place 1280x720            open at 320x240, then ask for 1280x720 as the game's `SetWindowPos` does
//!
//! Exits 1 with the reason when there is no display, no Vulkan adapter or no frame.

#[cfg(all(unix, not(target_os = "macos")))]
fn main() {
    use fork_window::{dik, Window, WindowRequest};
    let args: Vec<String> = std::env::args().collect();
    let smoke = args.iter().any(|a| a == "--smoke");
    let fullscreen = args.iter().any(|a| a == "--fullscreen");
    let hold = args
        .iter()
        .position(|a| a == "--hold")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<f64>().ok())
        .map_or(
            std::time::Duration::ZERO,
            std::time::Duration::from_secs_f64,
        );
    // The game opens its window small and then sizes it to its prefs (nv-rs#835).
    let place = args
        .iter()
        .position(|a| a == "--place")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.split_once('x'))
        .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)));
    let mut done_at: Option<std::time::Instant> = None;
    let fail = |why: String| -> ! {
        eprintln!("clear: {why}");
        std::process::exit(1)
    };
    // The monitor query comes first, as the game's settings do: the window must still be made
    // (winit makes one event loop per process; both use it).
    match fork_window::display() {
        Some(d) => println!(
            "display: monitor {} {}x{} {} Hz",
            d.index,
            d.monitor.width,
            d.monitor.height,
            d.refresh_hz()
        ),
        None => println!("display: none known"),
    }
    let window = Window::open(
        WindowRequest {
            title: "nv-rs window layer".into(),
            width: if place.is_some() { 320 } else { 640 },
            height: if place.is_some() { 240 } else { 360 },
            fullscreen,
            decorations: !fullscreen,
            ..Default::default()
        },
        None,
    )
    .unwrap_or_else(|e| fail(e));
    let id = window.id();
    if let Some(size) = place {
        fork_window::place(id, None, Some(size));
        println!("placed: asked {}x{}", size.0, size.1);
    }
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let surface = fork_window::surface_for(&instance, id).unwrap_or_else(|e| fail(e));
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        compatible_surface: Some(&surface),
        ..Default::default()
    }))
    .unwrap_or_else(|e| fail(format!("no adapter: {e}")));
    let info = adapter.get_info();
    println!(
        "adapter: {} ({:?}, {:?})",
        info.name, info.device_type, info.backend
    );
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
        .unwrap_or_else(|e| fail(format!("device: {e}")));
    let (w, h) = fork_window::client_size(id);
    let mut config = surface
        .get_default_config(&adapter, w.max(1), h.max(1))
        .unwrap_or_else(|| fail("the surface has no format".into()));
    surface.configure(&device, &config);
    println!("window {w}x{h}, format {:?}", config.format);

    let (mut shown, mut tries) = (0u32, 0u32);
    while !fork_window::close_requested(id) {
        let input = fork_window::take_input(id);
        if input.keys[dik::by_name("Escape").unwrap() as usize] {
            break;
        }
        let (w, h) = fork_window::client_size(id);
        if (w, h) != (config.width, config.height) && w > 0 && h > 0 {
            config.width = w;
            config.height = h;
            surface.configure(&device, &config);
        }
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            other => {
                tries += 1;
                if tries > 600 {
                    fail(format!("no frame: {other:?}"));
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
                continue;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        let t = shown as f64 / 60.0;
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.1 + 0.1 * t.sin().abs(),
                        g: 0.3,
                        b: 0.5,
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
        queue.present(frame);
        shown += 1;
        if smoke && shown >= 30 {
            let at = *done_at.get_or_insert_with(|| {
                println!("smoke: {shown} frames shown");
                std::time::Instant::now()
            });
            if at.elapsed() >= hold {
                let (w, h) = fork_window::client_size(id);
                println!(
                    "final: window {w}x{h} surface {}x{}",
                    config.width, config.height
                );
                break;
            }
        }
    }
    // Surface first, then the window (the layer's contract).
    drop(surface);
    drop(window);
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn main() {
    eprintln!("clear: this platform's window layer is not part of fork-window");
    std::process::exit(2);
}

#[cfg(all(unix, not(target_os = "macos")))]
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    struct Noop;
    impl std::task::Wake for Noop {
        fn wake(self: std::sync::Arc<Self>) {}
    }
    let waker = std::task::Waker::from(std::sync::Arc::new(Noop));
    let mut cx = std::task::Context::from_waker(&waker);
    let mut f = std::pin::pin!(f);
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}
