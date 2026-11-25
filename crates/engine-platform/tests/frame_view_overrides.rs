// SPDX-License-Identifier: GPL-3.0-only
//! The shader overrides' switch (`want_frame_view`) puts the platform's hook where the world is
//! drawn (`engine_core::world_drawn`, which records the frame's camera) and nothing else: not the
//! stand-in of a skipped depth-only clear, and not the new lighting itself. Its own test binary,
//! so the run-wide hooks start empty.

use engine_core::world_drawn::{depth_clear_skipped_installed, world_drawn_installed};

// A program of its own: no engine crates above the core (it makes no engine).
engine_core::assemble!();

#[test]
fn the_overrides_record_the_camera_where_the_world_is_drawn_only() {
    assert!(!world_drawn_installed());
    engine_platform::d3d9real::want_frame_view(|_| None);
    assert!(world_drawn_installed());
    assert!(!depth_clear_skipped_installed());
    assert!(!d3d9_wgpu::scene_light::enabled());
}
