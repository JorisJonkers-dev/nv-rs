// SPDX-License-Identifier: GPL-3.0-only
use super::*;
use fork_catalogue::{covers, ids};

const ACTOR: u32 = 0x0010_0abc;
const OTHER: u32 = 0x0010_0def;
const SPRINT: u16 = 0x0213;

fn key(target: Target, first_person: bool) -> Key {
    Key {
        target,
        first_person,
        group: SPRINT,
    }
}

fn entry(owner: u32, path: &str, sequence: u32) -> Override {
    Override {
        owner,
        path: path.as_bytes().to_vec(),
        sequence,
    }
}

#[test]
fn an_empty_store_answers_nothing() {
    covers(&[ids::extenders::knvse::SETACTORANIMATIONPATH]);
    let store = Store::default();
    assert!(store.is_empty());
    assert_eq!(store.len(), 0);
    for first_person in [false, true] {
        assert_eq!(store.lookup(ACTOR, first_person, SPRINT), None);
    }
}

/// An override answers for its actor, person and group only.
#[test]
fn an_override_answers_for_its_actor_person_and_group_only() {
    covers(&[ids::extenders::knvse::SETACTORANIMATIONPATH]);
    let mut store = Store::default();
    store.enable(
        key(Target::Actor(ACTOR), false),
        entry(1, "a\\sprint.kf", 0x50),
    );
    assert_eq!(
        store.lookup(ACTOR, false, SPRINT).map(|o| o.sequence),
        Some(0x50)
    );
    assert_eq!(store.lookup(ACTOR, true, SPRINT), None, "first person");
    assert_eq!(store.lookup(OTHER, false, SPRINT), None, "another actor");
    assert_eq!(
        store.lookup(ACTOR, false, SPRINT + 1),
        None,
        "another group"
    );
}

/// Two mods on one stack: the newer answers; disabling it brings the older back; disabling the
/// older empties the stack.
#[test]
fn overrides_stack_and_unstack() {
    covers(&[ids::extenders::knvse::SETACTORANIMATIONPATH]);
    let mut store = Store::default();
    let k = key(Target::Actor(ACTOR), true);
    store.enable(k, entry(1, "one\\sprint.kf", 0x50));
    store.enable(k, entry(2, "two\\sprint.kf", 0x60));
    assert_eq!(store.len(), 2);
    assert_eq!(store.lookup(ACTOR, true, SPRINT).map(|o| o.owner), Some(2));
    assert!(store.disable(k, 2, b"two\\sprint.kf"));
    assert_eq!(store.lookup(ACTOR, true, SPRINT).map(|o| o.owner), Some(1));
    assert!(!store.disable(k, 2, b"two\\sprint.kf"), "gone already");
    assert!(store.disable(k, 1, b"one\\sprint.kf"));
    assert!(store.is_empty());
}

/// Disabling the lower entry leaves the top one answering; another owner's disable of a path it
/// did not set removes nothing.
#[test]
fn disabling_takes_only_that_owners_entry_for_that_path() {
    covers(&[ids::extenders::knvse::SETACTORANIMATIONPATH]);
    let mut store = Store::default();
    let k = key(Target::Actor(ACTOR), false);
    store.enable(k, entry(1, "one\\sprint.kf", 0x50));
    store.enable(k, entry(2, "two\\sprint.kf", 0x60));
    assert!(!store.disable(k, 3, b"one\\sprint.kf"));
    assert!(!store.disable(k, 1, b"two\\sprint.kf"));
    assert!(store.disable(k, 1, b"one\\sprint.kf"));
    assert_eq!(store.lookup(ACTOR, false, SPRINT).map(|o| o.owner), Some(2));
    assert_eq!(store.len(), 1);
}

/// Enabling the same mod's same path again moves it to the top (with the new sequence), and the
/// path is matched without case and with either slash.
#[test]
fn enabling_again_moves_the_entry_to_the_top() {
    covers(&[ids::extenders::knvse::SETACTORANIMATIONPATH]);
    let mut store = Store::default();
    let k = key(Target::Actor(ACTOR), false);
    store.enable(k, entry(1, "One/Sprint.KF", 0x50));
    store.enable(k, entry(2, "two\\sprint.kf", 0x60));
    store.enable(k, entry(1, "one\\sprint.kf", 0x70));
    assert_eq!(store.len(), 2);
    let top = store.lookup(ACTOR, false, SPRINT).unwrap();
    assert_eq!((top.owner, top.sequence), (1, 0x70));
    assert_eq!(top.path, b"one\\sprint.kf");
    assert!(store.disable(k, 1, b"ONE/sprint.kf"));
    assert_eq!(store.lookup(ACTOR, false, SPRINT).map(|o| o.owner), Some(2));
}

/// Every actor's override answers for any actor without one of its own; an actor's own comes
/// first.
#[test]
fn an_actors_own_override_comes_before_every_actors() {
    covers(&[ids::extenders::knvse::SETACTORANIMATIONPATH]);
    let mut store = Store::default();
    store.enable(key(Target::Every, false), entry(1, "all\\sprint.kf", 0x50));
    assert_eq!(
        store.lookup(OTHER, false, SPRINT).map(|o| o.sequence),
        Some(0x50)
    );
    store.enable(
        key(Target::Actor(ACTOR), false),
        entry(1, "me\\sprint.kf", 0x60),
    );
    assert_eq!(
        store.lookup(ACTOR, false, SPRINT).map(|o| o.sequence),
        Some(0x60)
    );
    assert_eq!(
        store.lookup(OTHER, false, SPRINT).map(|o| o.sequence),
        Some(0x50)
    );
    assert_eq!(store.lookup(ACTOR, true, SPRINT), None, "first person");
}

#[test]
fn clearing_empties_every_stack() {
    covers(&[ids::extenders::knvse::SETACTORANIMATIONPATH]);
    let mut store = Store::default();
    store.enable(key(Target::Every, false), entry(1, "a.kf", 0x50));
    store.enable(key(Target::Actor(ACTOR), true), entry(2, "b.kf", 0x60));
    store.clear();
    assert!(store.is_empty());
    assert_eq!(store.lookup(ACTOR, true, SPRINT), None);
}
