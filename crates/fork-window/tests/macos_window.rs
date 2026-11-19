//! macOS: the window layer on the program's real main thread, as the play build runs it
//! (`run_on_main_thread`). The game's thread opens a window, which must have AppKit handles, a
//! `CAMetalLayer` and a client size, closes it, opens a second one on the same loop (winit makes
//! one loop per process), and ends the run with 0. Any failure ends it with 1 and the reason.
//!
//! No test harness (`harness = false`): the loop needs the process's real main thread. Elsewhere
//! the test does nothing.

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
fn main() {
    fork_window::run_on_main_thread(Box::new(game), end)
}

#[cfg(target_os = "macos")]
fn end(code: i32) -> ! {
    std::process::exit(code)
}

#[cfg(target_os = "macos")]
fn fail(why: &str) -> ! {
    eprintln!("FAILED: {why}");
    end(1)
}

#[cfg(target_os = "macos")]
fn game() {
    use fork_window::{Window, WindowRequest};
    use raw_window_handle::RawWindowHandle;

    let request = || WindowRequest {
        title: "fork-window macOS test".into(),
        width: 320,
        height: 200,
        ..WindowRequest::default()
    };
    let first =
        Window::open(request(), None).unwrap_or_else(|e| fail(&format!("first window: {e}")));
    let id = first.id();
    match fork_window::raw_handles(id) {
        Some((_, RawWindowHandle::AppKit(_))) => {}
        other => fail(&format!("not an AppKit window: {other:?}")),
    }
    if fork_window::metal_layer(id).is_none() {
        fail("the window has no Metal layer");
    }
    let (w, h) = fork_window::client_size(id);
    if w == 0 || h == 0 {
        fail(&format!("no client size ({w}x{h})"));
    }
    drop(first);
    if fork_window::raw_handles(id).is_some() || fork_window::metal_layer(id).is_some() {
        fail("the closed window is still registered");
    }
    let second =
        Window::open(request(), None).unwrap_or_else(|e| fail(&format!("second window: {e}")));
    if fork_window::metal_layer(second.id()).is_none() {
        fail("the second window has no Metal layer");
    }
    println!("ok: two windows of {w}x{h} px with Metal layers on the main thread's loop");
}
