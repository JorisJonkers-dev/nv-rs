// SPDX-License-Identifier: GPL-3.0-only
use super::*;
use crate::store::{Key, Override, Target};
use fork_catalogue::{covers, ids};

const PLAYER: u32 = 0x0300_0000;
const NPC: u32 = 0x0300_0100;
const FIRST: u32 = 0x0300_0200;
const THIRD: u32 = 0x0300_0300;
const NPC_ANIMATION: u32 = 0x0300_0400;
const NO_ACTOR: u32 = 0x0300_0500;
const PLAYER_ID: u32 = 0x14;
const NPC_ID: u32 = 0x0010_0abc;
const SPRINT: u32 = 0x0213;

/// An engine with the player (first- and third-person animations), an NPC with one animation, an
/// animation of no actor, and kNVSE installed as `on` says.
fn engine(on: bool) -> Engine {
    let mut e = Engine::new();
    e.map(0x011d_e000, 0x1000);
    e.map(0x0300_0000, 0x1000);
    e.set_global(PLAYER_SINGLETON, PLAYER);
    e.mem.set_u32(PLAYER + FORM_ID, PLAYER_ID);
    e.mem.set_u32(NPC + FORM_ID, NPC_ID);
    e.mem.set_u32(FIRST + ANIMATION_ACTOR, PLAYER);
    e.mem.set_u32(THIRD + ANIMATION_ACTOR, PLAYER);
    e.mem.set_u32(NPC_ANIMATION + ANIMATION_ACTOR, NPC);
    e.register(PLAYER_GET_ANIMATION, |_, a| Ret {
        eax: if a[1] == 1 { FIRST } else { THIRD },
        ..Ret::default()
    });
    crate::install_as(&mut e, on);
    e
}

fn ask(e: &mut Engine, animation: u32, group: u32) -> Option<u32> {
    e.site(0x0049_48c0, &[animation, group, 4])
}

fn enable(e: &mut Engine, target: Target, first_person: bool, owner: u32, sequence: u32) {
    let key = Key {
        target,
        first_person,
        group: SPRINT as u16,
    };
    let path = format!("mod{owner}\\sprint.kf").into_bytes();
    crate::enable(
        e,
        key,
        Override {
            owner,
            path,
            sequence,
        },
    );
}

/// Installed with an empty store, and not installed with overrides set, the seam answers nothing:
/// the game's own path.
#[test]
fn the_seam_answers_nothing_while_the_store_is_empty_or_the_plugin_is_off() {
    covers(&[
        ids::extenders::knvse::SETACTORANIMATIONPATH,
        ids::engine::functions::F004948C0,
    ]);
    let mut e = engine(true);
    assert_eq!(e.ext_free::<crate::Active>().count(), 0);
    for animation in [FIRST, THIRD, NPC_ANIMATION, NO_ACTOR] {
        assert_eq!(ask(&mut e, animation, SPRINT), None);
    }
    let mut e = engine(false);
    enable(&mut e, Target::Every, false, 1, 0x50);
    enable(&mut e, Target::Every, true, 1, 0x60);
    for animation in [FIRST, THIRD, NPC_ANIMATION] {
        assert_eq!(ask(&mut e, animation, SPRINT), None, "not installed");
    }
}

/// The player's first-person animation takes first-person overrides, its third-person one and an
/// NPC's take third-person ones, each for the actor set; another group answers nothing.
#[test]
fn the_seam_answers_the_override_for_the_actor_and_person() {
    covers(&[
        ids::extenders::knvse::SETACTORANIMATIONPATH,
        ids::engine::functions::F004948C0,
    ]);
    let mut e = engine(true);
    enable(&mut e, Target::Actor(PLAYER_ID), true, 1, 0x50);
    assert_eq!(ask(&mut e, FIRST, SPRINT), Some(0x50));
    assert_eq!(ask(&mut e, THIRD, SPRINT), None);
    assert_eq!(ask(&mut e, NPC_ANIMATION, SPRINT), None);
    assert_eq!(ask(&mut e, FIRST, SPRINT + 1), None, "another group");
    enable(&mut e, Target::Actor(NPC_ID), false, 1, 0x60);
    assert_eq!(ask(&mut e, NPC_ANIMATION, SPRINT), Some(0x60));
    assert_eq!(ask(&mut e, THIRD, SPRINT), None);
    enable(&mut e, Target::Every, false, 1, 0x70);
    assert_eq!(ask(&mut e, THIRD, SPRINT), Some(0x70));
    assert_eq!(
        ask(&mut e, NPC_ANIMATION, SPRINT),
        Some(0x60),
        "its own first"
    );
    assert_eq!(ask(&mut e, NO_ACTOR, SPRINT), None, "no actor");
}

/// Stacked overrides answer through the seam newest first and come back as they are disabled;
/// an override whose sequence is not loaded answers nothing; clearing empties the count.
#[test]
fn the_seam_follows_the_stack() {
    covers(&[
        ids::extenders::knvse::SETACTORANIMATIONPATH,
        ids::engine::functions::F004948C0,
    ]);
    let mut e = engine(true);
    enable(&mut e, Target::Actor(NPC_ID), false, 1, 0x50);
    enable(&mut e, Target::Actor(NPC_ID), false, 2, 0x60);
    assert_eq!(e.ext_free::<crate::Active>().count(), 2);
    assert_eq!(ask(&mut e, NPC_ANIMATION, SPRINT), Some(0x60));
    let key = Key {
        target: Target::Actor(NPC_ID),
        first_person: false,
        group: SPRINT as u16,
    };
    assert!(crate::disable(&mut e, key, 2, b"mod2\\sprint.kf"));
    assert_eq!(ask(&mut e, NPC_ANIMATION, SPRINT), Some(0x50));
    assert_eq!(e.ext_free::<crate::Active>().count(), 1);
    enable(&mut e, Target::Actor(NPC_ID), false, 3, 0);
    assert_eq!(ask(&mut e, NPC_ANIMATION, SPRINT), None, "not loaded");
    crate::clear(&mut e);
    assert_eq!(e.ext_free::<crate::Active>().count(), 0);
    assert_eq!(ask(&mut e, NPC_ANIMATION, SPRINT), None);
}

/// A second install does nothing (one handler at the seam).
#[test]
fn installing_twice_registers_the_seam_once() {
    covers(&[ids::extenders::knvse::SETACTORANIMATIONPATH]);
    let mut e = engine(true);
    crate::install_as(&mut e, true);
    assert_eq!(e.site_owners(0x0049_48c0), [Owner::Knvse]);
}
