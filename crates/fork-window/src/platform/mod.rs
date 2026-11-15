//! X11 and Wayland: a winit 0.30 event loop on a thread of its own.

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
use winit::platform::wayland::EventLoopBuilderExtWayland;
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
}

/// The one event loop of the process. winit allows one per process (a second is refused with
/// "EventLoop can't be recreated"), so the monitor query and every window go through it: it is
/// made on the first use, on a thread of its own that lives to the end of the process.
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
    let primary = event_loop
        .primary_monitor()
        .and_then(|p| handles.iter().position(|h| *h == p));
    let wanted = crate::parse_wanted(std::env::var("NV_MONITOR").ok().as_deref());
    crate::pick(&monitors, wanted, primary)
}

fn run(ready: mpsc::Sender<Result<EventLoopProxy<Command>, String>>) {
    // winit takes Wayland when the session is one, else X11 (`WINIT_UNIX_BACKEND=x11` forces X11;
    // a second event loop cannot be made after a failed one, so there is no retry here).
    let mut builder = EventLoop::<Command>::with_user_event();
    // winit refuses an event loop off the main thread unless asked, per backend.
    EventLoopBuilderExtX11::with_any_thread(&mut builder, true);
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
        let attrs = WindowAttributes::default()
            .with_title(self.request.title.clone())
            .with_inner_size(PhysicalSize::new(self.request.width, self.request.height))
            .with_decorations(self.request.decorations)
            .with_resizable(false)
            .with_visible(self.request.visible)
            .with_fullscreen(self.request.fullscreen.then(|| {
                // Over the monitor the pick names (`NV_MONITOR`, the primary, the first).
                let chosen =
                    pick_on(event_loop).and_then(|d| event_loop.available_monitors().nth(d.index));
                winit::window::Fullscreen::Borderless(chosen)
            }));
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
        self.id = registry::register(self.shared.clone());
        self.window = Some(window);
        self.hold_pointer();
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
    fn hold_pointer(&self) {
        let Some(w) = &self.window else { return };
        w.set_cursor_visible(false);
        if w.set_cursor_grab(CursorGrabMode::Locked).is_err()
            && w.set_cursor_grab(CursorGrabMode::Confined).is_err()
        {
            eprintln!("fork-window: the pointer cannot be held in the window");
        }
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
                self.window = None;
                let _ = done.send(());
            }
        }
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.shared
                    .close
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
            WindowEvent::Resized(s) => {
                self.shared.set_size(s.width, s.height);
                self.changed();
            }
            WindowEvent::ScaleFactorChanged { .. } => self.changed(),
            WindowEvent::Focused(on) => {
                if !on {
                    // Losing the focus lets go of everything held (as the Windows layer does).
                    if let Ok(mut i) = self.shared.input.lock() {
                        i.keys = [false; 256];
                        i.buttons = [false; 8];
                    }
                    self.cursor = None;
                    self.release_pointer();
                } else {
                    self.hold_pointer();
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
                        i.keys[k as usize] = event.state == ElementState::Pressed;
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let (Some(b), Ok(mut i)) = (button_of(button), self.shared.input.lock()) {
                    i.buttons[b] = state == ElementState::Pressed;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winit_keys_map_to_directinput() {
        assert_eq!(dik_of(KeyCode::KeyW), Some(0x11));
        assert_eq!(dik_of(KeyCode::Escape), Some(0x01));
        assert_eq!(dik_of(KeyCode::ArrowDown), Some(0xd0));
        assert_eq!(dik_of(KeyCode::NumpadEnter), Some(0x9c));
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
