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
//! are modules of `platform`; X11 and Wayland (winit 0.30) are the one that exists.

pub mod dik;
mod registry;
mod surface;

#[cfg(all(unix, not(target_os = "macos")))]
mod platform;

pub use registry::{client_size, close_requested, held_keys, raw_handles, take_input, Input};
pub use surface::surface_for;

#[cfg(all(unix, not(target_os = "macos")))]
pub use platform::Window;

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
}

impl Default for WindowRequest {
    fn default() -> Self {
        WindowRequest {
            title: "nv-rs".into(),
            width: 1280,
            height: 720,
            decorations: true,
            visible: true,
        }
    }
}

/// Called on the window thread when the window was activated or resized, or its display changed:
/// the device layer's `window_changed` goes here (it asks the swap chain for a frame at once).
pub type ChangeHook = fn();
