#![cfg_attr(not(unix), allow(dead_code))]
//! The handle registry: the window's id, its raw handles, its size and its input.

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
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
    /// Keys pressed since the last read of the keys, held or not by now: a tap shorter than a
    /// frame (down and up between two reads) still reaches the game as one frame down.
    pub key_taps: [bool; 256],
    /// The same for the buttons.
    pub button_taps: [bool; 8],
    /// Where the cursor last was over the window, as fractions of its client area (the shape of
    /// `d3d9real::Input::pointer`). Not read from the window system yet: always `None`, so the
    /// menus follow the mouse's motion, as before.
    pub pointer: Option<[f64; 2]>,
    /// Whether the cursor moved since the last frame (always `false`, see [`Input::pointer`]).
    pub pointer_moved: bool,
    /// The `WM_ACTIVATE` word when the window became active again (always `None` here).
    pub reactivated: Option<u32>,
}

impl Input {
    /// A key or button changed: a press is kept until a frame has read it.
    pub fn set_key(&mut self, code: u8, down: bool) {
        self.keys[code as usize] = down;
        if down {
            self.key_taps[code as usize] = true;
        }
    }

    /// See [`Input::set_key`].
    pub fn set_button(&mut self, button: usize, down: bool) {
        self.buttons[button] = down;
        if down {
            self.button_taps[button] = true;
        }
    }

    /// Everything released (focus lost): nothing is held and no press is waiting.
    pub fn release_all(&mut self) {
        self.keys = [false; 256];
        self.buttons = [false; 8];
        self.key_taps = [false; 256];
        self.button_taps = [false; 8];
    }
}

impl Default for Input {
    fn default() -> Self {
        Input {
            keys: [false; 256],
            motion: [0; 3],
            buttons: [false; 8],
            key_taps: [false; 256],
            button_taps: [false; 8],
            pointer: None,
            pointer_moved: false,
            reactivated: None,
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
    /// The game's back buffer, width in the high 32 bits, height in the low; 0 until the device
    /// layer says (`Present` stretches it over the whole client area).
    pub game: AtomicU64,
    /// macOS: the address of the window's `CAMetalLayer`, which the window thread keeps retained
    /// while the window is registered; 0 for none ([`metal_layer`]).
    pub metal: AtomicUsize,
}

impl Shared {
    pub fn new() -> Arc<Shared> {
        Arc::new(Shared {
            handles: Mutex::new(None),
            input: Mutex::new(Input::default()),
            size: AtomicU64::new(0),
            close: AtomicBool::new(false),
            game: AtomicU64::new(0),
            metal: AtomicUsize::new(0),
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

/// macOS: the `CAMetalLayer` of window `id`, made on the main thread with the window (AppKit's
/// view may only be touched there, while the game's device makes its surface on another thread);
/// `None` elsewhere, for an unknown window, or a window without one. The layer stays valid while
/// the window does (the caller's contract of `Window::shutdown`).
pub fn metal_layer(id: isize) -> Option<std::ptr::NonNull<std::ffi::c_void>> {
    let address = get(id)?.metal.load(Ordering::Acquire);
    std::ptr::NonNull::new(address as *mut std::ffi::c_void)
}

/// The client size of window `id` in pixels (0, 0 when unknown).
pub fn client_size(id: isize) -> (u32, u32) {
    get(id).map_or((0, 0), |s| {
        let v = s.size.load(Ordering::Relaxed);
        ((v >> 32) as u32, v as u32)
    })
}

/// Tells window `id` the size of the game's back buffer, which `Present` stretches over the
/// client area: the pointer's motion is scaled to it ([`scale_motion`]) so the game's cursor and
/// the pointer cover the same distance.
pub fn set_game_area(id: isize, width: u32, height: u32) {
    if let Some(s) = get(id) {
        s.game
            .store(((width as u64) << 32) | height as u64, Ordering::Relaxed);
    }
}

/// The game's back buffer size of window `id` ((0, 0) until [`set_game_area`]).
pub(crate) fn game_area(s: &Shared) -> (u32, u32) {
    let v = s.game.load(Ordering::Relaxed);
    ((v >> 32) as u32, v as u32)
}

/// Window pixels of pointer motion as game pixels: the back buffer is stretched over the client
/// area, so a move of `delta` window pixels is `delta * game / window` game pixels. `rest` keeps
/// the fraction not yet whole (carried to the next call); the whole part is returned. An unknown
/// size (0) leaves the motion as it is.
pub fn scale_motion(delta: f64, game: u32, window: u32, rest: &mut f64) -> i32 {
    let scaled = if game == 0 || window == 0 {
        delta
    } else {
        delta * game as f64 / window as f64
    } + *rest;
    // A hair added away from zero so 640 / 3 * 3 does not read as 639.9999.
    let whole = (scaled + scaled.signum() * 1e-9).trunc();
    *rest = scaled - whole;
    whole as i32
}

/// The game's cursor position for a pointer at (`x`, `y`) in the window (both in window pixels):
/// the same place in the stretched back buffer, kept inside it.
pub fn to_game_position(x: f64, y: f64, game: (u32, u32), window: (u32, u32)) -> (i32, i32) {
    let map = |v: f64, g: u32, w: u32| {
        if g == 0 || w == 0 {
            v as i32
        } else {
            ((v * g as f64 / w as f64) as i32).clamp(0, g as i32 - 1)
        }
    };
    (map(x, game.0, window.0), map(y, game.1, window.1))
}

/// The keys window `id` holds, by DirectInput key code.
///
/// A key pressed and released since the last call counts as held in this one (a tap shorter than
/// a frame: Control's sneak toggle, a single click), and is released in the next.
pub fn held_keys(id: isize) -> [bool; 256] {
    let Some(s) = get(id) else {
        return [false; 256];
    };
    let Ok(mut i) = s.input.lock() else {
        return [false; 256];
    };
    let mut held = i.keys;
    for (h, tap) in held.iter_mut().zip(i.key_taps.iter_mut()) {
        *h |= std::mem::take(tap);
    }
    held
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
    let mut now = *i;
    for (b, tap) in now.buttons.iter_mut().zip(i.button_taps.iter_mut()) {
        *b |= std::mem::take(tap);
    }
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
    fn a_metal_layer_is_known_only_while_its_window_is_registered() {
        assert_eq!(metal_layer(-7), None, "an unknown window");
        let shared = Shared::new();
        let id = register(shared.clone());
        assert_eq!(
            metal_layer(id),
            None,
            "a window without a layer (every system but macOS)"
        );
        let mut stand_in = 0u8;
        shared
            .metal
            .store(std::ptr::addr_of_mut!(stand_in) as usize, Ordering::Release);
        assert_eq!(
            metal_layer(id).map(|p| p.as_ptr() as usize),
            Some(std::ptr::addr_of_mut!(stand_in) as usize)
        );
        unregister(id);
        assert_eq!(metal_layer(id), None, "gone with its window");
    }

    #[test]
    fn pointer_motion_is_scaled_to_the_back_buffer() {
        let mut rest = 0.0;
        // 1080 window pixels across a 640 pixel back buffer: 1080 of motion is 640.
        assert_eq!(scale_motion(1080.0, 640, 1080, &mut rest), 640);
        assert_eq!(scale_motion(-540.0, 640, 1080, &mut rest), -320);
        // Small moves are not lost: the fraction carries over.
        let mut rest = 0.0;
        let total: i32 = (0..1080)
            .map(|_| scale_motion(1.0, 640, 1080, &mut rest))
            .sum();
        assert_eq!(total, 640);
        // An unknown size leaves the motion alone.
        assert_eq!(scale_motion(7.0, 0, 1080, &mut 0.0), 7);
    }

    #[test]
    fn pointer_positions_map_into_the_back_buffer() {
        assert_eq!(to_game_position(0.0, 0.0, (640, 480), (1080, 1920)), (0, 0));
        assert_eq!(
            to_game_position(540.0, 960.0, (640, 480), (1080, 1920)),
            (320, 240)
        );
        assert_eq!(
            to_game_position(5000.0, -3.0, (640, 480), (1080, 1920)),
            (639, 0)
        );
        assert_eq!(to_game_position(10.0, 20.0, (0, 0), (0, 0)), (10, 20));
    }

    /// A Control tap that starts and ends between two frames' reads (a hitching frame) reaches the
    /// game as one frame down, then up: the sneak toggle works however short the press is.
    #[test]
    fn a_tap_between_two_frames_is_not_lost() {
        let s = Shared::new();
        let id = register(s.clone());
        {
            let mut i = s.input.lock().unwrap();
            i.set_key(0x1d, true);
            i.set_key(0x1d, false);
            i.set_button(0, true);
            i.set_button(0, false);
        }
        assert!(held_keys(id)[0x1d], "the tap is seen by the frame");
        assert!(!held_keys(id)[0x1d], "and released by the next");
        assert!(take_input(id).buttons[0]);
        assert!(!take_input(id).buttons[0]);
        // Still held: stays down, and a press lost with the focus is not replayed.
        {
            let mut i = s.input.lock().unwrap();
            i.set_key(0x9d, true);
            assert!(i.key_taps[0x9d]);
            i.release_all();
        }
        assert!(!held_keys(id)[0x9d]);
        unregister(id);
    }

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
