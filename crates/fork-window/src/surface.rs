//! The wgpu surface of a window of this layer.

/// The surface of window `id` ([`crate::Window::id`]) on `instance`: Wayland and X11 (Xlib or
/// XCB) from the raw handles of the registry. What the device layer's `surface_for` returns off
/// Windows.
pub fn surface_for(instance: &wgpu::Instance, id: isize) -> Result<wgpu::Surface<'static>, String> {
    let (display, window) = crate::raw_handles(id).ok_or("no window")?;
    // SAFETY: the window thread keeps the window alive until `Window::shutdown`, which the
    // caller calls after dropping every surface (the game's device lives as long as its window).
    unsafe {
        instance
            .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(display),
                raw_window_handle: window,
            })
            .map_err(|e| format!("create_surface: {e}"))
    }
}
