//! The fork's window layer for platforms other than Windows.
//!
//! The device layer names the game's window by a bare `isize`; on Windows that is an `HWND`. Here
//! it is an id from [`Window::open`]: the window lives on a thread of its own (as the Windows
//! window thread does), its raw handles are in a registry under that id, and [`surface_for`] turns
//! the id into a wgpu surface. Its keyboard and mouse are kept in the shape of
//! `engine-platform`'s `d3d9real::Input` ([`Input`]: DirectInput key codes, relative motion,
//! buttons), taken once a frame with [`take_input`].
//!
//! Additive and fork-owned: nothing here touches the platform layer upstream reserves. Platforms
//! are modules of `platform`: X11 and Wayland, and macOS (winit 0.30). On macOS the loop runs on
//! the program's main thread, which the program hands over with [`run_on_main_thread`], and each
//! window has a `CAMetalLayer` ([`metal_layer`]). Elsewhere (Windows, which has its own window
//! layer) the same API builds and [`Window::open`] refuses.

pub mod dik;
mod display;
/// Test support for the X11 window system (used by this crate's and engine-platform's tests).
#[cfg(all(unix, not(target_os = "macos")))]
#[doc(hidden)]
pub mod probe;
mod registry;
mod surface;

#[cfg(not(unix))]
mod elsewhere;
#[cfg(unix)]
mod platform;

pub use display::{
    display, hz_of, last_monitor, parse_wanted, pick, remember_monitor, Display, Monitor, Wanted,
};
pub use registry::{
    client_size, close_requested, held_keys, metal_layer, raw_handles, scale_motion, set_game_area,
    take_input, to_game_position, Input,
};
pub use surface::surface_for;

#[cfg(not(unix))]
pub use elsewhere::Window;
#[cfg(target_os = "macos")]
pub use platform::run_on_main_thread;
#[cfg(unix)]
pub use platform::{is_minimised, place, set_minimised, set_title, Window};

/// What the game asked of its window.
#[derive(Clone, Debug)]
pub struct WindowRequest {
    pub title: String,
    /// The client area, in pixels.
    pub width: u32,
    pub height: u32,
    /// A window with a title bar and borders (the game's windowed mode) or a bare one (borderless).
    pub decorations: bool,
    pub visible: bool,
    /// Borderless over the whole monitor (the game's `bFull Screen=1`: exclusive full screen is
    /// not offered here, so the window covers the monitor instead).
    pub fullscreen: bool,
}

impl Default for WindowRequest {
    fn default() -> Self {
        WindowRequest {
            title: "nv-rs".into(),
            width: 1280,
            height: 720,
            decorations: true,
            visible: true,
            fullscreen: false,
        }
    }
}

/// Called on the window thread when the window was activated or resized, or its display changed:
/// the device layer's `window_changed` goes here (it asks the swap chain for a frame at once).
pub type ChangeHook = fn();
