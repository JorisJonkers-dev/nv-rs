//! Systems this layer has no window on yet (macOS: nv-rs#379). The same API as the X11 and Wayland
//! module, so the crates above it build there; opening a window says why it cannot, and the device
//! layer stays off screen (`engine-platform`'s `fork_d3d9rec.rs` logs the reason).

use crate::{ChangeHook, WindowRequest};

/// A window of this layer: never made on this system ([`Window::open`] refuses).
pub struct Window {
    id: isize,
}

impl Window {
    /// Refuses: this system has no window of this layer yet.
    pub fn open(request: WindowRequest, on_change: Option<ChangeHook>) -> Result<Window, String> {
        let _ = (request, on_change);
        Err(format!(
            "the fork's window layer has no window on {} yet",
            std::env::consts::OS
        ))
    }

    /// The id the device layer names this window by (never 0).
    pub fn id(&self) -> isize {
        self.id
    }

    /// Ends the window; there is none to end.
    pub fn shutdown(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_a_window_is_refused_with_the_system_named() {
        let error = Window::open(WindowRequest::default(), None)
            .err()
            .expect("no window on this system");
        assert!(error.contains(std::env::consts::OS), "{error}");
    }

    #[test]
    fn there_is_no_display_to_ask() {
        assert_eq!(crate::display(), None);
    }
}
