//! Fork-owned (docs/fork/LINUX.md, "Direct3D 9 off Windows"): the Direct3D 9 recorder of
//! `d3d9real.rs` off Windows. The recorder itself (the calls recorded as `d3d9_stream::Cmd`) is the
//! one the Windows build runs; what is Windows only there (the real `d3d9.dll`, the game's window,
//! the F12 report) is left out, and the stream's backend is wgpu alone, off screen
//! (`d3d9_wgpu::Output::Offscreen`): no window is made, the frames are read back by a test or
//! a tool (`NV_D3D9_RECORD` files replay with `d3d9-replay`).

use super::*;
use std::sync::atomic::{AtomicIsize, Ordering};

const WS_CAPTION: u32 = 0x00C0_0000;
const WS_VISIBLE: u32 = 0x1000_0000;

/// The id of the first top-level real window (the game's main window), 0 for none: its keys and
/// mouse are the game's DirectInput devices' ([`super::held_keys`], [`super::take_input`]).
static MAIN_WINDOW: AtomicIsize = AtomicIsize::new(0);

/// The game's real window's id, when there is one.
pub(super) fn main_window() -> Option<isize> {
    Some(MAIN_WINDOW.load(Ordering::Relaxed)).filter(|&w| w != 0)
}

/// The game's top-level windows (by modelled handle): their `SetWindowPos` rectangle is on the
/// screen, a child's is in its parent's client area.
static TOPS: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());

/// `SWP_NOSIZE`, `SWP_NOMOVE`: the game's `SetWindowPos` flags keeping the size or the place.
const SWP_NOSIZE: u32 = 1;
const SWP_NOMOVE: u32 = 2;

/// The windows made, kept for the run (dropping one closes it).
static KEPT: std::sync::Mutex<Vec<fork_window::Window>> = std::sync::Mutex::new(Vec::new());

/// Whether the game's windows are real ones: a display is there (X11 or Wayland; macOS always has
/// one) and `NV_D3D9_OFFSCREEN` is not set (CI without a display, and tests, stay off screen).
fn windows_wanted() -> bool {
    let set = |n: &str| std::env::var_os(n).is_some_and(|v| !v.is_empty());
    wanted()
        && !set("NV_D3D9_OFFSCREEN")
        && (cfg!(target_os = "macos") || set("DISPLAY") || set("WAYLAND_DISPLAY"))
}

/// macOS: the fork-window window `id` as the device layer's window registry knows a window
/// (`d3d9_wgpu::register_window`): the surface is made on its Metal layer, made on the main thread.
#[cfg(target_os = "macos")]
struct MacWindow(isize);

#[cfg(target_os = "macos")]
impl raw_window_handle::HasWindowHandle for MacWindow {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let (_, window) =
            fork_window::raw_handles(self.0).ok_or(raw_window_handle::HandleError::Unavailable)?;
        // SAFETY: fork-window keeps the window alive while it is registered (`KEPT` holds it for
        // the run); the handle is only read, on the device layer's terms.
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(window) })
    }
}

#[cfg(target_os = "macos")]
impl raw_window_handle::HasDisplayHandle for MacWindow {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        let (display, _) =
            fork_window::raw_handles(self.0).ok_or(raw_window_handle::HandleError::Unavailable)?;
        // SAFETY: as for the window handle above.
        Ok(unsafe { raw_window_handle::DisplayHandle::borrow_raw(display) })
    }
}

#[cfg(target_os = "macos")]
impl d3d9_wgpu::GameWindow for MacWindow {
    fn client_size(&self) -> Option<(u32, u32)> {
        Some(fork_window::client_size(self.0)).filter(|&(w, h)| w > 0 && h > 0)
    }

    fn minimised(&self) -> bool {
        false
    }

    fn refresh_millihertz(&self) -> Option<u32> {
        fork_window::display()
            .map(|d| d.monitor.refresh_mhz)
            .filter(|&mhz| mhz > 0)
    }

    fn metal_layer(&self) -> Option<std::ptr::NonNull<std::ffi::c_void>> {
        fork_window::metal_layer(self.0)
    }
}

/// The small window of a run that draws off screen (`NV_RUN_WINDOW`, `fork_window::mirror`), made once
/// for the run's first top-level window and kept to its end. Off unless asked for; a run without a
/// display, or any run on macOS, has none. The run's drawing is not touched.
#[cfg(not(target_os = "macos"))]
fn run_window(title: &[u8]) {
    static MIRROR: std::sync::Mutex<Option<fork_window::mirror::Mirror>> =
        std::sync::Mutex::new(None);
    let display = ["DISPLAY", "WAYLAND_DISPLAY"]
        .iter()
        .any(|n| std::env::var_os(n).is_some_and(|v| !v.is_empty()));
    if !fork_window::mirror::wanted() || !display || !wanted() {
        return;
    }
    let mut held = MIRROR.lock().unwrap_or_else(|p| p.into_inner());
    if held.is_some() {
        return;
    }
    let label = fork_window::run_label();
    match fork_window::mirror::open(label.as_deref(), &String::from_utf8_lossy(title)) {
        Ok(m) => {
            fork_log::info!(target: "window", "the run's window (minimised, off screen run) is {:#x}", m.id());
            *held = Some(m);
        }
        Err(why) => fork_log::warn!(target: "window", "no window for the off screen run: {why}"),
    }
}

#[cfg(target_os = "macos")]
fn run_window(_title: &[u8]) {}

/// `CreateWindowExA` made the modelled window `handle`: a real window of its title and size (a
/// child shares its parent's: one surface covers both). No caption (`WS_POPUP`: the game's
/// full screen) is a borderless window over the monitor. The window thread, and with it the
/// game's keys and mouse, close request and resizes, is `fork_window`'s. When the window cannot
/// be made the run stays off screen.
pub fn window_created(
    e: &mut Engine,
    handle: u32,
    title: &[u8],
    style: u32,
    _ex_style: u32,
    rect: [i32; 4],
    parent: u32,
) {
    if !windows_wanted() {
        if parent == 0 {
            run_window(title);
        }
        return;
    }
    if parent != 0 {
        if let Some(w) = real_window(e, parent).filter(|&w| w != 0) {
            let _device = engine_core::real_threads::device_guard(e);
            real(e).windows.insert(handle, w);
        }
        return;
    }
    let [left, top, right, bottom] = rect;
    let request = fork_window::WindowRequest {
        title: String::from_utf8_lossy(title).into_owned(),
        width: right.wrapping_sub(left).clamp(1, 16384) as u32,
        height: bottom.wrapping_sub(top).clamp(1, 16384) as u32,
        decorations: style & WS_CAPTION == WS_CAPTION,
        visible: style & WS_VISIBLE != 0,
        fullscreen: style & WS_CAPTION == 0,
        activate: true,
    };
    let (width, height, borderless) = (request.width, request.height, request.fullscreen);
    match fork_window::Window::open(request, Some(d3d9_wgpu::window_changed)) {
        Ok(window) => {
            let id = window.id();
            fork_log::info!(target: "d3d9",
                "NV_D3D9={}: window {handle:#x} ({width}x{height}{}) is real window {id:#x}",
                tag(),
                if borderless {
                    ", borderless over the monitor"
                } else {
                    ""
                }
            );
            #[cfg(target_os = "macos")]
            d3d9_wgpu::register_window(id, std::sync::Arc::new(MacWindow(id)));
            let _ = MAIN_WINDOW.compare_exchange(0, id, Ordering::Relaxed, Ordering::Relaxed);
            TOPS.lock().unwrap_or_else(|p| p.into_inner()).push(handle);
            KEPT.lock().unwrap_or_else(|p| p.into_inner()).push(window);
            watch_close(id);
            let _device = engine_core::real_threads::device_guard(e);
            real(e).windows.insert(handle, id);
        }
        Err(error) => fork_log::warn!(target: "d3d9",
            "NV_D3D9={}: the real window could not be made ({error}): off screen",
            tag()
        ),
    }
}

/// Closing the game's window ends the run, as on Windows (`WM_CLOSE`): the exit hooks run (the
/// saves' copy-out), then the process ends.
fn watch_close(id: isize) {
    let spawned = std::thread::Builder::new()
        .name("nv-window-close".into())
        .spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(50));
            if d3d9_wgpu::device_lost() {
                fork_log::fatal!(target: "d3d9", 
                    "NV_D3D9: the graphics device was lost: the run ends (see the line above)"
                );
                crate::run_exit_hooks();
                crate::end_process(1);
            }
            if fork_window::close_requested(id) {
                fork_log::info!(target: "d3d9", "NV_D3D9: the game's window was closed: the run ends");
                crate::run_exit_hooks();
                crate::end_process(0);
            }
        });
    if let Err(error) = spawned {
        fork_log::warn!(target: "d3d9", "NV_D3D9: no watch on the window's close request ({error})");
    }
}

/// The game's title bytes (its code page's; the game's titles are ASCII) as text.
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

/// `SetWindowTextA`: the real window takes the title the game gave its record.
pub fn window_titled(e: &mut Engine, handle: u32, title: &[u8]) {
    if let Some(id) = real_window(e, handle).filter(|&w| w != 0) {
        fork_window::set_title(id, &latin1(title));
    }
}

/// The client area for the rectangle `rect` the game asked its window for, less the frame
/// `border` the game added for it (the renderer's start calls `AdjustWindowRect` with the
/// window's style, then `SetWindowPos`): the size the game wanted, whatever frame the desktop
/// draws round it. At least 1 by 1.
fn client_for(rect: [i32; 4], border: [i32; 4]) -> (u32, u32) {
    let side = |a: i32, b: i32, low: i32, high: i32| {
        let want = i64::from(b) - i64::from(a);
        let frame = i64::from(high) - i64::from(low);
        (want - frame).clamp(1, i64::from(u32::MAX)) as u32
    };
    (
        side(rect[0], rect[2], border[0], border[2]),
        side(rect[1], rect[3], border[1], border[3]),
    )
}

/// `SetWindowPos`: the real window follows the game's rectangle (its client area the rectangle
/// less the frame the game added for it, `SWP_NOSIZE` and `SWP_NOMOVE` keep what they name). The
/// borderless window (the game's full screen) keeps the monitor, and the swap chain follows the
/// window's size (docs/fork/LINUX.md, "Display match"). A child window shares its parent's real
/// window: it sizes the parent's client area and never moves it.
pub fn window_moved(e: &mut Engine, handle: u32, rect: [i32; 4], flags: u32) {
    let Some(id) = real_window(e, handle).filter(|&w| w != 0) else {
        return;
    };
    let top = TOPS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .contains(&handle);
    let border = if top {
        crate::window::game_frame(e, handle).unwrap_or([0; 4])
    } else {
        [0; 4]
    };
    let size = (flags & SWP_NOSIZE == 0).then(|| client_for(rect, border));
    let position = (top && flags & SWP_NOMOVE == 0).then_some((rect[0], rect[1]));
    fork_window::place(id, position, size);
}

/// `d3d9real::run_on_main_thread` (the viewer hands the main thread over): on Linux fork-window
/// needs no main thread, so `game` runs here and the process ends when it returns.
#[cfg(not(target_os = "macos"))]
pub fn run_on_main_thread(game: Box<dyn FnOnce() + Send>) -> ! {
    game();
    crate::end_process(0)
}

/// `d3d9real::run_on_main_thread` on macOS: the window loop takes the main thread (AppKit's
/// windows are the main thread's only) and `game` runs on a thread of its own; the process ends
/// when it returns (101 when it panicked).
#[cfg(target_os = "macos")]
pub fn run_on_main_thread(game: Box<dyn FnOnce() + Send>) -> ! {
    fork_window::run_on_main_thread(game, crate::end_process)
}

/// The monitors for upstream's display part of the machine table (`machine_table::this_display`):
/// the one fork-window chose (`fork_window::display`), at its current mode. The host does not ask
/// for that part (the fork's adapter table answers the display), so this is for other callers.
pub fn monitors() -> Result<Vec<crate::machine_table::Monitor>, String> {
    let d = fork_window::display().ok_or("fork-window found no display")?;
    let m = d.monitor;
    let rate = (m.refresh_mhz != 0).then_some(m.refresh_mhz);
    Ok(vec![crate::machine_table::Monitor {
        size: (m.width, m.height),
        scale: 1.0,
        refresh_millihertz: rate,
        modes: vec![crate::machine_table::Mode {
            width: m.width,
            height: m.height,
            bit_depth: 32,
            refresh_millihertz: m.refresh_mhz,
        }],
        primary: true,
    }])
}

/// The wgpu backend, off screen, on the stream `which` names: the real Direct3D 9 is not there,
/// so `real` and `real,wgpu` draw on wgpu too (a line says so). With the render thread, the
/// occlusion counts are posted for the game's side to read without waiting
/// (`d3d9_wgpu::QueryAnswers`), as the Windows build does for wgpu alone.
pub(super) fn make_stream(e: &mut Engine, which: Backends) -> Stream {
    let name = which.name();
    if which != Backends::Wgpu {
        fork_log::info!(target: "d3d9", "NV_D3D9={name}: there is no real Direct3D 9 here: wgpu draws instead");
    }
    let wanted_thread = real(e).render_thread;
    let (shader_settings, wgpu_cache) =
        d3d9_wgpu::shader_cache::plan(real(e).wgpu_cache.clone(), !cfg!(test));
    // To the game's window when windows are real ones (the stream is made at `Direct3DCreate9`,
    // before the window is known; a device with no real window stays off screen).
    let output = if windows_wanted() {
        d3d9_wgpu::Output::Window
    } else {
        d3d9_wgpu::Output::Offscreen
    };
    let backend = |answers: Option<std::sync::Arc<d3d9_wgpu::QueryAnswers>>| {
        let b = d3d9_wgpu::WgpuBackend::new(output).with_shader_cache(shader_settings.as_ref());
        let b = match wgpu_cache.clone() {
            Some(dir) => b.with_cache_dir(dir),
            None => b,
        };
        match answers {
            Some(a) => b.with_answers(a),
            None => b,
        }
    };
    let mut stream = if wanted_thread {
        let settings = d3d9_stream::Settings::from_env();
        let answers = std::sync::Arc::new(d3d9_wgpu::QueryAnswers::default());
        real(e).wgpu_answers = Some(answers.clone());
        fork_log::info!(target: "d3d9",
            "NV_D3D9={name}: the render thread is on (NV_RENDER_THREAD=0 turns it off; at most {} frame(s) in flight)",
            settings.frames_in_flight
        );
        Stream::threaded(Box::new(backend(Some(answers))), settings)
    } else {
        Stream::direct(Box::new(backend(None)))
    };
    if let Some(path) = std::env::var_os("NV_D3D9_RECORD") {
        let repeatable = e.repeatable;
        let recording = std::fs::File::create(&path).and_then(|file| {
            let out = Box::new(std::io::BufWriter::with_capacity(1 << 20, file));
            if repeatable {
                stream.record_repeatable(out)
            } else {
                stream.record_to(out)
            }
        });
        match recording {
            Ok(()) => fork_log::info!(target: "d3d9",
                "NV_D3D9_RECORD: every Direct3D 9 command goes to {}",
                std::path::Path::new(&path).display()
            ),
            Err(error) => fork_log::warn!(target: "d3d9",
                "NV_D3D9_RECORD: {} could not be written ({error}): not recorded",
                std::path::Path::new(&path).display()
            ),
        }
    }
    stream
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::d3d9;
    use d3d9_stream::Backend;

    /// The backend of the stream, shared with the test, which reads the frame back from it.
    struct Shared(std::sync::Arc<std::sync::Mutex<d3d9_wgpu::WgpuBackend>>);

    impl Backend for Shared {
        fn execute(&mut self, cmd: Cmd<Data<'_>>) -> Reply {
            self.0.lock().unwrap().execute(cmd)
        }
    }

    /// TEST DATA, not a capture: the adapter table of `d3d9device`'s tests (a 640x480 X8R8G8B8
    /// desktop, HAL caps, a windowed X8R8G8B8 back buffer accepted).
    fn table() -> String {
        let hex = |b: &[u8]| -> String { b.iter().map(|x| format!("{x:02x}")).collect() };
        let mut mode = vec![0u8; 16];
        mode[..4].copy_from_slice(&640u32.to_le_bytes());
        mode[4..8].copy_from_slice(&480u32.to_le_bytes());
        mode[8..12].copy_from_slice(&60u32.to_le_bytes());
        mode[12..].copy_from_slice(&0x16u32.to_le_bytes());
        let mut caps = vec![0u8; 304];
        caps[..4].copy_from_slice(&1u32.to_le_bytes());
        [
            format!("GetAdapterDisplayMode\t0\t0\t{}", hex(&mode)),
            format!("GetDeviceCaps\t0 1\t0\t{}", hex(&caps)),
            "Domain\td0\t0,1".to_string(),
            "CheckDeviceType\t0 1 16 16\t0\t\td0".to_string(),
        ]
        .join("\n")
    }

    /// A guest program through the engine: `Direct3DCreate9`, a device (64x64, X8R8G8B8, no
    /// window), a clear to green, a triangle drawn with a vertex and a pixel shader, `Present`.
    /// The recorder hands the calls to wgpu off screen (lavapipe on CI) and the back buffer is
    /// read back: the cleared colour in the corner, the triangle's colour at its centre.
    #[test]
    fn a_guest_program_draws_a_frame_through_the_recorder_on_wgpu() {
        guest_frame(None);
    }

    /// The same program with the game's window a real one (`fork_window`, X11 or Wayland) and
    /// the frames presented to it: the Xvfb smoke (`fork-window-smoke.yml`) sets
    /// `NV_WINDOW_SMOKE`; without it (no display) the test does nothing.
    #[test]
    fn a_guest_program_draws_to_a_window() {
        // One real window at a time (the layer has one): the window tests below take turns.
        let Some(_one_at_a_time) = window_session() else {
            return;
        };
        let window = fork_window::Window::open(
            fork_window::WindowRequest {
                width: 64,
                height: 64,
                ..Default::default()
            },
            None,
        )
        .expect("a window under the display");
        guest_frame(Some(window.id()));
        assert!(!fork_window::close_requested(window.id()));
    }

    /// The window session of the tests below: one real window at a time (the layer has one), and
    /// none where there is no display (`NV_WINDOW_SMOKE` unset).
    fn window_session() -> Option<std::sync::MutexGuard<'static, ()>> {
        static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
        if std::env::var_os("NV_WINDOW_SMOKE").is_none() {
            eprintln!("skipped: NV_WINDOW_SMOKE is not set");
            return None;
        }
        Some(ONE_AT_A_TIME.lock().unwrap_or_else(|p| p.into_inner()))
    }

    #[cfg(not(target_os = "macos"))]
    fn becomes<T: PartialEq + std::fmt::Debug>(what: &str, want: T, mut read: impl FnMut() -> T) {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let got = read();
            if got == want {
                return;
            }
            assert!(
                std::time::Instant::now() < until,
                "{what}: wanted {want:?}, still {got:?} after 5 s"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// The game's window `handle` as `window_created` leaves it: a real top-level window under
    /// the modelled handle, in an engine the game's calls go to.
    #[cfg(not(target_os = "macos"))]
    fn engine_with_window(
        request: fork_window::WindowRequest,
        handle: u32,
    ) -> (Engine, fork_window::Window, u32) {
        let window = fork_window::Window::open(request, None).expect("a window under the display");
        let x = fork_window::probe::x_window(window.id()).expect("an X11 window");
        let mut e = Engine::new();
        real(&mut e).windows.insert(handle, window.id());
        TOPS.lock().unwrap_or_else(|p| p.into_inner()).push(handle);
        (e, window, x)
    }

    /// #214: the game's `SetWindowTextA` after its window is made reaches the X11 property.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn the_games_set_window_text_reaches_the_x11_window() {
        let Some(_s) = window_session() else { return };
        let probe = fork_window::probe::Probe::connect();
        let (mut e, _window, x) = engine_with_window(
            fork_window::WindowRequest {
                title: "Fallout".into(),
                ..Default::default()
            },
            0x11c,
        );
        assert_eq!(probe.title(x).as_deref(), Some("Fallout"));
        window_titled(&mut e, 0x11c, b"Fallout: New Vegas - Mojave");
        becomes(
            "the title",
            Some("Fallout: New Vegas - Mojave".to_string()),
            || probe.title(x),
        );
        // A window the game never made (no real window) is left alone.
        window_titled(&mut e, 0x999, b"nobody");
        assert_eq!(
            probe.title(x).as_deref(),
            Some("Fallout: New Vegas - Mojave")
        );
    }

    /// #214: the game's `SetWindowPos` (the rectangle it grew for its client area) moves and sizes
    /// the real window, the flags keep what they name, a child window sizes its parent's client area
    /// and never moves it, and the swap chain's size (`client_size`) is the one the server has.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn the_games_set_window_pos_moves_and_sizes_the_x11_window() {
        let Some(_s) = window_session() else { return };
        let probe = fork_window::probe::Probe::connect();
        let (mut e, window, x) = engine_with_window(
            fork_window::WindowRequest {
                width: 640,
                height: 480,
                decorations: false,
                ..Default::default()
            },
            0x11c,
        );
        let geometry = |probe: &fork_window::probe::Probe| {
            let g = probe.geometry(x);
            (g.x, g.y, g.width, g.height)
        };
        // No frame is recorded for the window (`game_frame` has none): the rectangle is the client.
        window_moved(&mut e, 0x11c, [100, 50, 900, 650], 0);
        becomes("moved and sized", (100, 50, 800, 600), || geometry(&probe));
        becomes("the swap chain", (800, 600), || {
            fork_window::client_size(window.id())
        });

        window_moved(&mut e, 0x11c, [0, 0, 512, 384], SWP_NOMOVE);
        becomes("size only (SWP_NOMOVE)", (100, 50, 512, 384), || {
            geometry(&probe)
        });
        becomes("the swap chain", (512, 384), || {
            fork_window::client_size(window.id())
        });

        window_moved(&mut e, 0x11c, [20, 10, 9999, 9999], SWP_NOSIZE);
        becomes("place only (SWP_NOSIZE)", (20, 10, 512, 384), || {
            geometry(&probe)
        });

        // A child window: its rectangle is in the parent's client area. Its size is the parent's
        // client area's, its place is not the window's.
        real(&mut e).windows.insert(0x120, window.id());
        window_moved(&mut e, 0x120, [300, 200, 700, 500], 0);
        becomes("a child's size", (20, 10, 400, 300), || geometry(&probe));

        // The frame the game added (AdjustWindowRect) is taken off: the client area is what it wanted.
        assert_eq!(client_for([0, 0, 656, 519], [-8, -31, 8, 8]), (640, 480));
        assert_eq!(client_for([0, 0, 0, 0], [0; 4]), (1, 1));
    }

    /// #218: `Reset` of the device to another size while the window is the full-screen one (the
    /// game's display mode change, which is a windowed reset here): the window stays refitted to the
    /// monitor, the back buffer takes the requested size, a frame is presented, and nothing changes
    /// the monitor's own mode (RandR's, read from the server).
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_mode_change_through_reset_keeps_the_borderless_window_on_the_monitor() {
        let Some(_s) = window_session() else { return };
        let _wm = fork_window::probe::FakeWm::start();
        let probe = fork_window::probe::Probe::connect();
        let monitor = probe.crtc();
        let window = fork_window::Window::open(
            fork_window::WindowRequest {
                width: 64,
                height: 64,
                decorations: false,
                fullscreen: true,
                ..Default::default()
            },
            None,
        )
        .expect("a full-screen window");
        let id = window.id();
        let x = fork_window::probe::x_window(id).expect("an X11 window");
        let covers = |probe: &fork_window::probe::Probe| {
            let g = probe.geometry(x);
            (g.x, g.y, g.width, g.height)
        };
        let whole = (monitor.x, monitor.y, monitor.width, monitor.height);
        becomes("the window over the monitor", whole, || covers(&probe));

        let backend = std::sync::Arc::new(std::sync::Mutex::new(d3d9_wgpu::WgpuBackend::new(
            d3d9_wgpu::Output::Window,
        )));
        let mut e = Engine::new();
        d3d9::load_adapter_table(&mut e, &table());
        let create = d3d9::proc_address(&mut e, b"Direct3DCreate9").unwrap();
        let d3d = e.call(create, &[d3d9::D3D_SDK_VERSION]).u32();
        real(&mut e).stream = Some(Stream::direct(Box::new(Shared(backend.clone()))));
        real(&mut e).windows.insert(0x11c, id);
        let present = e.mem.alloc(0x38);
        e.mem.set_u32(present, 64);
        e.mem.set_u32(present + 4, 64);
        e.mem.set_u32(present + 0x1c, 0x11c);
        e.mem.set_u32(present + 0x20, 1);
        let out = e.mem.alloc(4);
        assert_eq!(
            e.vcall(d3d, 4 * 16, &[0, 1, 0x11c, 0x40, present, out])
                .u32(),
            0,
            "CreateDevice"
        );
        let device = e.mem.u32(out);
        if backend.lock().unwrap().device().is_none() {
            assert!(
                std::env::var_os("NV_REQUIRE_GPU").is_none(),
                "NV_REQUIRE_GPU is set and wgpu has no adapter"
            );
            eprintln!("no GPU adapter: the reset test is skipped");
            return;
        }
        assert_eq!(e.vcall(device, 4 * 17, &[0, 0, 0, 0]).u32(), 0, "Present");

        // The game asks for 128x96 (another mode): a windowed reset at that size.
        e.mem.set_u32(present, 128);
        e.mem.set_u32(present + 4, 96);
        assert_eq!(e.vcall(device, 4 * 16, &[present]).u32(), 0, "Reset");
        assert_eq!(
            e.vcall(device, 4 * 18, &[0, 0, 0, out]).u32(),
            0,
            "GetBackBuffer"
        );
        let back = e.mem.u32(out);
        e.vcall(device, 4 * 41, &[]);
        e.vcall(device, 4 * 43, &[0, 0, 1, 0xFF00_FF00, 1.0f32.to_bits(), 0]);
        e.vcall(device, 4 * 42, &[]);
        assert_eq!(
            e.vcall(device, 4 * 17, &[0, 0, 0, 0]).u32(),
            0,
            "Present after Reset"
        );
        real(&mut e).stream.as_mut().expect("the stream").finish();
        let (w, h, _) = backend
            .lock()
            .unwrap()
            .device()
            .and_then(|d| d.read_surface_pixels(back))
            .expect("the back buffer's pixels");
        assert_eq!(
            (w, h),
            (128, 96),
            "the back buffer takes the requested size"
        );

        // The window is still the monitor's, and the swap chain follows it, not the back buffer.
        assert_eq!(
            covers(&probe),
            whole,
            "the window is refitted to the monitor"
        );
        assert_eq!(
            fork_window::client_size(id),
            (monitor.width, monitor.height)
        );
        assert_eq!(probe.crtc(), monitor, "the monitor's own mode is unchanged");
        assert_eq!(probe.screen_size(), (monitor.width, monitor.height));
    }

    fn guest_frame(window: Option<isize>) {
        let output = if window.is_some() {
            d3d9_wgpu::Output::Window
        } else {
            d3d9_wgpu::Output::Offscreen
        };
        let backend =
            std::sync::Arc::new(std::sync::Mutex::new(d3d9_wgpu::WgpuBackend::new(output)));
        let mut e = Engine::new();
        d3d9::load_adapter_table(&mut e, &table());
        let create = d3d9::proc_address(&mut e, b"Direct3DCreate9").unwrap();
        let d3d = e.call(create, &[d3d9::D3D_SDK_VERSION]).u32();
        // What `NV_D3D9=wgpu` makes at the first `Direct3DCreate9`, with the test's backend.
        real(&mut e).stream = Some(Stream::direct(Box::new(Shared(backend.clone()))));
        let present = e.mem.alloc(0x38);
        if let Some(id) = window {
            // The modelled window 0x11c is the real window `id` (what `window_created` records).
            real(&mut e).windows.insert(0x11c, id);
        }
        e.mem.set_u32(present, 64);
        e.mem.set_u32(present + 4, 64);
        e.mem.set_u32(present + 0x1c, 0x11c);
        e.mem.set_u32(present + 0x20, 1);
        let out = e.mem.alloc(4);
        let hr = e
            .vcall(d3d, 4 * 16, &[0, 1, 0x11c, 0x40, present, out])
            .u32();
        assert_eq!(hr, 0, "CreateDevice");
        let device = e.mem.u32(out);
        if backend.lock().unwrap().device().is_none() {
            assert!(
                std::env::var_os("NV_REQUIRE_GPU").is_none(),
                "NV_REQUIRE_GPU is set and wgpu has no adapter"
            );
            eprintln!("no GPU adapter: the headless frame test is skipped");
            return;
        }
        assert!(real(&mut e).devices.iter().any(|(d, _)| *d == device));
        let words = |e: &mut Engine, w: &[u32]| {
            let at = e.mem.alloc(4 * w.len() as u32);
            for (i, x) in w.iter().enumerate() {
                e.mem.set_u32(at + 4 * i as u32, *x);
            }
            at
        };
        // vs_2_0: dcl_position v0; mov oPos, v0. ps_3_0: mov oC0, c0.
        let vs = words(
            &mut e,
            &[
                0xFFFE_0200,
                0x0200_001F,
                0x8000_0000,
                0x900F_0000,
                0x0200_0001,
                0xC00F_0000,
                0x90E4_0000,
                0x0000_FFFF,
            ],
        );
        let ps = words(
            &mut e,
            &[
                0xFFFF_0300,
                0x0200_0001,
                0x800F_0800,
                0xA0E4_0000,
                0x0000_FFFF,
            ],
        );
        let elements = e.mem.alloc(16);
        // `D3DVERTEXELEMENT9`: stream 0, offset 0, FLOAT4 (3), method 0, usage POSITION (0), index 0.
        e.mem.set_u16(elements, 0);
        e.mem.set_u16(elements + 2, 0);
        e.mem.set_u8(elements + 4, 3);
        e.mem.set_u8(elements + 5, 0);
        e.mem.set_u8(elements + 6, 0);
        e.mem.set_u8(elements + 7, 0);
        e.mem.set_u16(elements + 8, 0xFF);
        e.mem.set_u16(elements + 10, 0);
        e.mem.set_u8(elements + 12, 17);
        e.mem.set_u8(elements + 13, 0);
        e.mem.set_u8(elements + 14, 0);
        e.mem.set_u8(elements + 15, 0);
        assert_eq!(
            e.vcall(device, 4 * 91, &[vs, out]).u32(),
            0,
            "CreateVertexShader"
        );
        let vs = e.mem.u32(out);
        assert_eq!(
            e.vcall(device, 4 * 106, &[ps, out]).u32(),
            0,
            "CreatePixelShader"
        );
        let ps = e.mem.u32(out);
        assert_eq!(
            e.vcall(device, 4 * 86, &[elements, out]).u32(),
            0,
            "CreateVertexDeclaration"
        );
        let decl = e.mem.u32(out);
        assert_eq!(
            e.vcall(device, 4 * 18, &[0, 0, 0, out]).u32(),
            0,
            "GetBackBuffer"
        );
        let back = e.mem.u32(out);
        let f = |x: f32| x.to_bits();
        let triangle = words(
            &mut e,
            &[
                f(-0.8),
                f(-0.8),
                f(0.0),
                f(1.0),
                f(0.0),
                f(0.8),
                f(0.0),
                f(1.0),
                f(0.8),
                f(-0.8),
                f(0.0),
                f(1.0),
            ],
        );
        let colour = words(&mut e, &[f(1.0), f(0.0), f(0.0), f(1.0)]);
        e.vcall(device, 4 * 41, &[]);
        // D3DCLEAR_TARGET, opaque green; the triangle clockwise (Direct3D culls counter-clockwise).
        e.vcall(device, 4 * 43, &[0, 0, 1, 0xFF00_FF00, f(1.0), 0]);
        e.vcall(device, 4 * 87, &[decl]);
        e.vcall(device, 4 * 92, &[vs]);
        e.vcall(device, 4 * 107, &[ps]);
        e.vcall(device, 4 * 109, &[0, colour, 1]);
        // `DrawPrimitiveUP`: D3DPT_TRIANGLELIST, one primitive, 16 bytes a vertex.
        e.vcall(device, 4 * 83, &[4, 1, triangle, 16]);
        e.vcall(device, 4 * 42, &[]);
        assert_eq!(e.vcall(device, 4 * 17, &[0, 0, 0, 0]).u32(), 0, "Present");
        real(&mut e).stream.as_mut().expect("the stream").finish();
        let mut b = backend.lock().unwrap();
        let (w, h, px) = b
            .device()
            .and_then(|d| d.read_surface_pixels(back))
            .expect("the back buffer's pixels");
        assert_eq!((w, h), (64, 64));
        let at = |x: usize, y: usize| {
            let i = 4 * (y * 64 + x);
            [px[i], px[i + 1], px[i + 2]]
        };
        // B, G, R: green where nothing was drawn, red inside the triangle.
        assert_eq!(at(2, 2), [0x00, 0xFF, 0x00], "the cleared corner");
        assert_eq!(at(32, 40), [0x00, 0x00, 0xFF], "the triangle's centre");
    }
}
