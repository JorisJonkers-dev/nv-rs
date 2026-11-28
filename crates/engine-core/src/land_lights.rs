// SPDX-License-Identifier: GPL-3.0-only
//! More than three lights on a land quad (nv-rs#912, a fork setting: `light.land_lights`). The exe draws a land
//! quad with one lit pass (`LANDnO4`) that holds the sun and three point lights (`AddLandscapePasses_2x`,
//! `00bdf3e0`), so lamps and fires past the third leave the ground they stand on dark. With the setting on, the
//! lights past the third are drawn in further passes of the same land type, added to the picture, each holding
//! three more of them; those passes carry only their point lights (their ambient, sun and fog terms, which the
//! shader adds in every pass, are set to zero for them). Off, nothing here runs and the game's passes are
//! its own.

use std::sync::atomic::{AtomicBool, Ordering};

/// The most extra passes one land quad gets (three lights each, so the lit lights from the fourth to the
/// twelfth).
pub const EXTRA_PASSES: usize = 3;

/// The first and last land pass types (`LAND1O` to `LAND7O4_SimpleShp`).
pub const FIRST_TYPE: u32 = 0x1f7;
pub const LAST_TYPE: u32 = 0x22e;

static ON: AtomicBool = AtomicBool::new(false);

/// Turns the extra passes on or off from the next frame on.
pub fn set(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

/// Whether the extra passes are on.
pub fn wanted() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Whether `pass_type` is a land pass.
pub fn is_land(pass_type: u32) -> bool {
    pass_type.wrapping_sub(FIRST_TYPE) <= LAST_TYPE - FIRST_TYPE
}

/// How many lit lights `AddLandscapePasses_2x` looks at: the exe's three, or with the extra passes on all
/// that they can hold.
pub fn looked_at() -> usize {
    if wanted() {
        3 + 3 * EXTRA_PASSES
    } else {
        3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn land_types_are_the_fifty_six_of_the_exe() {
        assert!(is_land(0x1f7) && is_land(0x22e));
        assert!(!is_land(0x1f6) && !is_land(0x22f) && !is_land(0));
    }

    #[test]
    fn off_looks_at_three() {
        set(false);
        assert_eq!(looked_at(), 3);
        set(true);
        assert_eq!(looked_at(), 12);
        set(false);
    }
}
