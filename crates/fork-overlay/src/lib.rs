// SPDX-License-Identifier: GPL-3.0-only
//! The in-game graphics overlay (nv-rs#592, nv-rs#730; ADR-0019, "The overlay"): an egui panel
//! generated from the graphics modules' schemas (`fork_gfx`), drawn by egui-wgpu into the window's
//! frame at the present, after the window's capture is taken.
//!
//! - [`font`]: the panel's font, a TrueType file made from the port's own bitmap glyphs (no font
//!   file, no licence beyond the engine's).
//! - [`painter`]: the egui context with the overlay's look, and the renderer.
//! - [`panel`]: the panel itself and the changes it hands back for the profile's file.

pub mod font;
pub mod painter;
pub mod panel;

/// The toolkit, for the caller's input events and tests.
pub use egui;
pub use painter::{Painter, Stats};
pub use panel::{Change, Panel, View};
