// SPDX-License-Identifier: GPL-3.0-only
//! The lens of a place: field of view, far plane and fog (nv-rs#479). The game derives all three each frame from
//! settings, the player's own fields and the weather, so a write from the runner is undone before the frame is drawn.
//! These are set at their sources instead: the runner records what the place says here and three registrations take
//! it into the game's own functions.
//!
//! - the field of view: `PlayerCharacter::fWorldFOV` (`+0x670`) is the world camera's angle; `0095de30` moves it each
//!   frame toward the setting `fDefaultWorldFOV`, so the registration runs it and then writes the place's value;
//! - the far plane: `SceneGraph::GetFarDistance` (`008786b0`; in a world with terrain LOD it is one fixed number, not the
//!   `fNoLODFarDistance` settings) is read by `SetCameraFOV`, by the sky's fog update
//!   and by the atmosphere's far-plane update; it answers the place's value;
//! - the fog: the sky's fog distances (`+0xd4`, `+0xd8`) are recomputed each frame from the weather and then read
//!   through `00923000` (a folded accessor other classes share: only the sky's is touched); that reader pushes them out
//!   of reach first.

use engine::prelude::*;
use engine::units::fallout_ai::middlehighprocess::fn_00923000;
use engine::units::fallout_ai::playercharacter_p3::fn_0095de30;
use engine::units::fallout_misc::scenegraph::scene_graph_get_far_distance;
use fork_scene::Place;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// `SceneGraph::GetFarDistance`: the game's scene graph's own (`BSSceneGraph`'s is only for menu scene graphs).
const GET_FAR_DISTANCE: u32 = 0x0087_86b0;
/// `PlayerCharacter::UpdateFOV` (the per-frame move of the two fields of view).
const UPDATE_FOV: u32 = 0x0095_de30;
/// The sky's fog distances accessor (`this + 0xd4`).
const SKY_FOG_DISTANCES: u32 = 0x0092_3000;
const PLAYER: u32 = 0x011d_ea3c;
const SKY: u32 = 0x011c_cb78;
const WORLD_FOV: u32 = 0x670;
const FIRST_PERSON_FOV: u32 = 0x674;
/// The sky's fog near and far.
const FOG_NEAR: u32 = 0xd4;
const FOG_FAR: u32 = 0xd8;
/// Out of reach of any picture.
const NO_FOG_NEAR: f32 = 1.0e8;
const NO_FOG_FAR: f32 = 2.0e8;

/// 0 = the game's own.
static FOV: AtomicU32 = AtomicU32::new(0);
static FAR: AtomicU32 = AtomicU32::new(0);
static SAID_FAR: AtomicBool = AtomicBool::new(false);
static FOG_OFF: AtomicBool = AtomicBool::new(false);

/// Takes the lens of `place` (the game's own for what the place does not say).
pub fn set(place: &Place) {
    FOV.store(
        place.fov.map_or(0, |v| (v as f32).to_bits()),
        Ordering::Relaxed,
    );
    FAR.store(
        place.far.map_or(0, |v| (v as f32).to_bits()),
        Ordering::Relaxed,
    );
    FOG_OFF.store(!place.fog, Ordering::Relaxed);
}

fn fov() -> Option<f32> {
    Some(FOV.load(Ordering::Relaxed))
        .filter(|b| *b != 0)
        .map(f32::from_bits)
}

fn far() -> Option<f32> {
    Some(FAR.load(Ordering::Relaxed))
        .filter(|b| *b != 0)
        .map(f32::from_bits)
}

/// Writes the field of view into the player (the runner calls it each frame, the game's own update runs only when
/// the player is simulated).
pub fn write_fov(e: &mut Engine) {
    let player = e.mem.u32(PLAYER);
    if let (Some(fov), true) = (fov(), player != 0) {
        e.mem.set_f32(player + WORLD_FOV, fov);
        e.mem.set_f32(player + FIRST_PERSON_FOV, fov);
    }
}

/// Pushes the sky's fog out of reach when the place says no fog.
pub fn write_fog(e: &mut Engine) {
    let sky = e.mem.u32(SKY);
    if FOG_OFF.load(Ordering::Relaxed) && sky != 0 {
        e.mem.set_f32(sky + FOG_NEAR, NO_FOG_NEAR);
        e.mem.set_f32(sky + FOG_FAR, NO_FOG_FAR);
    }
}

/// The registrations; once, with the runner's.
pub fn install(e: &mut Engine) {
    e.register(GET_FAR_DISTANCE, |e, a| match far() {
        Some(v) => {
            fork_log::debug!(target: "scene", "far distance {v}");
            Ret {
                st0: f64::from(v),
                ..Ret::default()
            }
        }
        None => {
            let v = scene_graph_get_far_distance(e, Ptr::new(a[0]));
            if !SAID_FAR.swap(true, Ordering::Relaxed) {
                fork_log::info!(target: "scene", "the game's own far distance is {v}");
            }
            Ret {
                st0: f64::from(v),
                ..Ret::default()
            }
        }
    });
    e.register(UPDATE_FOV, |e, a| {
        fn_0095de30(e, Ptr::new(a[0]), f32::from_bits(a[1]));
        write_fov(e);
        Ret::default()
    });
    e.register(SKY_FOG_DISTANCES, |e, a| {
        // The accessor is a folded one: other classes with a member at +0xd4 (the middle-high process) share it.
        if FOG_OFF.load(Ordering::Relaxed) && a[0] != 0 && a[0] == e.mem.u32(SKY) {
            e.mem.set_f32(a[0] + FOG_NEAR, NO_FOG_NEAR);
            e.mem.set_f32(a[0] + FOG_FAR, NO_FOG_FAR);
        }
        Ret {
            eax: fn_00923000(e, a[0]),
            ..Ret::default()
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use fork_scene::PlaceFile;
    use std::sync::Mutex;

    /// The lens is process-wide state (the game has one): the tests take turns.
    static TURN: Mutex<()> = Mutex::new(());

    fn place(lens: &str) -> Place {
        PlaceFile::parse(&format!(
            "version = 1\n[[place]]\nname = \"a\"\nworld = \"W\"\nx = 1\ny = 2\nz = 3\n{lens}\n"
        ))
        .unwrap()
        .places
        .remove(0)
    }

    #[test]
    fn the_far_distance_is_the_places_or_the_games_own() {
        let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
        let mut e = Engine::new();
        install(&mut e);
        e.map(0x0108_3000, 0x1000);
        e.mem.set_f32(0x0108_3c64, 600.0);
        set(&place("far = 1234"));
        assert_eq!(e.call(GET_FAR_DISTANCE, &args![0u32]).f32(), 1234.0);
        // A place without `far` hands the question to the game's own function (the fixed flag at +0xb8 answers
        // the number at 01083c64).
        set(&place(""));
        let graph = e.mem.alloc(0xc0);
        e.mem.set_u8(graph + 0xb8, 1);
        assert_eq!(e.call(GET_FAR_DISTANCE, &args![graph]).f32(), 600.0);
    }

    #[test]
    fn no_fog_pushes_the_distances_out_of_reach_and_fog_leaves_them() {
        let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
        let mut e = Engine::new();
        install(&mut e);
        e.map(SKY & !0xfff, 0x1000);
        let sky = e.mem.alloc(0x100);
        e.mem.set_u32(SKY, sky);
        let other = e.mem.alloc(0x100);
        e.mem.set_f32(other + FOG_NEAR, 7.0);
        e.mem.set_f32(sky + FOG_NEAR, 100.0);
        e.mem.set_f32(sky + FOG_FAR, 5000.0);
        set(&place(""));
        assert_eq!(e.call(SKY_FOG_DISTANCES, &args![sky]).u32(), sky + FOG_NEAR);
        assert_eq!(e.mem.f32(sky + FOG_FAR), 5000.0);
        set(&place("fog = false"));
        // Another object's member is only answered.
        assert_eq!(
            e.call(SKY_FOG_DISTANCES, &args![other]).u32(),
            other + FOG_NEAR
        );
        assert_eq!(e.mem.f32(other + FOG_NEAR), 7.0);
        assert_eq!(e.call(SKY_FOG_DISTANCES, &args![sky]).u32(), sky + FOG_NEAR);
        assert_eq!(e.mem.f32(sky + FOG_NEAR), NO_FOG_NEAR);
        assert_eq!(e.mem.f32(sky + FOG_FAR), NO_FOG_FAR);
        set(&place(""));
    }

    #[test]
    fn the_field_of_view_is_written_into_the_players_world_and_first_person_fields() {
        let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
        let mut e = Engine::new();
        e.map(PLAYER & !0xfff, 0x1000);
        let player = e.mem.alloc(0x900);
        e.mem.set_u32(PLAYER, player);
        e.mem.set_f32(player + WORLD_FOV, 75.0);
        set(&place(""));
        write_fov(&mut e);
        assert_eq!(e.mem.f32(player + WORLD_FOV), 75.0);
        set(&place("fov = 20"));
        write_fov(&mut e);
        assert_eq!(e.mem.f32(player + WORLD_FOV), 20.0);
        assert_eq!(e.mem.f32(player + FIRST_PERSON_FOV), 20.0);
        set(&place(""));
    }
}
