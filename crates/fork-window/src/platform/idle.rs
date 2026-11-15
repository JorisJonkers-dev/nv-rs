//! Keeps the screen from blanking while the game's window is up (winit has no idle inhibit).
//!
//! Wayland: `zwp_idle_inhibit_manager_v1` on the window's own connection (GNOME's Mutter, KWin,
//! wlroots compositors and Hyprland offer it); the inhibitor lives as long as the window, and the
//! compositor honours it while the surface is visible. X11: `xdg-screensaver suspend <window id>`
//! when xdg-utils is installed (it knows each desktop's own way and ends with the window).
//! Neither present: nothing happens and the report says `none`. `NV_IDLE_INHIBIT=0` turns it off.

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use std::process::{Command, Stdio};
use wayland_client::backend::{Backend, ObjectId};
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_registry::WlRegistry, wl_surface::WlSurface};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::wp::idle_inhibit::zv1::client::{
    zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1, zwp_idle_inhibitor_v1::ZwpIdleInhibitorV1,
};

/// What holds the screen awake; dropping it lets go.
pub(crate) enum Inhibit {
    None,
    Wayland {
        conn: Connection,
        inhibitor: ZwpIdleInhibitorV1,
        _manager: ZwpIdleInhibitManagerV1,
    },
    X11 {
        window: u64,
    },
}

impl Inhibit {
    /// The mechanism's name for the log line: `wayland`, `xdg-screensaver` or `none`.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Inhibit::None => "none",
            Inhibit::Wayland { .. } => "wayland",
            Inhibit::X11 { .. } => "xdg-screensaver",
        }
    }

    /// Starts inhibiting for the window with these raw handles, by whichever way its session has.
    pub(crate) fn start(display: RawDisplayHandle, window: RawWindowHandle) -> Inhibit {
        if std::env::var("NV_IDLE_INHIBIT").is_ok_and(|v| v == "0") {
            return Inhibit::None;
        }
        match (display, window) {
            (RawDisplayHandle::Wayland(d), RawWindowHandle::Wayland(w)) => {
                wayland(d.display.as_ptr().cast(), w.surface.as_ptr().cast())
                    .unwrap_or(Inhibit::None)
            }
            (_, RawWindowHandle::Xlib(w)) => x11(w.window),
            (_, RawWindowHandle::Xcb(w)) => x11(u64::from(w.window.get())),
            _ => Inhibit::None,
        }
    }
}

impl Drop for Inhibit {
    fn drop(&mut self) {
        match self {
            Inhibit::None => {}
            Inhibit::Wayland {
                conn, inhibitor, ..
            } => {
                inhibitor.destroy();
                let _ = conn.flush();
            }
            Inhibit::X11 { window } => {
                let _ = Command::new("xdg-screensaver")
                    .args(["resume", &window.to_string()])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
    }
}

struct State;

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: wayland_client::protocol::wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<ZwpIdleInhibitManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpIdleInhibitManagerV1,
        _: <ZwpIdleInhibitManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<ZwpIdleInhibitorV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpIdleInhibitorV1,
        _: <ZwpIdleInhibitorV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<WlSurface, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlSurface,
        _: <WlSurface as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// The inhibitor on the window's surface, made on winit's own connection (a queue of its own, so
/// winit's events are untouched). `None` where the compositor has no idle inhibit manager.
fn wayland(display: *mut std::ffi::c_void, surface: *mut std::ffi::c_void) -> Option<Inhibit> {
    // SAFETY: both pointers are the live wl_display and wl_surface winit made for this window, and
    // the window outlives the inhibitor (it is dropped first, in `Close`).
    let backend = unsafe { Backend::from_foreign_display(display.cast()) };
    let conn = Connection::from_backend(backend);
    let (globals, queue) = registry_queue_init::<State>(&conn).ok()?;
    let qh = queue.handle();
    let manager: ZwpIdleInhibitManagerV1 = globals.bind(&qh, 1..=1, ()).ok()?;
    let id = unsafe { ObjectId::from_ptr(WlSurface::interface(), surface.cast()) }.ok()?;
    let surface = WlSurface::from_id(&conn, id).ok()?;
    let inhibitor = manager.create_inhibitor(&surface, &qh, ());
    conn.flush().ok()?;
    Some(Inhibit::Wayland {
        conn,
        inhibitor,
        _manager: manager,
    })
}

/// `xdg-screensaver suspend` for the window, where xdg-utils is installed.
fn x11(window: u64) -> Inhibit {
    let ran = Command::new("xdg-screensaver")
        .args(["suspend", &window.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match ran {
        Ok(s) if s.success() => Inhibit::X11 { window },
        _ => Inhibit::None,
    }
}
