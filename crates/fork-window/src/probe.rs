// SPDX-License-Identifier: GPL-3.0-only
//! Test support, X11 only: what the X server says about a window of this layer, and the two
//! things a bare virtual X server (Xvfb) lacks that the window layer depends on.
//!
//! - [`Probe`] is a second client of the same server: it reads a window's title, its geometry on
//!   the screen, its decoration hint and its `_NET_WM_STATE`, and changes RandR's mode of the
//!   screen (the way a player changes the display's resolution), so a test sees what the server
//!   holds rather than what this layer believes.
//! - [`FakeWm`] stands in for a window manager: it acts on the requests a client makes of one
//!   (map, configure, `_NET_WM_STATE_FULLSCREEN`, minimise as `WM_CHANGE_STATE`, activate as
//!   `_NET_ACTIVE_WINDOW`) the way the EWMH says a window manager does, and refits the windows it
//!   holds full screen when the display changes. Without one, Xvfb ignores them. It checks that
//!   this layer asks right and follows the answer, not what any real window manager does.

use raw_window_handle::RawWindowHandle;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::protocol::randr::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{
    self, AtomEnum, ConfigureWindowAux, ConnectionExt as _, EventMask, InputFocus, PropMode, Window,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::CURRENT_TIME;

/// The X window of window `id` of this layer (`None` when it is gone or not an X11 window).
pub fn x_window(id: isize) -> Option<Window> {
    match crate::raw_handles(id)?.1 {
        RawWindowHandle::Xlib(h) => Some(h.window as Window),
        RawWindowHandle::Xcb(h) => Some(h.window.get()),
        _ => None,
    }
}

/// A window's place and size on the screen, in pixels (its client area).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// A monitor as RandR holds it (the first CRTC).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Crtc {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// The mode's id: unchanged exactly when nothing changed the monitor's own mode.
    pub mode: u32,
}

/// A client of the X server the window layer runs on.
pub struct Probe {
    conn: RustConnection,
    root: Window,
    /// The monitor's size before [`Probe::set_display_size`] changed it.
    original: Option<(u16, u16)>,
}

impl Default for Probe {
    fn default() -> Self {
        Probe::connect()
    }
}

impl Probe {
    /// Connects to `$DISPLAY`.
    pub fn connect() -> Probe {
        let (conn, screen) = RustConnection::connect(None).expect("connect to the X server");
        let root = conn.setup().roots[screen].root;
        Probe {
            conn,
            root,
            original: None,
        }
    }

    fn atom(&self, name: &str) -> u32 {
        self.conn
            .intern_atom(false, name.as_bytes())
            .expect("intern_atom")
            .reply()
            .expect("intern_atom reply")
            .atom
    }

    fn property(&self, window: Window, property: u32, kind: impl Into<u32>) -> Vec<u8> {
        self.conn
            .get_property(false, window, property, kind, 0, 1 << 12)
            .expect("get_property")
            .reply()
            .map(|r| r.value)
            .unwrap_or_default()
    }

    /// The UTF-8 title a window manager shows (`_NET_WM_NAME`).
    pub fn title(&self, window: Window) -> Option<String> {
        let value = self.property(window, self.atom("_NET_WM_NAME"), self.atom("UTF8_STRING"));
        (!value.is_empty()).then(|| String::from_utf8_lossy(&value).into_owned())
    }

    /// The title older window managers read (`WM_NAME`).
    pub fn legacy_title(&self, window: Window) -> Option<String> {
        let value = self.property(window, AtomEnum::WM_NAME.into(), AtomEnum::STRING);
        (!value.is_empty()).then(|| String::from_utf8_lossy(&value).into_owned())
    }

    /// The window's class hint (`WM_CLASS`: instance, then class), which the dock groups windows by.
    pub fn wm_class(&self, window: Window) -> Vec<String> {
        let value = self.property(window, AtomEnum::WM_CLASS.into(), AtomEnum::STRING);
        value
            .split(|&b| b == 0)
            .filter(|p| !p.is_empty())
            .map(|p| String::from_utf8_lossy(p).into_owned())
            .collect()
    }

    /// The client area's place on the screen and its size, as the server has them now.
    pub fn geometry(&self, window: Window) -> Geometry {
        let size = self
            .conn
            .get_geometry(window)
            .expect("get_geometry")
            .reply()
            .expect("get_geometry reply");
        let origin = self
            .conn
            .translate_coordinates(window, self.root, 0, 0)
            .expect("translate_coordinates")
            .reply()
            .expect("translate_coordinates reply");
        Geometry {
            x: i32::from(origin.dst_x),
            y: i32::from(origin.dst_y),
            width: u32::from(size.width),
            height: u32::from(size.height),
        }
    }

    /// Whether the window asks for a frame (a title bar and borders): `_MOTIF_WM_HINTS` says
    /// "no decorations" when it does not; a window with no such hint is framed.
    pub fn decorated(&self, window: Window) -> bool {
        let atom = self.atom("_MOTIF_WM_HINTS");
        let value = self.property(window, atom, atom);
        let word = |i: usize| {
            value
                .get(4 * i..4 * i + 4)
                .map(|b| u32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
        };
        // flags, functions, decorations: the decorations field counts when bit 1 of the flags is set.
        match (word(0), word(2)) {
            (Some(flags), Some(decorations)) if flags & 2 != 0 => decorations != 0,
            _ => true,
        }
    }

    /// The names of the window's `_NET_WM_STATE` atoms (`_NET_WM_STATE_FULLSCREEN`, `..._HIDDEN`).
    pub fn net_wm_state(&self, window: Window) -> Vec<String> {
        let value = self.property(window, self.atom("_NET_WM_STATE"), AtomEnum::ATOM);
        value
            .chunks_exact(4)
            .map(|b| u32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
            .map(|atom| {
                let name = self
                    .conn
                    .get_atom_name(atom)
                    .expect("get_atom_name")
                    .reply()
                    .expect("get_atom_name reply")
                    .name;
                String::from_utf8_lossy(&name).into_owned()
            })
            .collect()
    }

    /// The window that has the keyboard focus (0: none, 1: the pointer's).
    pub fn focus(&self) -> Window {
        self.conn
            .get_input_focus()
            .expect("get_input_focus")
            .reply()
            .expect("get_input_focus reply")
            .focus
    }

    /// The screen's size, the root window's.
    pub fn screen_size(&self) -> (u32, u32) {
        let g = self
            .conn
            .get_geometry(self.root)
            .expect("get_geometry")
            .reply()
            .expect("get_geometry reply");
        (u32::from(g.width), u32::from(g.height))
    }

    fn resources(&self) -> randr::GetScreenResourcesCurrentReply {
        self.conn
            .randr_get_screen_resources_current(self.root)
            .expect("randr_get_screen_resources_current")
            .reply()
            .expect("randr reply")
    }

    /// The first monitor (CRTC) as RandR holds it.
    pub fn crtc(&self) -> Crtc {
        let r = self.resources();
        let info = self
            .conn
            .randr_get_crtc_info(r.crtcs[0], r.config_timestamp)
            .expect("randr_get_crtc_info")
            .reply()
            .expect("randr_get_crtc_info reply");
        Crtc {
            x: i32::from(info.x),
            y: i32::from(info.y),
            width: u32::from(info.width),
            height: u32::from(info.height),
            mode: info.mode,
        }
    }

    /// Changes the display to `width` by `height` as a player does in the desktop's display
    /// settings: a mode of that size is made, given to the output and set on its CRTC, and the
    /// screen follows. The mode lives as long as this client's connection does, and the display
    /// goes back to what it was when the probe is dropped. Xvfb's screen cannot grow past the size
    /// it started with, so this shrinks it or returns it.
    pub fn set_display_size(&mut self, width: u16, height: u16) -> Result<(), String> {
        let r = self.resources();
        let (crtc, output) = (r.crtcs[0], r.outputs[0]);
        let before = self.crtc();
        if self.original.is_none() {
            self.original = Some((before.width as u16, before.height as u16));
        }
        let known = r
            .modes
            .iter()
            .find(|m| m.width == width && m.height == height);
        let mode = match known {
            Some(m) => m.id,
            None => {
                let name = format!("{width}x{height}");
                let info = randr::ModeInfo {
                    id: 0,
                    width,
                    height,
                    dot_clock: u32::from(width) * u32::from(height) * 60,
                    hsync_start: width + 8,
                    hsync_end: width + 16,
                    htotal: width + 32,
                    hskew: 0,
                    vsync_start: height + 2,
                    vsync_end: height + 4,
                    vtotal: height + 8,
                    name_len: name.len() as u16,
                    mode_flags: randr::ModeFlag::from(0u8),
                };
                let id = self
                    .conn
                    .randr_create_mode(self.root, info, name.as_bytes())
                    .map_err(|e| e.to_string())?
                    .reply()
                    .map_err(|e| format!("create mode: {e}"))?
                    .mode;
                self.conn
                    .randr_add_output_mode(output, id)
                    .map_err(|e| e.to_string())?
                    .check()
                    .map_err(|e| format!("add output mode: {e}"))?;
                id
            }
        };
        let (screen_w, screen_h) = self.screen_size();
        let grows = u32::from(width) > screen_w || u32::from(height) > screen_h;
        let millimetres = |pixels: u16| u32::from(pixels) * 254 / 960;
        let size = |probe: &Probe| {
            probe
                .conn
                .randr_set_screen_size(
                    probe.root,
                    width,
                    height,
                    millimetres(width),
                    millimetres(height),
                )
                .map_err(|e| e.to_string())?
                .check()
                .map_err(|e| format!("set screen size: {e}"))
        };
        // The screen has to hold the monitor all the time: bigger first, smaller after.
        if grows {
            size(self)?;
        }
        let set = self
            .conn
            .randr_set_crtc_config(
                crtc,
                CURRENT_TIME,
                r.config_timestamp,
                0,
                0,
                mode,
                randr::Rotation::ROTATE0,
                &[output],
            )
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| format!("set crtc config: {e}"))?;
        if set.status != randr::SetConfig::SUCCESS {
            return Err(format!("RandR refused the mode: {:?}", set.status));
        }
        if !grows {
            size(self)?;
        }
        self.conn.flush().map_err(|e| e.to_string())
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        if let Some((w, h)) = self.original.take() {
            let _ = self.set_display_size(w, h);
        }
    }
}

/// A stand-in window manager. See the module's documentation. Dropped, it stops and lets go of the
/// root window.
pub struct FakeWm {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl FakeWm {
    /// Becomes the screen's window manager. Panics when another one is there.
    pub fn start() -> FakeWm {
        let (conn, screen) = RustConnection::connect(None).expect("connect to the X server");
        let root = conn.setup().roots[screen].root;
        conn.change_window_attributes(
            root,
            &xproto::ChangeWindowAttributesAux::new().event_mask(
                EventMask::SUBSTRUCTURE_REDIRECT
                    | EventMask::SUBSTRUCTURE_NOTIFY
                    | EventMask::PROPERTY_CHANGE,
            ),
        )
        .expect("select input on the root")
        .check()
        .expect("another window manager is running");
        // A window manager hears of a display change, to refit the windows it holds full screen.
        conn.randr_select_input(root, randr::NotifyMask::SCREEN_CHANGE)
            .expect("select RandR input")
            .check()
            .expect("select RandR input");
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::Builder::new()
            .name("fake-wm".into())
            .spawn(move || Wm::new(conn, root).run(&flag))
            .expect("fake window manager thread");
        FakeWm {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for FakeWm {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Wm {
    conn: RustConnection,
    root: Window,
    atoms: HashMap<&'static str, u32>,
    /// The geometry a full-screen window had before, to give back when it leaves full screen.
    before_fullscreen: HashMap<Window, (i16, i16, u16, u16)>,
}

const NAMES: [&str; 6] = [
    "_NET_WM_STATE",
    "_NET_WM_STATE_FULLSCREEN",
    "_NET_WM_STATE_HIDDEN",
    "_NET_ACTIVE_WINDOW",
    "WM_CHANGE_STATE",
    "WM_STATE",
];

impl Wm {
    fn new(conn: RustConnection, root: Window) -> Wm {
        let atoms = NAMES
            .iter()
            .map(|&n| {
                let atom = conn
                    .intern_atom(false, n.as_bytes())
                    .expect("intern_atom")
                    .reply()
                    .expect("intern_atom reply")
                    .atom;
                (n, atom)
            })
            .collect();
        Wm {
            conn,
            root,
            atoms,
            before_fullscreen: HashMap::new(),
        }
    }

    fn run(&mut self, stop: &AtomicBool) {
        while !stop.load(Ordering::Relaxed) {
            match self.conn.poll_for_event() {
                Ok(Some(event)) => self.handle(event),
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(2)),
                Err(_) => break,
            }
            let _ = self.conn.flush();
        }
        // Let go of the root window: the next window manager (or test) may take it.
        let _ = self.conn.change_window_attributes(
            self.root,
            &xproto::ChangeWindowAttributesAux::new().event_mask(EventMask::NO_EVENT),
        );
        let _ = self.conn.flush();
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::MapRequest(e) => {
                let _ = self.conn.map_window(e.window);
                let _ = self
                    .conn
                    .set_input_focus(InputFocus::POINTER_ROOT, e.window, CURRENT_TIME);
            }
            Event::ConfigureRequest(e) => {
                let mut aux = ConfigureWindowAux::new();
                let mask = e.value_mask;
                if mask.contains(xproto::ConfigWindow::X) {
                    aux = aux.x(i32::from(e.x));
                }
                if mask.contains(xproto::ConfigWindow::Y) {
                    aux = aux.y(i32::from(e.y));
                }
                if mask.contains(xproto::ConfigWindow::WIDTH) {
                    aux = aux.width(u32::from(e.width));
                }
                if mask.contains(xproto::ConfigWindow::HEIGHT) {
                    aux = aux.height(u32::from(e.height));
                }
                let _ = self.conn.configure_window(e.window, &aux);
            }
            Event::ClientMessage(m) => self.message(m),
            // The display changed: the windows held full screen are refitted to their monitor.
            Event::RandrScreenChangeNotify(_) => {
                let held: Vec<Window> = self.before_fullscreen.keys().copied().collect();
                for window in held {
                    self.fullscreen(window, true);
                }
            }
            _ => {}
        }
    }

    fn message(&mut self, m: xproto::ClientMessageEvent) {
        let d = m.data.as_data32();
        let (state, fullscreen, hidden) = (
            self.atoms["_NET_WM_STATE"],
            self.atoms["_NET_WM_STATE_FULLSCREEN"],
            self.atoms["_NET_WM_STATE_HIDDEN"],
        );
        if m.type_ == state && (d[1] == fullscreen || d[2] == fullscreen) {
            let on = match d[0] {
                0 => false,
                1 => true,
                _ => !self.before_fullscreen.contains_key(&m.window),
            };
            self.fullscreen(m.window, on);
        } else if m.type_ == self.atoms["WM_CHANGE_STATE"] && d[0] == 3 {
            // Iconic: hidden, unmapped, and the focus goes to no window.
            let _ = self.conn.change_property32(
                PropMode::REPLACE,
                m.window,
                state,
                AtomEnum::ATOM,
                &[hidden],
            );
            // The focus goes to the root (no client has it), then the window is taken off the screen.
            let _ = self
                .conn
                .set_input_focus(InputFocus::POINTER_ROOT, self.root, CURRENT_TIME);
            let _ = self.conn.unmap_window(m.window);
        } else if m.type_ == self.atoms["_NET_ACTIVE_WINDOW"] {
            let _ = self.conn.delete_property(m.window, state);
            let _ = self.conn.map_window(m.window);
            let _ = self
                .conn
                .set_input_focus(InputFocus::POINTER_ROOT, m.window, CURRENT_TIME);
        }
    }

    /// The monitor (CRTC) the window is on, else the first.
    fn monitor_of(&self, window: Window) -> Option<(i16, i16, u16, u16)> {
        let at = self
            .conn
            .translate_coordinates(window, self.root, 0, 0)
            .ok()?
            .reply()
            .ok()?;
        let r = self
            .conn
            .randr_get_screen_resources_current(self.root)
            .ok()?
            .reply()
            .ok()?;
        let crtcs: Vec<_> = r
            .crtcs
            .iter()
            .filter_map(|&c| {
                self.conn
                    .randr_get_crtc_info(c, r.config_timestamp)
                    .ok()?
                    .reply()
                    .ok()
            })
            .filter(|i| i.width > 0 && i.height > 0)
            .collect();
        let on = |i: &&randr::GetCrtcInfoReply| {
            at.dst_x >= i.x
                && at.dst_y >= i.y
                && i32::from(at.dst_x) < i32::from(i.x) + i32::from(i.width)
                && i32::from(at.dst_y) < i32::from(i.y) + i32::from(i.height)
        };
        crtcs
            .iter()
            .find(on)
            .or_else(|| crtcs.first())
            .map(|i| (i.x, i.y, i.width, i.height))
    }

    fn fullscreen(&mut self, window: Window, on: bool) {
        let state = self.atoms["_NET_WM_STATE"];
        let fullscreen = self.atoms["_NET_WM_STATE_FULLSCREEN"];
        if on {
            let Some((x, y, w, h)) = self.monitor_of(window) else {
                return;
            };
            if let (Ok(c), Ok(t)) = (
                self.conn.get_geometry(window).map(|c| c.reply()),
                self.conn
                    .translate_coordinates(window, self.root, 0, 0)
                    .map(|c| c.reply()),
            ) {
                if let (Ok(c), Ok(t)) = (c, t) {
                    self.before_fullscreen
                        .entry(window)
                        .or_insert((t.dst_x, t.dst_y, c.width, c.height));
                }
            }
            let _ = self.conn.change_property32(
                PropMode::REPLACE,
                window,
                state,
                AtomEnum::ATOM,
                &[fullscreen],
            );
            let _ = self.conn.configure_window(
                window,
                &ConfigureWindowAux::new()
                    .x(i32::from(x))
                    .y(i32::from(y))
                    .width(u32::from(w))
                    .height(u32::from(h)),
            );
        } else {
            let _ = self.conn.delete_property(window, state);
            if let Some((x, y, w, h)) = self.before_fullscreen.remove(&window) {
                let _ = self.conn.configure_window(
                    window,
                    &ConfigureWindowAux::new()
                        .x(i32::from(x))
                        .y(i32::from(y))
                        .width(u32::from(w))
                        .height(u32::from(h)),
                );
            }
        }
    }
}
