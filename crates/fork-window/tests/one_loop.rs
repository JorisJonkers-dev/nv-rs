//! The monitor query and the windows share the process's one event loop (winit refuses a
//! second: "EventLoop can't be recreated", which left the game with no window). Needs a display,
//! so it runs where `NV_WINDOW_SMOKE=1` (the Xvfb job) and skips elsewhere.

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn a_monitor_query_then_windows_in_one_process() {
    if std::env::var_os("NV_WINDOW_SMOKE").is_none() {
        eprintln!("skipped: NV_WINDOW_SMOKE is not set");
        return;
    }
    let display = fork_window::display();
    let first = fork_window::Window::open(fork_window::WindowRequest::default(), None)
        .expect("a window after the monitor query");
    let id = first.id();
    assert!(id != 0 && fork_window::raw_handles(id).is_some());
    drop(first);
    assert!(fork_window::raw_handles(id).is_none());
    // The loop outlives a window: another can be made after it.
    let second = fork_window::Window::open(fork_window::WindowRequest::default(), None)
        .expect("a second window on the same loop");
    assert!(fork_window::raw_handles(second.id()).is_some());
    if let Some(d) = display {
        assert!(d.monitor.width > 0 && d.monitor.height > 0);
        assert_eq!(fork_window::display(), Some(d));
    }
}
