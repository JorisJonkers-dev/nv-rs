#![cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
//! The handle registry: the window's id, its raw handles, its size and its input.

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// What the window's keyboard and mouse did since the last frame (the shape of
/// `engine-platform`'s `d3d9real::Input`).
#[derive(Debug, Clone, Copy)]
pub struct Input {
    /// The keys held, by DirectInput key code (the scan code, 0x80 added for extended keys).
    pub keys: [bool; 256],
    /// The mouse's relative motion and wheel (120 a notch) since the last frame.
    pub motion: [i32; 3],
    /// The buttons held, in DirectInput's order (left, right, middle, X1, X2).
    pub buttons: [bool; 8],
}

impl Default for Input {
    fn default() -> Self {
        Input {
            keys: [false; 256],
            motion: [0; 3],
            buttons: [false; 8],
        }
    }
}

/// The raw handles as they are kept: a pointer inside is only dereferenced by the graphics
/// driver, on the surface's owner's terms ([`crate::surface_for`]).
#[derive(Clone, Copy)]
pub(crate) struct Handles(pub RawDisplayHandle, pub RawWindowHandle);
// SAFETY: the handles are plain values naming the window; the window thread owns the window and
// outlives every surface made from them (the caller's contract of `Window::shutdown`).
unsafe impl Send for Handles {}

pub(crate) struct Shared {
    pub handles: Mutex<Option<Handles>>,
    pub input: Mutex<Input>,
    /// Width in the high 32 bits, height in the low.
    pub size: AtomicU64,
    pub close: AtomicBool,
}

impl Shared {
    pub fn new() -> Arc<Shared> {
        Arc::new(Shared {
            handles: Mutex::new(None),
            input: Mutex::new(Input::default()),
            size: AtomicU64::new(0),
            close: AtomicBool::new(false),
        })
    }

    pub fn set_size(&self, w: u32, h: u32) {
        self.size
            .store(((w as u64) << 32) | h as u64, Ordering::Relaxed);
    }
}

fn table() -> &'static Mutex<HashMap<isize, Arc<Shared>>> {
    static T: OnceLock<Mutex<HashMap<isize, Arc<Shared>>>> = OnceLock::new();
    T.get_or_init(Default::default)
}

/// Ids are never 0 (the device layer reads 0 as no window).
pub(crate) fn register(shared: Arc<Shared>) -> isize {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let id = NEXT.fetch_add(1, Ordering::Relaxed) as isize;
    if let Ok(mut t) = table().lock() {
        t.insert(id, shared);
    }
    id
}

pub(crate) fn unregister(id: isize) {
    if let Ok(mut t) = table().lock() {
        t.remove(&id);
    }
}

fn get(id: isize) -> Option<Arc<Shared>> {
    table().lock().ok()?.get(&id).cloned()
}

/// The raw display and window handles of window `id`, `None` when it is unknown or gone.
pub fn raw_handles(id: isize) -> Option<(RawDisplayHandle, RawWindowHandle)> {
    let s = get(id)?;
    let h = (*s.handles.lock().ok()?)?;
    Some((h.0, h.1))
}

/// The client size of window `id` in pixels (0, 0 when unknown).
pub fn client_size(id: isize) -> (u32, u32) {
    get(id).map_or((0, 0), |s| {
        let v = s.size.load(Ordering::Relaxed);
        ((v >> 32) as u32, v as u32)
    })
}

/// The keys window `id` holds, by DirectInput key code.
pub fn held_keys(id: isize) -> [bool; 256] {
    get(id)
        .and_then(|s| s.input.lock().ok().map(|i| i.keys))
        .unwrap_or([false; 256])
}

/// The input of window `id` for this frame: what is held, and the motion since the last call
/// (taken: it starts again from 0).
pub fn take_input(id: isize) -> Input {
    let Some(s) = get(id) else {
        return Input::default();
    };
    let Ok(mut i) = s.input.lock() else {
        return Input::default();
    };
    let now = *i;
    i.motion = [0; 3];
    now
}

/// Whether the player closed window `id` (the host ends the run, as on Windows).
pub fn close_requested(id: isize) -> bool {
    get(id).is_some_and(|s| s.close.load(Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_not_zero_and_input_is_taken() {
        let s = Shared::new();
        let id = register(s.clone());
        assert_ne!(id, 0);
        s.set_size(640, 360);
        assert_eq!(client_size(id), (640, 360));
        {
            let mut i = s.input.lock().unwrap();
            i.motion = [3, -2, 120];
            i.keys[0x1e] = true;
        }
        let a = take_input(id);
        assert_eq!(a.motion, [3, -2, 120]);
        assert!(a.keys[0x1e]);
        let b = take_input(id);
        assert_eq!(b.motion, [0; 3]);
        assert!(b.keys[0x1e], "held keys stay");
        assert!(raw_handles(id).is_none());
        assert!(!close_requested(id));
        unregister(id);
        assert_eq!(client_size(id), (0, 0));
    }
}
