// SPDX-License-Identifier: GPL-3.0-only
use super::*;

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

fn entry(owner: u32, path: &str, kf: u32) -> Override {
    Override {
        owner,
        path: path.as_bytes().to_vec(),
        kf,
        condition: 0,
        poll: false,
    }
}

#[test]
fn an_empty_store_answers_nothing() {
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
    let mut store = Store::default();
    store.enable(
        key(Target::Actor(ACTOR), false),
        entry(1, "a\\sprint.kf", 0x50),
    );
    assert_eq!(store.lookup(ACTOR, false, SPRINT).map(|o| o.kf), Some(0x50));
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
    let mut store = Store::default();
    let k = key(Target::Actor(ACTOR), true);
    store.enable(k, entry(1, "one\\sprint.kf", 0x50));
    store.enable(k, entry(2, "two\\sprint.kf", 0x60));
    assert_eq!(store.len(), 2);
    assert_eq!(store.lookup(ACTOR, true, SPRINT).map(|o| o.owner), Some(2));
    assert!(store.disable(k, 2, b"two\\sprint.kf").is_some());
    assert_eq!(store.lookup(ACTOR, true, SPRINT).map(|o| o.owner), Some(1));
    assert!(
        store.disable(k, 2, b"two\\sprint.kf").is_none(),
        "gone already"
    );
    assert!(store.disable(k, 1, b"one\\sprint.kf").is_some());
    assert!(store.is_empty());
}

/// Disabling the lower entry leaves the top one answering; another owner's disable of a path it
/// did not set removes nothing.
#[test]
fn disabling_takes_only_that_owners_entry_for_that_path() {
    let mut store = Store::default();
    let k = key(Target::Actor(ACTOR), false);
    store.enable(k, entry(1, "one\\sprint.kf", 0x50));
    store.enable(k, entry(2, "two\\sprint.kf", 0x60));
    assert!(store.disable(k, 3, b"one\\sprint.kf").is_none());
    assert!(store.disable(k, 1, b"two\\sprint.kf").is_none());
    assert!(store.disable(k, 1, b"one\\sprint.kf").is_some());
    assert_eq!(store.lookup(ACTOR, false, SPRINT).map(|o| o.owner), Some(2));
    assert_eq!(store.len(), 1);
}

/// Enabling the same mod's same path again moves it to the top (with the new model), and the
/// path is matched without case and with either slash.
#[test]
fn enabling_again_moves_the_entry_to_the_top() {
    let mut store = Store::default();
    let k = key(Target::Actor(ACTOR), false);
    store.enable(k, entry(1, "One/Sprint.KF", 0x50));
    store.enable(k, entry(2, "two\\sprint.kf", 0x60));
    store.enable(k, entry(1, "one\\sprint.kf", 0x70));
    assert_eq!(store.len(), 2);
    let top = store.lookup(ACTOR, false, SPRINT).unwrap();
    assert_eq!((top.owner, top.kf), (1, 0x70));
    assert_eq!(top.path, b"one\\sprint.kf");
    assert!(store.disable(k, 1, b"ONE/sprint.kf").is_some());
    assert_eq!(store.lookup(ACTOR, false, SPRINT).map(|o| o.owner), Some(2));
}

/// Every actor's override answers for any actor without one of its own; an actor's own comes
/// first.
#[test]
fn an_actors_own_override_comes_before_every_actors() {
    let mut store = Store::default();
    store.enable(key(Target::Every, false), entry(1, "all\\sprint.kf", 0x50));
    assert_eq!(store.lookup(OTHER, false, SPRINT).map(|o| o.kf), Some(0x50));
    store.enable(
        key(Target::Actor(ACTOR), false),
        entry(1, "me\\sprint.kf", 0x60),
    );
    assert_eq!(store.lookup(ACTOR, false, SPRINT).map(|o| o.kf), Some(0x60));
    assert_eq!(store.lookup(OTHER, false, SPRINT).map(|o| o.kf), Some(0x50));
    assert_eq!(store.lookup(ACTOR, true, SPRINT), None, "first person");
}

#[test]
fn clearing_empties_every_stack() {
    let mut store = Store::default();
    store.enable(key(Target::Every, false), entry(1, "a.kf", 0x50));
    store.enable(key(Target::Actor(ACTOR), true), entry(2, "b.kf", 0x60));
    let taken = store.clear();
    assert_eq!(taken.len(), 2);
    assert!(store.is_empty());
    assert_eq!(store.lookup(ACTOR, true, SPRINT), None);
}

/// Enabling the same mod's path again hands back the entry it replaced (so its model reference and
/// condition can be given back); a new path hands back nothing.
#[test]
fn enabling_again_hands_back_the_replaced_entry() {
    let mut store = Store::default();
    let k = key(Target::Actor(ACTOR), false);
    assert_eq!(store.enable(k, entry(1, "a\\sprint.kf", 0x50)), None);
    assert_eq!(
        store.enable(k, entry(2, "a\\sprint.kf", 0x60)),
        None,
        "another owner"
    );
    let old = store.enable(k, entry(1, "A/Sprint.kf", 0x70)).unwrap();
    assert_eq!((old.owner, old.kf), (1, 0x50));
    assert_eq!(store.len(), 2);
}

/// Disabling by path takes that owner's entries for the path off every group of that target and
/// person, and nothing else.
#[test]
fn disabling_a_path_takes_it_off_every_group_of_its_target_and_person() {
    let mut store = Store::default();
    let at = |target, first_person, group| Key {
        target,
        first_person,
        group,
    };
    let me = Target::Actor(ACTOR);
    store.enable(at(me, false, SPRINT), entry(1, "x.kf", 0x50));
    store.enable(at(me, false, SPRINT + 1), entry(1, "x.kf", 0x51));
    store.enable(at(me, true, SPRINT), entry(1, "x.kf", 0x52));
    store.enable(at(Target::Every, false, SPRINT), entry(1, "x.kf", 0x53));
    store.enable(at(me, false, SPRINT), entry(2, "x.kf", 0x54));
    let taken = store.disable_path(me, false, 1, b"X.KF");
    let mut kfs: Vec<u32> = taken.iter().map(|o| o.kf).collect();
    kfs.sort();
    assert_eq!(kfs, [0x50, 0x51]);
    assert_eq!(store.len(), 3);
    assert_eq!(store.lookup(ACTOR, false, SPRINT).map(|o| o.kf), Some(0x54));
    assert!(store.disable_path(me, false, 1, b"x.kf").is_empty());
}

/// The candidates come in the order the seam asks them: the actor's own stack from the top, then
/// every actor's from the top.
#[test]
fn candidates_are_the_actors_stack_then_every_actors_newest_first() {
    let mut store = Store::default();
    store.enable(key(Target::Every, false), entry(1, "e1.kf", 0x10));
    store.enable(key(Target::Actor(ACTOR), false), entry(1, "a1.kf", 0x20));
    store.enable(key(Target::Every, false), entry(2, "e2.kf", 0x30));
    store.enable(key(Target::Actor(ACTOR), false), entry(2, "a2.kf", 0x40));
    let order: Vec<u32> = store
        .candidates(ACTOR, false, SPRINT)
        .map(|o| o.kf)
        .collect();
    assert_eq!(order, [0x40, 0x20, 0x30, 0x10]);
    let other: Vec<u32> = store
        .candidates(OTHER, false, SPRINT)
        .map(|o| o.kf)
        .collect();
    assert_eq!(other, [0x30, 0x10]);
    assert_eq!(store.candidates(ACTOR, true, SPRINT).count(), 0);
}
