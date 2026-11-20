// SPDX-License-Identifier: GPL-3.0-only
//! #218: the game's full screen is a borderless window over the monitor (exclusive full screen and
//! a video mode change are not offered here). These tests read the window back from the X server
//! (`fork_window::probe`): no caption, exactly the monitor, on the monitor the display choice
//! names, and the framed window of the same game back at its size when `bFull Screen=0`.
//!
//! The game's device reset to another size, the back buffer and the monitor's own mode are in
//! `engine-platform` (`fork_d3d9rec.rs`, which has the device). Needs a display, so it runs where
//! `NV_WINDOW_SMOKE=1` (the Xvfb job) and skips elsewhere. Full screen on X11 is a request to the
//! window manager (`_NET_WM_STATE_FULLSCREEN`), which bare Xvfb lacks: `FakeWm` does what the EWMH
//! says a window manager does with it.

#![cfg(all(unix, not(target_os = "macos")))]

use fork_window::probe::{x_window, FakeWm, Probe};
use fork_window::{Window, WindowRequest};
use std::sync::{Mutex, MutexGuard, Once};
use std::time::{Duration, Instant};

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
static CHOICE: Once = Once::new();

/// The display session for one test, or `None` where there is none. `NV_MONITOR` names the output
/// (Xvfb's one is `screen`), as a player's choice of display does, before the event loop starts.
fn session() -> Option<MutexGuard<'static, ()>> {
    if std::env::var_os("NV_WINDOW_SMOKE").is_none() {
        eprintln!("skipped: NV_WINDOW_SMOKE is not set");
        return None;
    }
    let guard = ONE_AT_A_TIME.lock().unwrap_or_else(|p| p.into_inner());
    CHOICE.call_once(|| std::env::set_var("NV_MONITOR", "screen"));
    Some(guard)
}

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

/// The game's request for `bFull Screen=1`: a window with no caption (`WS_POPUP`), full screen,
/// whatever size `iSize` says.
fn full_screen(width: u32, height: u32) -> WindowRequest {
    WindowRequest {
        width,
        height,
        decorations: false,
        fullscreen: true,
        ..Default::default()
    }
}

#[test]
fn full_screen_is_a_captionless_window_over_exactly_the_monitor_the_choice_names() {
    let Some(_s) = session() else { return };
    let _wm = FakeWm::start();
    let probe = Probe::connect();
    let monitor = probe.crtc();
    let chosen = fork_window::display().expect("a monitor under Xvfb");
    assert_eq!(
        (chosen.monitor.width, chosen.monitor.height),
        (monitor.width, monitor.height),
        "NV_MONITOR=screen names RandR's output"
    );

    assert_eq!(chosen.why, "NV_MONITOR", "the choice is the named output");

    // `iSize` smaller than the monitor: the window still covers all of it.
    let window = Window::open(full_screen(800, 600), None).expect("a full-screen window");
    let id = window.id();
    let x = x_window(id).expect("an X11 window");
    becomes(
        "the window over the monitor",
        (monitor.x, monitor.y, monitor.width, monitor.height),
        || {
            let g = probe.geometry(x);
            (g.x, g.y, g.width, g.height)
        },
    );
    assert!(!probe.decorated(x), "no caption and no borders");
    assert!(probe
        .net_wm_state(x)
        .iter()
        .any(|s| s == "_NET_WM_STATE_FULLSCREEN"));
    // What the swap chain is configured from is the monitor, not the 800x600 asked for.
    becomes(
        "the swap chain's size",
        (monitor.width, monitor.height),
        || fork_window::client_size(id),
    );
    // Nothing changed the monitor's own mode.
    assert_eq!(probe.crtc(), monitor);
    assert_eq!(probe.screen_size(), (monitor.width, monitor.height));
}

#[test]
fn leaving_full_screen_gives_the_framed_window_back_at_its_size() {
    let Some(_s) = session() else { return };
    let _wm = FakeWm::start();
    let probe = Probe::connect();
    let monitor = probe.crtc();

    // `bFull Screen=1`, then the player's next run with `bFull Screen=0` at the size it had.
    let full = Window::open(full_screen(800, 600), None).expect("a full-screen window");
    let x = x_window(full.id()).expect("an X11 window");
    becomes(
        "covering the monitor",
        (monitor.width, monitor.height),
        || {
            let g = probe.geometry(x);
            (g.width, g.height)
        },
    );
    drop(full);

    let framed = Window::open(
        WindowRequest {
            width: 800,
            height: 600,
            decorations: true,
            fullscreen: false,
            ..Default::default()
        },
        None,
    )
    .expect("a framed window");
    let id = framed.id();
    let x = x_window(id).expect("an X11 window");
    becomes("the framed size", (800, 600), || {
        let g = probe.geometry(x);
        (g.width, g.height)
    });
    becomes("the swap chain's size", (800, 600), || {
        fork_window::client_size(id)
    });
    assert!(probe.decorated(x), "a title bar and borders again");
    assert!(!probe
        .net_wm_state(x)
        .iter()
        .any(|s| s == "_NET_WM_STATE_FULLSCREEN"));
    assert_eq!(probe.crtc(), monitor, "the monitor's mode is as it was");
}
