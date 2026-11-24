//! X11 and Wayland: a winit 0.30 event loop on a thread of its own. macOS: the same loop on the
//! program's main thread ([`run_on_main_thread`]: AppKit runs its windows there only), each window
//! with a `CAMetalLayer` made there for the device layer's surface ([`crate::metal_layer`]).

#[cfg(not(target_os = "macos"))]
mod idle;

/// macOS: the screen is not kept awake yet (the system's display sleep applies).
#[cfg(target_os = "macos")]
mod idle {
    use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

    pub(crate) struct Inhibit;

    impl Inhibit {
        pub(crate) fn name(&self) -> &'static str {
            "none"
        }

        pub(crate) fn start(_: RawDisplayHandle, _: RawWindowHandle) -> Inhibit {
            Inhibit
        }
    }
}

use crate::registry::{self, Handles, Shared};
use crate::{dik, ChangeHook, WindowRequest};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::OnceLock;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{
    DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, PhysicalKey};
#[cfg(not(target_os = "macos"))]
use winit::platform::wayland::EventLoopBuilderExtWayland;
#[cfg(not(target_os = "macos"))]
use winit::platform::x11::EventLoopBuilderExtX11;
use winit::window::{CursorGrabMode, WindowAttributes, WindowId};

/// Whether `event` says the window is back or changed, so the present loop is woken
/// (`changed`).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn wakes(event: &WindowEvent) -> bool {
    matches!(
        event,
        WindowEvent::Focused(true)
            | WindowEvent::Occluded(false)
            | WindowEvent::RedrawRequested
            | WindowEvent::Resized(_)
            | WindowEvent::ScaleFactorChanged { .. }
    )
}

/// The DirectInput code of a physical key.
pub(crate) fn dik_of(code: KeyCode) -> Option<u8> {
    dik::by_name(&format!("{code:?}"))
}

/// The DirectInput button index of a mouse button.
pub(crate) fn button_of(button: MouseButton) -> Option<usize> {
    match button {
        MouseButton::Left => Some(0),
        MouseButton::Right => Some(1),
        MouseButton::Middle => Some(2),
        MouseButton::Back => Some(3),
        MouseButton::Forward => Some(4),
        MouseButton::Other(_) => None,
    }
}

/// How long the game has to take a close request before a second one ends the process.
const CLOSE_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// Whether a close request at `now` is a repeat of one the game left alone for [`CLOSE_GRACE`].
fn close_ignored(first: Option<std::time::Instant>, now: std::time::Instant) -> bool {
    first.is_some_and(|f| now.duration_since(f) >= CLOSE_GRACE)
}

/// `wayland` or `x11`, by the window's raw handle.
fn session_name(w: &winit::window::Window) -> &'static str {
    use raw_window_handle::RawWindowHandle;
    match w.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Wayland(_)) => "wayland",
        Ok(RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_)) => "x11",
        #[cfg(target_os = "macos")]
        Ok(RawWindowHandle::AppKit(_)) => "macos",
        _ => "unknown",
    }
}

/// What the event loop thread answers [`Window::open`] with: the window's id, or why not.
type Ready = Result<isize, String>;

enum Command {
    /// Make the window (the loop's one at a time).
    Open {
        request: WindowRequest,
        on_change: Option<ChangeHook>,
        ready: mpsc::Sender<Ready>,
    },
    /// Answer the display the loop's monitors give ([`crate::display`]).
    Probe(mpsc::Sender<Option<crate::Display>>),
    /// End the window; answered when it is gone.
    Close(mpsc::Sender<()>),
    /// A new title for window `id` (the game's `SetWindowTextA`); answered when it is asked of the
    /// window system, with whether the window was there.
    Title(isize, String, mpsc::Sender<bool>),
    /// Window `id` moved to `position` (its outer corner) and/or sized to `size` (its client
    /// area) (the game's `SetWindowPos`); answered when asked of the window system, with whether
    /// anything was asked: the full-screen window keeps its monitor and is left as it is.
    Place(
        isize,
        Option<(i32, i32)>,
        Option<(u32, u32)>,
        mpsc::Sender<bool>,
    ),
    /// Window `id` minimised or brought back; answered when asked of the window system.
    Minimise(isize, bool, mpsc::Sender<bool>),
    /// Whether window `id` is minimised, as the window system says.
    IsMinimised(isize, mpsc::Sender<bool>),
}

/// Sends `command` made with a reply channel to the loop and waits for the answer (`false` when
/// there is no loop, or it does not answer).
fn ask(make: impl FnOnce(mpsc::Sender<bool>) -> Command) -> bool {
    let Ok(hub) = hub() else { return false };
    let (tx, rx) = mpsc::channel();
    if hub.send_event(make(tx)).is_err() {
        return false;
    }
    rx.recv_timeout(std::time::Duration::from_secs(10))
        .unwrap_or(false)
}

/// Gives window `id` the title `title` (the game's `SetWindowTextA` after the window is made).
/// Whether the window is there.
pub fn set_title(id: isize, title: &str) -> bool {
    ask(|tx| Command::Title(id, title.to_owned(), tx))
}

/// Moves window `id` to `position` (its outer corner, in pixels) and/or sizes its client area to
/// `size` (the game's `SetWindowPos`). The window of a full-screen request keeps the monitor
/// (nothing is asked of it), and Wayland places no window for its client. Whether a change was
/// asked of the window system.
pub fn place(id: isize, position: Option<(i32, i32)>, size: Option<(u32, u32)>) -> bool {
    ask(|tx| Command::Place(id, position, size, tx))
}

/// Minimises window `id`, or brings it back (the compositor or window manager acts on the
/// request). Whether the window is there.
pub fn set_minimised(id: isize, minimised: bool) -> bool {
    ask(|tx| Command::Minimise(id, minimised, tx))
}

/// Whether the window system says window `id` is minimised.
pub fn is_minimised(id: isize) -> bool {
    ask(|tx| Command::IsMinimised(id, tx))
}

/// The one event loop of the process. winit allows one per process (a second is refused with
/// "EventLoop can't be recreated"), so the monitor query and every window go through it: it is
/// made on the first use, on a thread of its own that lives to the end of the process.
#[cfg(not(target_os = "macos"))]
fn hub() -> Result<&'static EventLoopProxy<Command>, String> {
    static HUB: OnceLock<Result<EventLoopProxy<Command>, String>> = OnceLock::new();
    HUB.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("nv-window".into())
            .spawn(move || run(tx))
            .map_err(|e| format!("window thread: {e}"))?;
        rx.recv()
            .map_err(|_| "the window thread ended at start".to_string())?
    })
    .as_ref()
    .map_err(Clone::clone)
}

/// macOS: the loop [`run_on_main_thread`] runs on the main thread, once it has started.
#[cfg(target_os = "macos")]
static MAIN_HUB: OnceLock<Result<EventLoopProxy<Command>, String>> = OnceLock::new();

/// macOS: the loop on the main thread, waited for while [`run_on_main_thread`] starts it; an error
/// when the program did not hand the main thread over (the run then stays off screen).
#[cfg(target_os = "macos")]
fn hub() -> Result<&'static EventLoopProxy<Command>, String> {
    if !HANDED_OVER.load(std::sync::atomic::Ordering::Acquire) {
        return Err("on macOS the window layer needs the program's main thread, which this program did not hand to it".into());
    }
    let started = std::time::Instant::now();
    loop {
        if let Some(hub) = MAIN_HUB.get() {
            return hub.as_ref().map_err(Clone::clone);
        }
        if started.elapsed() > std::time::Duration::from_secs(10) {
            return Err("the main thread's event loop did not start in 10 s".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// macOS: whether [`run_on_main_thread`] was called.
#[cfg(target_os = "macos")]
static HANDED_OVER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The stack of the thread the game runs on when the main thread is the window loop's: the main
/// thread's own size on macOS (8 MB), not a spawned thread's default (2 MB in Rust).
#[cfg(target_os = "macos")]
const GAME_STACK: usize = 8 << 20;

/// macOS: runs `game` on a thread of its own and the window loop on the calling thread, which must
/// be the program's main thread (AppKit's windows and events are the main thread's only). When
/// `game` returns, `end` is called with 0 (101 when it panicked) on the game's thread; `end` ends
/// the process. If the loop ends first (it cannot be made), `end(1)` is called here.
#[cfg(target_os = "macos")]
pub fn run_on_main_thread(game: Box<dyn FnOnce() + Send>, end: fn(i32) -> !) -> ! {
    HANDED_OVER.store(true, std::sync::atomic::Ordering::Release);
    let spawned = std::thread::Builder::new()
        .name("nv-game".into())
        .stack_size(GAME_STACK)
        .spawn(move || {
            let ended = std::panic::catch_unwind(std::panic::AssertUnwindSafe(game));
            end(if ended.is_ok() { 0 } else { 101 })
        });
    if let Err(e) = spawned {
        eprintln!("fork-window: the game's thread could not start ({e})");
        end(1);
    }
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        if let Ok(hub) = rx.recv() {
            let _ = MAIN_HUB.set(hub);
        }
    });
    run(tx);
    end(1)
}

/// A window of the process's event loop. Drop (or [`Window::shutdown`]) ends the window: drop
/// every wgpu surface made from it first.
pub struct Window {
    id: isize,
    proxy: Option<EventLoopProxy<Command>>,
}

impl Window {
    /// Makes the window and returns when it exists (or why it could not be made: no display, no
    /// compositor). Wayland is used when the session is one, X11 otherwise (winit's choice).
    pub fn open(request: WindowRequest, on_change: Option<ChangeHook>) -> Result<Window, String> {
        let proxy = hub()?;
        let (tx, rx) = mpsc::channel::<Ready>();
        proxy
            .send_event(Command::Open {
                request,
                on_change,
                ready: tx,
            })
            .map_err(|_| "the event loop has ended".to_string())?;
        match rx.recv() {
            Ok(Ok(id)) => Ok(Window {
                id,
                proxy: Some(proxy.clone()),
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("the window thread ended without a window".into()),
        }
    }

    /// The id the device layer names this window by (never 0).
    pub fn id(&self) -> isize {
        self.id
    }

    /// Ends the window (what drop does); the event loop stays, as winit makes only one.
    pub fn shutdown(&mut self) {
        if let Some(proxy) = self.proxy.take() {
            let (tx, rx) = mpsc::channel();
            if proxy.send_event(Command::Close(tx)).is_ok() {
                let _ = rx.recv();
            }
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// The monitors of this session, asked of the process's event loop; the pick is [`crate::pick`]
/// with `NV_MONITOR` and the primary monitor.
pub(crate) fn probe_monitors() -> Option<crate::Display> {
    let (tx, rx) = mpsc::channel();
    hub().ok()?.send_event(Command::Probe(tx)).ok()?;
    rx.recv_timeout(std::time::Duration::from_secs(10))
        .ok()
        .flatten()
}

fn monitor_of(m: &winit::monitor::MonitorHandle) -> crate::Monitor {
    let size = m.size();
    crate::Monitor {
        width: size.width,
        height: size.height,
        refresh_mhz: m.refresh_rate_millihertz().unwrap_or(0),
    }
}

fn pick_on(event_loop: &ActiveEventLoop) -> Option<crate::Display> {
    let handles: Vec<_> = event_loop.available_monitors().collect();
    let monitors: Vec<_> = handles.iter().map(monitor_of).collect();
    let names: Vec<_> = handles
        .iter()
        .map(|h| h.name().unwrap_or_default())
        .collect();
    let primary = event_loop
        .primary_monitor()
        .and_then(|p| handles.iter().position(|h| *h == p));
    let wanted = crate::parse_wanted(std::env::var("NV_MONITOR").ok().as_deref());
    crate::pick(
        &monitors,
        &names,
        wanted.as_ref(),
        crate::last_monitor().as_deref(),
        primary,
    )
}

/// Whether the session is Wayland, where a client cannot place its window: the compositor does.
/// The window's application id (Wayland `app_id`, X11 `WM_CLASS` class and instance): the run's
/// label, else `nv-rs` ([`crate::app_id`]); the desktop's dock names and groups windows by it.
#[cfg(not(target_os = "macos"))]
fn named(attrs: WindowAttributes, id: &str) -> WindowAttributes {
    use winit::platform::wayland::WindowAttributesExtWayland;
    use winit::platform::x11::WindowAttributesExtX11;
    let attrs = WindowAttributesExtWayland::with_name(attrs, id, id);
    WindowAttributesExtX11::with_name(attrs, id, id)
}

/// macOS has no application id of a window.
#[cfg(target_os = "macos")]
fn named(attrs: WindowAttributes, _id: &str) -> WindowAttributes {
    attrs
}

fn wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var("WINIT_UNIX_BACKEND").map_or(true, |v| v != "x11")
}

fn run(ready: mpsc::Sender<Result<EventLoopProxy<Command>, String>>) {
    // winit takes Wayland when the session is one, else X11 (`WINIT_UNIX_BACKEND=x11` forces X11;
    // a second event loop cannot be made after a failed one, so there is no retry here).
    let mut builder = EventLoop::<Command>::with_user_event();
    // winit refuses an event loop off the main thread unless asked, per backend (macOS: never
    // allowed, so the loop is the main thread's: `run_on_main_thread`).
    #[cfg(not(target_os = "macos"))]
    EventLoopBuilderExtX11::with_any_thread(&mut builder, true);
    #[cfg(not(target_os = "macos"))]
    EventLoopBuilderExtWayland::with_any_thread(&mut builder, true);
    let event_loop = match builder.build() {
        Ok(l) => l,
        Err(e) => {
            let _ = ready.send(Err(format!("event loop: {e}")));
            return;
        }
    };
    event_loop.set_control_flow(ControlFlow::Wait);
    let _ = ready.send(Ok(event_loop.create_proxy()));
    let mut app = App {
        request: WindowRequest::default(),
        on_change: None,
        shared: Shared::new(),
        id: 0,
        window: None,
        device_motion: false,
        cursor: None,
        rest: [0.0; 2],
        landed_on: None,
        idle: None,
        #[cfg(target_os = "macos")]
        metal: None,
        held: "none",
        reported: None,
        close_first: None,
        active: false,
        deactivated: false,
    };
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("fork-window: the event loop ended: {e}");
    }
    if app.id != 0 {
        registry::unregister(app.id);
    }
}

struct App {
    request: WindowRequest,
    on_change: Option<ChangeHook>,
    shared: Arc<Shared>,
    id: isize,
    /// Kept so the window outlives the surfaces made on it until the loop ends.
    window: Option<winit::window::Window>,
    /// Whether the compositor sends relative motion; if it never does, the pointer's own moves
    /// stand in for it.
    device_motion: bool,
    cursor: Option<(f64, f64)>,
    /// The fractions of motion not yet whole game pixels, per axis.
    rest: [f64; 2],
    /// The name of the output the window was last said to be on ([`App::landed`]).
    landed_on: Option<String>,
    /// Keeps the screen awake while the window is up.
    idle: Option<idle::Inhibit>,
    /// macOS: the window's `CAMetalLayer`, kept retained while the window is registered.
    #[cfg(target_os = "macos")]
    metal: Option<raw_window_metal::Layer>,
    /// How the pointer is held (for the report).
    held: &'static str,
    /// What the last report said (size in pixels, scale), so a line is made only on a change.
    reported: Option<(u32, u32, u64)>,
    /// When the first close request of the window came ([`close_ignored`]).
    close_first: Option<std::time::Instant>,
    /// Whether the window has the focus.
    active: bool,
    /// Whether the window lost a focus it had since it was made: from then on each time it gets the
    /// focus back the game is told it was activated again (`Input::reactivated`). X11 says the
    /// window is not focused before it says it is, which is not a loss.
    deactivated: bool,
}

impl App {
    /// Makes the window for `request` (the loop's one) and answers `ready`.
    fn open(&mut self, event_loop: &ActiveEventLoop, ready: mpsc::Sender<Ready>) {
        let _ = ready.send(self.create(event_loop));
    }

    fn create(&mut self, event_loop: &ActiveEventLoop) -> Ready {
        if self.window.is_some() {
            return Err("a window is already open (one at a time)".into());
        }
        self.shared = Shared::new();
        self.device_motion = false;
        self.cursor = None;
        self.rest = [0.0; 2];
        self.reported = None;
        self.close_first = None;
        self.active = false;
        self.deactivated = false;
        let label = crate::run_label();
        let attrs = WindowAttributes::default()
            .with_title(crate::titled(label.as_deref(), &self.request.title))
            .with_inner_size(PhysicalSize::new(self.request.width, self.request.height))
            .with_decorations(self.request.decorations)
            // Resizable: maximised, tiled or dragged, the swap chain follows the client area
            // (`Resized`) and the back buffer is drawn over it (nv-rs#835).
            .with_resizable(true)
            .with_visible(self.request.visible)
            .with_active(self.request.activate)
            .with_fullscreen(self.request.fullscreen.then(|| {
                // `NV_MONITOR` names the output. Otherwise on Wayland none is asked for: the
                // compositor puts the window on the current output (GNOME: where the pointer or
                // the focus is), and the monitor it landed on is read after (`landed`). On X11
                // the pick (last used, primary, first).
                let named = std::env::var_os("NV_MONITOR").is_some();
                let chosen = if wayland_session() && !named {
                    None
                } else {
                    pick_on(event_loop).and_then(|d| event_loop.available_monitors().nth(d.index))
                };
                winit::window::Fullscreen::Borderless(chosen)
            }));
        let attrs = named(attrs, &crate::app_id(label.as_deref()));
        let window = event_loop
            .create_window(attrs)
            .map_err(|e| format!("create window: {e}"))?;
        let (display, handle) = match (window.display_handle(), window.window_handle()) {
            (Ok(d), Ok(w)) => (d.as_raw(), w.as_raw()),
            _ => return Err("the window has no raw handles".into()),
        };
        let size = window.inner_size();
        self.shared.set_size(size.width, size.height);
        if let Ok(mut h) = self.shared.handles.lock() {
            *h = Some(Handles(display, handle));
        }
        #[cfg(target_os = "macos")]
        {
            self.metal = metal_layer_of(handle);
            let address = self
                .metal
                .as_ref()
                .map_or(0, |l| l.as_ptr().as_ptr() as usize);
            self.shared
                .metal
                .store(address, std::sync::atomic::Ordering::Release);
        }
        self.id = registry::register(self.shared.clone());
        self.idle = Some(idle::Inhibit::start(display, handle));
        self.window = Some(window);
        self.held = self.hold_pointer();
        self.landed();
        self.report();
        Ok(self.id)
    }

    /// Motion of the pointer in window pixels as the game's: scaled to its back buffer.
    fn add_motion(&mut self, dx: f64, dy: f64) {
        let (gw, gh) = registry::game_area(&self.shared);
        let (ww, wh) = self
            .window
            .as_ref()
            .map_or((0, 0), |w| (w.inner_size().width, w.inner_size().height));
        let x = registry::scale_motion(dx, gw, ww, &mut self.rest[0]);
        let y = registry::scale_motion(dy, gh, wh, &mut self.rest[1]);
        if let Ok(mut i) = self.shared.input.lock() {
            i.motion[0] += x;
            i.motion[1] += y;
        }
    }

    /// The game's cursor is drawn by the game (its DirectInput mouse is exclusive, so Windows
    /// holds and hides the pointer): hide the pointer and keep it in the window, locked where the
    /// compositor allows (Wayland), else confined (X11), else as it is.
    ///
    /// Returns how it is held: `locked`, `confined` or `none`.
    fn hold_pointer(&self) -> &'static str {
        let Some(w) = &self.window else {
            return "none";
        };
        w.set_cursor_visible(false);
        if w.set_cursor_grab(CursorGrabMode::Locked).is_ok() {
            "locked"
        } else if w.set_cursor_grab(CursorGrabMode::Confined).is_ok() {
            "confined"
        } else {
            eprintln!("fork-window: the pointer cannot be held in the window");
            "none"
        }
    }

    /// One line on how the window came up: the session, its size in pixels and the scale it is
    /// drawn at, how the pointer is held and how the screen is kept awake (what to read when a
    /// desktop misbehaves; `docs/fork/DESKTOPS.md`).
    fn report(&mut self) {
        let Some(w) = &self.window else { return };
        let size = w.inner_size();
        let key = (size.width, size.height, w.scale_factor().to_bits());
        if self.reported == Some(key) {
            return;
        }
        self.reported = Some(key);
        let held = self.held;
        let idle = self.idle.as_ref().map_or("none", idle::Inhibit::name);
        fork_log::info!(
            target: "window",
            "fork-window: window {}x{} px at scale {} on {}, pointer {held}, idle inhibit {idle}",
            size.width,
            size.height,
            w.scale_factor(),
            session_name(w),
        );
    }

    fn release_pointer(&self) {
        if let Some(w) = &self.window {
            let _ = w.set_cursor_grab(CursorGrabMode::None);
            w.set_cursor_visible(true);
        }
    }

    /// What a window coming back needs: not minimised, and full screen again where it was asked
    /// for and the compositor dropped it while the window was away; a redraw is requested.
    fn restore(&self) {
        let Some(w) = &self.window else { return };
        if w.is_minimized() == Some(true) {
            w.set_minimized(false);
        }
        if self.request.fullscreen && w.fullscreen().is_none() {
            let monitor = w.current_monitor();
            w.set_fullscreen(Some(winit::window::Fullscreen::Borderless(monitor)));
        }
        w.request_redraw();
    }

    /// The monitor the window is on now: one line when it differs from what was last said, and
    /// its name kept for the next run's pick (the seeded size is chosen before the window exists).
    fn landed(&mut self) {
        let Some(m) = self.window.as_ref().and_then(|w| w.current_monitor()) else {
            return;
        };
        let name = m.name().unwrap_or_default();
        if name.is_empty() || self.landed_on.as_deref() == Some(name.as_str()) {
            return;
        }
        let mon = monitor_of(&m);
        fork_log::info!(
            target: "window",
            "fork-window: the window is on monitor {name} {}x{} {} Hz (placed by the {})",
            mon.width,
            mon.height,
            crate::hz_of(mon.refresh_mhz),
            if wayland_session() {
                "compositor"
            } else {
                "pick"
            }
        );
        crate::remember_monitor(&name);
        self.landed_on = Some(name);
    }

    /// Moves and sizes window `id` as the game's `SetWindowPos` asks, no larger than its monitor.
    /// The full-screen window keeps the monitor. The player may resize the window after (maximise,
    /// tile): the swap chain follows the client area either way.
    fn place(&mut self, id: isize, position: Option<(i32, i32)>, size: Option<(u32, u32)>) -> bool {
        let Some(w) = self.window.as_ref().filter(|_| self.id == id) else {
            return false;
        };
        if self.request.fullscreen || (position.is_none() && size.is_none()) {
            return false;
        }
        if let Some((x, y)) = position {
            w.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
        }
        if let Some(wanted) = size {
            let monitor = w
                .current_monitor()
                .map(|m| (m.size().width, m.size().height));
            let (width, height) = fit_monitor(wanted, monitor);
            let answer = w.request_inner_size(PhysicalSize::new(width, height));
            // Wayland applies the size at once and answers it here, with no `Resized` event to
            // follow (X11 and macOS answer `None` and send one): the size the swap chain reads
            // must take the answer, or it stays at the window's first size and the whole picture
            // is drawn that small and stretched over the window (nv-rs#835).
            if took_answer(&self.shared, answer.map(|s| (s.width, s.height))) {
                self.report();
                self.changed();
            }
        }
        true
    }

    fn changed(&self) {
        if let Some(f) = self.on_change {
            f();
        }
    }
}

impl ApplicationHandler<Command> for App {
    fn resumed(&mut self, _: &ActiveEventLoop) {}

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Command) {
        match event {
            Command::Open {
                request,
                on_change,
                ready,
            } => {
                self.request = request;
                self.on_change = on_change;
                self.open(event_loop, ready);
            }
            Command::Probe(answer) => {
                let _ = answer.send(pick_on(event_loop));
            }
            Command::Close(done) => {
                if self.id != 0 {
                    registry::unregister(self.id);
                    self.id = 0;
                }
                self.idle = None;
                #[cfg(target_os = "macos")]
                {
                    self.metal = None;
                }
                self.window = None;
                let _ = done.send(());
            }
            Command::Title(id, title, done) => {
                let there = self.id == id && self.window.is_some();
                if let (true, Some(w)) = (there, &self.window) {
                    w.set_title(&crate::titled(crate::run_label().as_deref(), &title));
                }
                let _ = done.send(there);
            }
            Command::Place(id, position, size, done) => {
                let _ = done.send(self.place(id, position, size));
            }
            Command::Minimise(id, minimised, done) => {
                let there = self.id == id && self.window.is_some();
                if let (true, Some(w)) = (there, &self.window) {
                    w.set_minimized(minimised);
                }
                let _ = done.send(there);
            }
            Command::IsMinimised(id, done) => {
                let down = self.id == id
                    && self
                        .window
                        .as_ref()
                        .is_some_and(|w| w.is_minimized() == Some(true));
                let _ = done.send(down);
            }
        }
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                let now = std::time::Instant::now();
                if close_ignored(self.close_first, now) {
                    // The user closed the window again and the game never took the first
                    // request (stalled in a frame the compositor does not call back, a hung
                    // driver): the window must still be closable.
                    eprintln!("fork-window: the game did not answer a close request in {CLOSE_GRACE:?}; ending the process");
                    std::process::exit(1);
                }
                self.close_first.get_or_insert(now);
                self.shared
                    .close
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
            WindowEvent::Resized(s) => {
                self.shared.set_size(s.width, s.height);
                self.landed();
                self.report();
                self.changed();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                self.report();
                self.changed();
            }
            WindowEvent::Focused(on) => {
                if !on {
                    // Losing the focus lets go of everything held (as the Windows layer does).
                    if let Ok(mut i) = self.shared.input.lock() {
                        i.release_all();
                    }
                    self.cursor = None;
                    self.release_pointer();
                    self.deactivated |= self.active;
                    self.active = false;
                } else {
                    self.active = true;
                    // A focus that follows a loss of it is the game's activation (`WA_ACTIVE`):
                    // its window procedure makes the mouse exclusive again. The first focus,
                    // when the window is made, is not passed on (the game ignores it).
                    if self.deactivated {
                        if let Ok(mut i) = self.shared.input.lock() {
                            i.reactivated = Some(1);
                        }
                    }
                    self.held = self.hold_pointer();
                    self.restore();
                    self.changed();
                }
            }
            // Shown again (alt-tab back, the compositor mapping the window): the swap chain
            // is asked for a frame at once, and a redraw is requested of the compositor.
            WindowEvent::Occluded(false) | WindowEvent::RedrawRequested => {
                if matches!(event, WindowEvent::Occluded(false)) {
                    self.restore();
                }
                self.changed();
            }
            WindowEvent::KeyboardInput { event, .. } if !event.repeat => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if let (Some(k), Ok(mut i)) = (dik_of(code), self.shared.input.lock()) {
                        i.set_key(k, event.state == ElementState::Pressed);
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let (Some(b), Ok(mut i)) = (button_of(button), self.shared.input.lock()) {
                    i.set_button(b, state == ElementState::Pressed);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let notches = match delta {
                    MouseScrollDelta::LineDelta(_, y) => (y * 120.0) as i32,
                    MouseScrollDelta::PixelDelta(p) => p.y as i32,
                };
                if let Ok(mut i) = self.shared.input.lock() {
                    i.motion[2] += notches;
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if !self.device_motion {
                    if let Some((x, y)) = self.cursor {
                        self.add_motion(position.x - x, position.y - y);
                    }
                }
                self.cursor = Some((position.x, position.y));
            }
            WindowEvent::CursorLeft { .. } => self.cursor = None,
            _ => {}
        }
    }

    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event {
            self.device_motion = true;
            self.add_motion(delta.0, delta.1);
        }
    }
}

/// macOS: a `CAMetalLayer` on the window's view (made layer-backed; the layer follows the view's
/// size and scale). Runs on the main thread, as AppKit requires. `None`, with a line, when the
/// window has no AppKit view.
#[cfg(target_os = "macos")]
fn metal_layer_of(handle: raw_window_handle::RawWindowHandle) -> Option<raw_window_metal::Layer> {
    match handle {
        // SAFETY: winit's AppKit handle names the window's live NSView, and this runs on the main
        // thread (the loop's: `run_on_main_thread`), as from_ns_view requires.
        raw_window_handle::RawWindowHandle::AppKit(h) => {
            Some(unsafe { raw_window_metal::Layer::from_ns_view(h.ns_view) })
        }
        other => {
            eprintln!("fork-window: the window is not an AppKit one ({other:?}): no Metal layer");
            None
        }
    }
}

/// The client area `wanted` (at least 1 by 1), no larger than the `monitor` it is on: a window
/// asked for more than the monitor holds (a 2560x1440 game on a smaller monitor) is opened at the
/// monitor's size instead of past its edges (nv-rs#835).
fn fit_monitor(wanted: (u32, u32), monitor: Option<(u32, u32)>) -> (u32, u32) {
    let (mut width, mut height) = (wanted.0.max(1), wanted.1.max(1));
    if let Some((mw, mh)) = monitor.filter(|&(w, h)| w > 0 && h > 0) {
        width = width.min(mw);
        height = height.min(mh);
    }
    (width, height)
}

/// The window system's answer to a size request: `Some` (Wayland: applied at once, no event
/// follows) is the window's size now; whether it changed the size the swap chain reads.
fn took_answer(shared: &Shared, answer: Option<(u32, u32)>) -> bool {
    let Some((width, height)) = answer.filter(|&(w, h)| w > 0 && h > 0) else {
        return false;
    };
    let before = shared.size.load(std::sync::atomic::Ordering::Relaxed);
    shared.set_size(width, height);
    before != shared.size.load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// nv-rs#835: on Wayland the game's `SetWindowPos` to its prefs size is answered at once with
    /// no `Resized` event; the size the swap chain reads must follow that answer, or the picture
    /// stays at the first window's 320x240 and is stretched.
    #[test]
    fn an_immediate_size_answer_is_the_window_size() {
        let shared = Shared::new();
        shared.set_size(320, 240);
        assert!(took_answer(&shared, Some((2560, 1440))));
        assert_eq!(registry::size_of(&shared), (2560, 1440));
        // The same answer again changes nothing; no answer (X11, macOS: an event follows) and an
        // empty one leave the size as it is.
        assert!(!took_answer(&shared, Some((2560, 1440))));
        assert!(!took_answer(&shared, None));
        assert!(!took_answer(&shared, Some((0, 0))));
        assert_eq!(registry::size_of(&shared), (2560, 1440));
    }

    #[test]
    fn a_window_larger_than_its_monitor_is_fitted_to_it() {
        assert_eq!(fit_monitor((2560, 1440), Some((1920, 1080))), (1920, 1080));
        assert_eq!(fit_monitor((1280, 720), Some((2560, 1440))), (1280, 720));
        assert_eq!(fit_monitor((2560, 1440), None), (2560, 1440));
        assert_eq!(fit_monitor((0, 0), Some((0, 0))), (1, 1));
    }

    #[test]
    fn winit_keys_map_to_directinput() {
        assert_eq!(dik_of(KeyCode::KeyW), Some(0x11));
        assert_eq!(dik_of(KeyCode::Escape), Some(0x01));
        assert_eq!(dik_of(KeyCode::ArrowDown), Some(0xd0));
        assert_eq!(dik_of(KeyCode::NumpadEnter), Some(0x9c));
        // The modifiers arrive as keys too (winit sends them as KeyboardInput): Control is the
        // game's sneak, Shift its walk and run, Alt the Alt-Tab test.
        assert_eq!(dik_of(KeyCode::ControlLeft), Some(0x1d));
        assert_eq!(dik_of(KeyCode::ControlRight), Some(0x9d));
        assert_eq!(dik_of(KeyCode::ShiftLeft), Some(0x2a));
        assert_eq!(dik_of(KeyCode::ShiftRight), Some(0x36));
        assert_eq!(dik_of(KeyCode::AltLeft), Some(0x38));
        assert_eq!(dik_of(KeyCode::AltRight), Some(0xb8));
        assert_eq!(dik_of(KeyCode::MediaPlayPause), None);
    }

    /// A window shown again must wake the present loop: every event that says so is one the
    /// handler answers with `changed` (kept as a list to see none is dropped).
    #[test]
    fn coming_back_events_wake_the_present_loop() {
        assert!(wakes(&WindowEvent::Focused(true)));
        assert!(wakes(&WindowEvent::Occluded(false)));
        assert!(wakes(&WindowEvent::RedrawRequested));
        assert!(!wakes(&WindowEvent::Focused(false)));
        assert!(!wakes(&WindowEvent::Occluded(true)));
    }

    #[test]
    fn a_repeated_close_ends_a_game_that_ignored_the_first() {
        let t0 = std::time::Instant::now();
        assert!(!close_ignored(None, t0));
        assert!(!close_ignored(
            Some(t0),
            t0 + std::time::Duration::from_secs(1)
        ));
        assert!(close_ignored(Some(t0), t0 + CLOSE_GRACE));
    }

    #[test]
    fn buttons_map_in_directinputs_order() {
        assert_eq!(button_of(MouseButton::Left), Some(0));
        assert_eq!(button_of(MouseButton::Right), Some(1));
        assert_eq!(button_of(MouseButton::Middle), Some(2));
        assert_eq!(button_of(MouseButton::Back), Some(3));
        assert_eq!(button_of(MouseButton::Forward), Some(4));
        assert_eq!(button_of(MouseButton::Other(9)), None);
    }

    /// Every name in the table is a winit key (a typo would never match a `KeyCode`).
    #[test]
    fn every_table_name_is_a_key() {
        use KeyCode::*;
        let all = [
            Escape,
            Digit1,
            Digit2,
            Digit3,
            Digit4,
            Digit5,
            Digit6,
            Digit7,
            Digit8,
            Digit9,
            Digit0,
            Minus,
            Equal,
            Backspace,
            Tab,
            KeyQ,
            KeyW,
            KeyE,
            KeyR,
            KeyT,
            KeyY,
            KeyU,
            KeyI,
            KeyO,
            KeyP,
            BracketLeft,
            BracketRight,
            Enter,
            ControlLeft,
            KeyA,
            KeyS,
            KeyD,
            KeyF,
            KeyG,
            KeyH,
            KeyJ,
            KeyK,
            KeyL,
            Semicolon,
            Quote,
            Backquote,
            ShiftLeft,
            Backslash,
            KeyZ,
            KeyX,
            KeyC,
            KeyV,
            KeyB,
            KeyN,
            KeyM,
            Comma,
            Period,
            Slash,
            ShiftRight,
            NumpadMultiply,
            AltLeft,
            Space,
            CapsLock,
            F1,
            F2,
            F3,
            F4,
            F5,
            F6,
            F7,
            F8,
            F9,
            F10,
            NumLock,
            ScrollLock,
            Numpad7,
            Numpad8,
            Numpad9,
            NumpadSubtract,
            Numpad4,
            Numpad5,
            Numpad6,
            NumpadAdd,
            Numpad1,
            Numpad2,
            Numpad3,
            Numpad0,
            NumpadDecimal,
            IntlBackslash,
            F11,
            F12,
            NumpadEnter,
            ControlRight,
            NumpadDivide,
            PrintScreen,
            AltRight,
            Pause,
            Home,
            ArrowUp,
            PageUp,
            ArrowLeft,
            ArrowRight,
            End,
            ArrowDown,
            PageDown,
            Insert,
            Delete,
            SuperLeft,
            SuperRight,
            ContextMenu,
        ];
        assert_eq!(all.len(), dik::KEYS.len());
        for k in all {
            assert!(dik_of(k).is_some(), "{k:?}");
        }
    }
}
