// SPDX-License-Identifier: GPL-3.0-only
//! Fork-only. The in-game graphics overlay (nv-rs#592, ADR-0019 and its Amendment 1): the panel of
//! `fork_overlay`, opened and closed with F10, drawn at the present after the window's capture (so
//! it is in no capture, report or scene picture), with the port's own glyphs as its font.
//!
//! The host installs a [`Session`] at start: the graphics settings as they were read, the file they
//! are written to and what to do when the values change. A change made in the panel is put into the
//! settings, written to the file at once (a new file beside it, renamed over it) and handed to
//! [`Session::on_change`] for the next frame.
//!
//! Closed, the overlay costs one atomic read at the present; the panel is made when it first opens.
//! Input: closed, only F10 is taken. Open, every key going down is the panel's (the game never sees
//! it), keys coming up still reach the game, so nothing held when the panel opened stays held.
//! Escape or F10 closes it.

use fork_gfx::{Place, Resolved};
use fork_overlay::egui;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// The port's glyphs of printable ASCII (the F11 counter's), the overlay's font.
pub fn glyphs() -> &'static [[u8; 7]; 95] {
    crate::bitmap_font::ascii()
}

/// The overlay's drawing for frames of `format`, its font made from [`glyphs`].
pub fn painter(device: &wgpu::Device, format: wgpu::TextureFormat) -> fork_overlay::Painter {
    fork_overlay::Painter::new(device, format, glyphs())
}

/// The graphics settings of the run, as the overlay edits them.
pub struct Session {
    /// The settings' inputs and the last resolution.
    pub live: fork_gfx::Live,
    /// Where a change is written (`None`: a run that reads no file; changes apply but are not
    /// kept).
    pub path: Option<PathBuf>,
    /// The values the run started with: a `next_start` setting changed since is marked.
    pub started: Option<Resolved>,
    /// Where the player is (the layers by place).
    pub place: Place,
    /// Called with the new values after a change, for the next frame.
    pub on_change: Box<dyn FnMut(&Resolved) + Send>,
    panel: fork_overlay::Panel,
}

impl Session {
    pub fn new(
        live: fork_gfx::Live,
        path: Option<PathBuf>,
        on_change: Box<dyn FnMut(&Resolved) + Send>,
    ) -> Session {
        let mut s = Session {
            live,
            path,
            started: None,
            place: Place::default(),
            on_change,
            panel: fork_overlay::Panel::default(),
        };
        s.started = s.live.update(&Place::default()).cloned();
        s
    }

    /// One frame of the panel: draws it with `ui_frame` (the painter's call) and applies what the
    /// player changed. Returns whether the settings changed.
    pub fn show(&mut self, ctx: &egui::Context) -> bool {
        let _ = self.live.update(&self.place);
        let Some(resolved) = self.live.resolved().cloned() else {
            return false;
        };
        let changes = {
            let view = fork_overlay::View {
                specs: self.live.specs(),
                presets: self.live.presets(),
                resolved: &resolved,
                started: self.started.as_ref(),
                file: self.live.file(),
                place: &self.place,
            };
            self.panel.show(ctx, &view)
        };
        self.apply(&changes)
    }

    /// Puts `changes` into the settings, writes the file and hands the new values on. Whether
    /// anything changed.
    pub fn apply(&mut self, changes: &[fork_overlay::Change]) -> bool {
        let mut changed = false;
        for c in changes {
            changed |= fork_overlay::panel::apply(self.live.file_mut(), c);
        }
        if !changed {
            return false;
        }
        self.panel.status = Some(match (&self.path, self.live.file()) {
            (Some(p), Some(f)) => match write_atomically(p, &f.to_text()) {
                Ok(()) => format!(
                    "Saved to {}",
                    p.file_name().map_or_else(
                        || p.display().to_string(),
                        |n| n.to_string_lossy().into_owned()
                    )
                ),
                Err(e) => format!("Not saved: {e}"),
            },
            _ => "Not saved: this run reads no settings file".to_owned(),
        });
        if let Some(r) = self.live.update(&self.place) {
            let r = r.clone();
            (self.on_change)(&r);
        }
        true
    }

    /// The panel's last status line (tests).
    pub fn status(&self) -> Option<&str> {
        self.panel.status.as_deref()
    }
}

/// Writes `text` to a new file beside `path` and renames it over `path`, so a reader never sees
/// half a file.
pub fn write_atomically(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".new");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);
static OPEN: AtomicBool = AtomicBool::new(false);
static SHIFT: AtomicBool = AtomicBool::new(false);
static EVENTS: Mutex<Vec<egui::Event>> = Mutex::new(Vec::new());

/// Installs the run's settings for the overlay (the host, at start); `None` takes them away.
pub fn install(session: Option<Session>) {
    if let Ok(mut s) = SESSION.lock() {
        *s = session;
    }
}

/// Runs `f` on the installed session, if any.
pub fn with_session<R>(f: impl FnOnce(&mut Session) -> R) -> Option<R> {
    SESSION.lock().ok()?.as_mut().map(f)
}

/// Sets where the player is, for the layers by place.
pub fn set_place(place: Place) {
    with_session(|s| s.place = place);
}

/// Whether the overlay is open.
pub fn is_open() -> bool {
    OPEN.load(Ordering::Relaxed)
}

/// Opens or closes the overlay (tests; the player uses F10).
pub fn set_open(open: bool) {
    OPEN.store(open, Ordering::Relaxed);
    if let Ok(mut e) = EVENTS.lock() {
        e.clear();
    }
}

/// DirectInput codes of the keys the overlay knows.
pub const DIK_F10: u8 = 0x44;
const DIK_ESCAPE: u8 = 0x01;
const DIK_LSHIFT: u8 = 0x2a;
const DIK_RSHIFT: u8 = 0x36;

/// The egui key of a DirectInput code, for the panel's keyboard use.
fn egui_key(code: u8) -> Option<egui::Key> {
    use egui::Key;
    Some(match code {
        0x0f => Key::Tab,
        0x1c | 0x9c => Key::Enter,
        0x39 => Key::Space,
        0xc8 => Key::ArrowUp,
        0xd0 => Key::ArrowDown,
        0xcb => Key::ArrowLeft,
        0xcd => Key::ArrowRight,
        0xc9 => Key::PageUp,
        0xd1 => Key::PageDown,
        0xc7 => Key::Home,
        0xcf => Key::End,
        _ => return None,
    })
}

/// A key of the game's window went down (`repeat`: again while held) or up, by DirectInput code,
/// from any window layer: true when the overlay takes it, and it goes no further. Closed, only F10
/// is taken (its press opens the panel); open, every key going down is taken (F10 and Escape
/// close the panel, the panel's keys become its input), keys coming up go on to the game. While
/// no wgpu device draws the game's window (`crate::frame_rate`'s count) nothing is taken.
pub fn key(code: u8, down: bool, repeat: bool) -> bool {
    if !crate::frame_rate::window_device_present() {
        return false;
    }
    if code == DIK_LSHIFT || code == DIK_RSHIFT {
        SHIFT.store(down, Ordering::Relaxed);
    }
    let open = is_open();
    if code == DIK_F10 || (open && down && code == DIK_ESCAPE) {
        if down && !repeat {
            set_open(!open && code == DIK_F10);
        }
        return true;
    }
    if !open || !down {
        return false;
    }
    if let (Some(k), Ok(mut e)) = (egui_key(code), EVENTS.lock()) {
        let modifiers = egui::Modifiers {
            shift: SHIFT.load(Ordering::Relaxed),
            ..Default::default()
        };
        for pressed in [true, false] {
            e.push(egui::Event::Key {
                key: k,
                physical_key: None,
                pressed,
                repeat,
                modifiers,
            });
        }
    }
    true
}

/// The input since the last frame of the panel.
fn take_events() -> Vec<egui::Event> {
    EVENTS
        .lock()
        .map(|mut e| std::mem::take(&mut *e))
        .unwrap_or_default()
}

/// The panel's scale for a frame `height` pixels high: whole steps of 720 lines (one up to 1439, two
/// at 1440, three at 2160), so its font pixels stay whole screen pixels.
pub fn scale(height: u32) -> f32 {
    (height / 720).max(1) as f32
}

/// The open overlay drawn by `painter` into `view` (`size`), recorded into `encoder`: the
/// installed session's panel (none installed: nothing is drawn). Returns the panel's numbers.
pub fn draw(
    painter: &mut fork_overlay::Painter,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    size: (u32, u32),
) -> Option<fork_overlay::Stats> {
    let mut guard = SESSION.lock().ok()?;
    let session = guard.as_mut()?;
    let events = take_events();
    Some(painter.draw(
        device,
        queue,
        encoder,
        view,
        size,
        scale(size.1),
        events,
        |ctx| {
            session.show(ctx);
        },
    ))
}

/// Runs a wgpu future to its end on this thread (tests that make their own device).
#[doc(hidden)]
pub fn block_on<F: std::future::Future>(f: F) -> F::Output {
    crate::device::block_on(f)
}
