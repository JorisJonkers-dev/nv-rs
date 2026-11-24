// SPDX-License-Identifier: GPL-3.0-only
//! The small window of a run that draws off screen (`fork_window::mirror`), read back from the X
//! server: named by the run's label (class hint and title), opened minimised, without a picture of the
//! game's. Needs a display: runs where `NV_WINDOW_SMOKE=1` (the Xvfb job) and skips elsewhere. Its own
//! test binary, so the process's one event loop is this test's.

#![cfg(all(unix, not(target_os = "macos")))]

use fork_window::probe::{x_window, FakeWm, Probe};
use std::time::{Duration, Instant};

#[test]
fn a_run_window_is_named_by_the_label_and_starts_minimised() {
    if std::env::var_os("NV_WINDOW_SMOKE").is_none() {
        eprintln!("skipped: NV_WINDOW_SMOKE is not set");
        return;
    }
    std::env::set_var("GAMERUN_LABEL", "gate14-mojave-ttw");
    let _wm = FakeWm::start();
    let probe = Probe::connect();
    let label = fork_window::run_label();
    let run = fork_window::mirror::open(label.as_deref(), "Fallout: New Vegas")
        .expect("a window under the display");
    let x = x_window(run.id()).expect("an X11 window (WINIT_UNIX_BACKEND=x11)");
    assert_eq!(
        probe.title(x).as_deref(),
        Some("gate14-mojave-ttw: Fallout: New Vegas")
    );
    assert_eq!(
        probe.wm_class(x),
        ["gate14-mojave-ttw", "gate14-mojave-ttw"]
    );
    let geometry = probe.geometry(x);
    assert_eq!((geometry.width, geometry.height), fork_window::mirror::SIZE);
    let until = Instant::now() + Duration::from_secs(5);
    while !probe
        .net_wm_state(x)
        .iter()
        .any(|s| s == "_NET_WM_STATE_HIDDEN")
    {
        assert!(
            Instant::now() < until,
            "the window did not become minimised"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_ne!(probe.focus(), x, "the window must not take the focus");
}
