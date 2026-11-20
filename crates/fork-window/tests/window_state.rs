// SPDX-License-Identifier: GPL-3.0-only
//! #214: what the game does to its window after it is made reaches the real window, read back from
//! the X server (`fork_window::probe`): a new title, `SetWindowPos`, minimise and restore, and the
//! display changing under it. Needs a display, so it runs where `NV_WINDOW_SMOKE=1` (the Xvfb job)
//! and skips elsewhere. The windows are made one at a time: the layer has one event loop and one
//! window.
//!
//! Minimise, restore and the activation that follows need a window manager, which a bare Xvfb
//! lacks: `fork_window::probe::FakeWm` acts as one (it does what the EWMH says a window manager
//! does with the request), so these tests prove what this layer asks and how it follows the answer.

#![cfg(all(unix, not(target_os = "macos")))]

use fork_window::probe::{x_window, FakeWm, Probe};
use fork_window::{Window, WindowRequest};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// The display session for one test, or `None` (a line says why) where there is none.
fn session() -> Option<MutexGuard<'static, ()>> {
    if std::env::var_os("NV_WINDOW_SMOKE").is_none() {
        eprintln!("skipped: NV_WINDOW_SMOKE is not set");
        return None;
    }
    Some(ONE_AT_A_TIME.lock().unwrap_or_else(|p| p.into_inner()))
}

/// Polls `read` until it gives `want`, for up to five seconds (the window system answers on its own
/// time); the last value is in the failure.
fn becomes<T: PartialEq + std::fmt::Debug>(what: &str, want: T, mut read: impl FnMut() -> T) {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let got = read();
        if got == want {
            return;
        }
        assert!(
            Instant::now() < until,
            "{what}: wanted {want:?}, still {got:?} after 5 s"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn open(request: WindowRequest) -> (Window, u32) {
    let window = Window::open(request, None).expect("a window under the display");
    let x = x_window(window.id()).expect("an X11 window (WINIT_UNIX_BACKEND=x11)");
    (window, x)
}

#[test]
fn a_title_set_after_the_window_is_made_reaches_the_x11_property() {
    let Some(_s) = session() else { return };
    let probe = Probe::connect();
    let (window, x) = open(WindowRequest {
        title: "Fallout: New Vegas".into(),
        ..Default::default()
    });
    assert_eq!(probe.title(x).as_deref(), Some("Fallout: New Vegas"));

    assert!(fork_window::set_title(
        window.id(),
        "Fallout: New Vegas - Loading"
    ));
    becomes(
        "the _NET_WM_NAME",
        Some("Fallout: New Vegas - Loading".to_string()),
        || probe.title(x),
    );
    // The older property a window manager that predates the EWMH reads follows too.
    assert_eq!(
        probe.legacy_title(x).as_deref(),
        Some("Fallout: New Vegas - Loading")
    );
    // A title for a window that is not there changes nothing and says so.
    assert!(!fork_window::set_title(window.id() + 1000, "nobody"));
    assert_eq!(
        probe.title(x).as_deref(),
        Some("Fallout: New Vegas - Loading")
    );
}

#[test]
fn set_window_pos_moves_and_sizes_the_window_and_the_swap_chain_follows() {
    let Some(_s) = session() else { return };
    let probe = Probe::connect();
    let (window, x) = open(WindowRequest {
        width: 640,
        height: 480,
        decorations: false,
        ..Default::default()
    });
    let id = window.id();
    assert_eq!(
        (probe.geometry(x).width, probe.geometry(x).height),
        (640, 480)
    );

    // Moved and sized together: the server has the new place and client size, and the size the
    // swap chain is configured from (`client_size`) is the one the server has.
    assert!(fork_window::place(id, Some((40, 30)), Some((800, 600))));
    becomes("the geometry", (40, 30, 800, 600), || {
        let g = probe.geometry(x);
        (g.x, g.y, g.width, g.height)
    });
    becomes("the swap chain's size", (800, 600), || {
        fork_window::client_size(id)
    });

    // `SWP_NOMOVE`: only the size asked for, the place stays.
    assert!(fork_window::place(id, None, Some((512, 384))));
    becomes("the size alone", (40, 30, 512, 384), || {
        let g = probe.geometry(x);
        (g.x, g.y, g.width, g.height)
    });
    becomes("the swap chain's size", (512, 384), || {
        fork_window::client_size(id)
    });

    // `SWP_NOSIZE`: only the place, the size stays.
    assert!(fork_window::place(id, Some((7, 9)), None));
    becomes("the place alone", (7, 9, 512, 384), || {
        let g = probe.geometry(x);
        (g.x, g.y, g.width, g.height)
    });
    assert_eq!(fork_window::client_size(id), (512, 384));

    // Neither asked for: nothing is done.
    assert!(!fork_window::place(id, None, None));
    // A window that is not there is not placed.
    assert!(!fork_window::place(id + 1000, Some((1, 1)), Some((10, 10))));
}

#[test]
fn minimise_and_restore_tell_the_game_it_was_deactivated_and_activated_again() {
    let Some(_s) = session() else { return };
    let _wm = FakeWm::start();
    let probe = Probe::connect();
    let (window, x) = open(WindowRequest {
        width: 320,
        height: 240,
        ..Default::default()
    });
    let id = window.id();
    becomes("the focus on the new window", x, || probe.focus());
    // The first activation, when the window is made, is not the game's: it ignores that one.
    assert_eq!(fork_window::take_input(id).reactivated, None);
    assert!(!fork_window::is_minimised(id));

    assert!(fork_window::set_minimised(id, true));
    becomes("the minimised state", true, || {
        fork_window::is_minimised(id)
    });
    assert!(probe
        .net_wm_state(x)
        .iter()
        .any(|s| s == "_NET_WM_STATE_HIDDEN"));
    // Deactivated: the window lost the focus, and nothing the player held stays held.
    becomes("the lost focus", true, || probe.focus() != x);
    assert_eq!(fork_window::held_keys(id), [false; 256]);
    assert_eq!(fork_window::take_input(id).reactivated, None);

    assert!(fork_window::set_minimised(id, false));
    becomes("the restored state", false, || {
        fork_window::is_minimised(id)
    });
    becomes("the focus back", x, || probe.focus());
    // Activated again: the game is told once (`WA_ACTIVE`), and then not again.
    let mut told = None;
    becomes("the reactivation", Some(1), || {
        told = told.or(fork_window::take_input(id).reactivated);
        told
    });
    assert_eq!(fork_window::take_input(id).reactivated, None);
    // And the window is back at the size it had.
    let g = probe.geometry(x);
    assert_eq!((g.width, g.height), (320, 240));
}

/// A full-screen window held by a window manager, the display changed under it. The refit itself
/// is the window manager's (or compositor's) work: it resizes the window to the monitor's new
/// mode. What this layer owes is to follow it: the window's size, which the swap chain is
/// configured from, becomes the new mode's.
fn display_change_refits_the_full_screen_window() {
    let mut probe = Probe::connect();
    let before = probe.crtc();
    let (window, x) = open(WindowRequest {
        decorations: false,
        fullscreen: true,
        ..Default::default()
    });
    let id = window.id();
    becomes(
        "covering the monitor",
        (before.width, before.height),
        || {
            let g = probe.geometry(x);
            (g.width, g.height)
        },
    );
    assert_eq!(fork_window::client_size(id), (before.width, before.height));
    probe
        .set_display_size(1024, 576)
        .expect("RandR changes the display");
    assert_eq!(probe.crtc().width, 1024, "the server has the new mode");
    becomes("the window refitted to the new mode", (1024, 576), || {
        let g = probe.geometry(x);
        (g.width, g.height)
    });
    becomes("the swap chain's size", (1024, 576), || {
        fork_window::client_size(id)
    });
}

#[test]
fn a_full_screen_window_follows_the_refit_when_the_display_changes() {
    let Some(_s) = session() else { return };
    let _wm = FakeWm::start();
    display_change_refits_the_full_screen_window();
}

/// The same under a real window manager: bare Xvfb has none, and nothing here refits a full-screen
/// window itself, so there is nothing to prove the refit with in CI. By hand: start Xvfb, a window
/// manager that does EWMH full screen and refits on a display change, then
/// `NV_WINDOW_SMOKE=1 cargo test -p fork-window --test window_state -- --ignored`.
#[test]
#[ignore = "needs a real window manager that refits full-screen windows on a display change; bare Xvfb has none"]
fn a_full_screen_window_is_refitted_by_a_real_window_manager_when_the_display_changes() {
    let Some(_s) = session() else { return };
    display_change_refits_the_full_screen_window();
}

/// The display the game seeds its size from is asked of the window system once and kept for the
/// run (`fork_window::display`), so a display change in the middle of a run does not change it:
/// the next run's settings are seeded from the new mode.
#[test]
fn the_display_chosen_at_start_is_kept_when_the_display_changes() {
    let Some(_s) = session() else { return };
    let mut probe = Probe::connect();
    let before = probe.crtc();
    let first = fork_window::display().expect("a monitor under Xvfb");
    assert_eq!(
        (first.monitor.width, first.monitor.height),
        (before.width, before.height)
    );
    probe
        .set_display_size(1024, 576)
        .expect("RandR changes the display");
    assert_eq!(probe.crtc().width, 1024);
    assert_eq!(fork_window::display(), Some(first));
}
