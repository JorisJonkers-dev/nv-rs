// SPDX-License-Identifier: GPL-3.0-only
//! How far lamps light their surroundings (nv-rs#827, a fork setting: `light.lamp_distance`). The game fades
//! a point light out between `fLightLODStartFade` and that plus `fLightLODRange` from the camera (the
//! Options menu's light fade slider) and drops it beyond; the scale multiplies both distances. 1 (the
//! default) leaves the game's distances bit for bit; the shadow fade distances are not touched.
//!
//! Here, in the core, because both the translation of `ShadowSceneLight::TestFrustumCull` (`engine-bs`) and its
//! native version (`engine-typed`) read it.

use std::sync::atomic::{AtomicU32, Ordering};

/// The scale's `f32` bits; 1.0 by default.
static SCALE: AtomicU32 = AtomicU32::new(0x3f80_0000);

/// Sets the scale from the next frame on: clamped to 1 to 16, 1 for anything not finite.
pub fn set(scale: f32) {
    let scale = if scale.is_finite() {
        scale.clamp(1.0, 16.0)
    } else {
        1.0
    };
    SCALE.store(scale.to_bits(), Ordering::Relaxed);
}

/// The scale now.
pub fn get() -> f32 {
    f32::from_bits(SCALE.load(Ordering::Relaxed))
}

/// `distance` times the scale; `distance` itself (the same bits) at 1.
pub fn scaled(distance: f64) -> f64 {
    let scale = get();
    if scale == 1.0 {
        distance
    } else {
        f64::from(distance as f32 * scale)
    }
}

/// The `f32` bits of a distance (`bits`) times the scale: the same bits at 1. The same value as [`scaled`] of
/// the distance read from `bits`.
pub fn scaled_bits(bits: u32) -> u32 {
    let scale = get();
    if scale == 1.0 {
        bits
    } else {
        (f32::from_bits(bits) * scale).to_bits()
    }
}
