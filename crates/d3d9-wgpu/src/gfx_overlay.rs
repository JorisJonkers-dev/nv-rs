// SPDX-License-Identifier: GPL-3.0-only
//! Fork-only. The in-game graphics overlay (nv-rs#592, ADR-0019): the panel of `fork_overlay`,
//! drawn with the port's own glyphs as its font.

/// The port's glyphs of printable ASCII (the F11 counter's), the overlay's font.
pub fn glyphs() -> &'static [[u8; 7]; 95] {
    crate::bitmap_font::ascii()
}

/// The overlay's drawing for frames of `format`, its font made from [`glyphs`].
pub fn painter(device: &wgpu::Device, format: wgpu::TextureFormat) -> fork_overlay::Painter {
    fork_overlay::Painter::new(device, format, glyphs())
}

/// Runs a wgpu future to its end on this thread (tests that make their own device).
#[doc(hidden)]
pub fn block_on<F: std::future::Future>(f: F) -> F::Output {
    crate::device::block_on(f)
}
