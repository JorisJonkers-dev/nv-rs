// SPDX-License-Identifier: GPL-3.0-only
//! `SetActorAnimationPath` through its record: what it puts in the store and takes off, for an
//! actor and for every actor, with and without a condition, and what it answers.

use super::*;
use crate::test_support::{
    call, engine, kf_file, n, r, refs, s, set_script_mod, set_script_ref, WORLD,
};
use fork_catalogue::{covers, ids};

const ACTOR: u32 = 0x0303_9000;
const ACTOR_ID: u32 = 0x0010_0abc;
const LAMBDA: u32 = 0x0303_c000;
const SPRINT: u16 = 0x0213;
const RUN: u16 = 0x0214;

fn setup() -> Engine {
    let mut e = engine();
    e.mem.set_u32(ACTOR + FORM_ID, ACTOR_ID);
    set_script_mod(&mut e, 7);
    e
}

fn set(e: &mut Engine, this: u32, args: &[Vec<u8>]) -> f64 {
    call(e, SET_ACTOR_ANIMATION_PATH, this, args).1
}

fn state(e: &mut Engine) -> &crate::State {
    e.ext::<crate::State>()
}

/// The record: the plugin's name at its opcode, its six parameters (three optional), no calling
/// reference needed.
#[test]
fn the_command_is_registered_at_its_opcode_with_its_parameters() {
    let e = engine();
    let record = nvse::table::record_of(&e, SET_ACTOR_ANIMATION_PATH);
    assert_ne!(record, 0);
    assert_eq!(e.mem.cstr(e.mem.u32(record)), b"SetActorAnimationPath");
    let params = e.mem.u32(record + 0x14);
    let kinds: Vec<(u32, u32)> = (0..6)
        .map(|i| {
            (
                e.mem.u32(params + 12 * i + 4),
                e.mem.u32(params + 12 * i + 8),
            )
        })
        .collect();
    assert_eq!(
        kinds,
        [
            (INT, 0),
            (INT, 0),
            (STRING, 0),
            (INT, 1),
            (ANY_FORM, 1),
            (INT, 1)
        ]
    );
    assert_eq!(COMMAND_NAMES, ["SetActorAnimationPath"]);
}

/// On an actor, enabled: the file is loaded and its model put on that actor's stack for the
/// group the file names, owned by the calling script's mod; it answers 1. Without a reference it
/// is for every actor; with first person 1 for the player's first-person animation.
#[test]
fn enabling_puts_the_files_model_on_the_stack_of_its_group() {
    covers(&[ids::extenders::knvse::SETACTORANIMATIONPATH]);
    let mut e = setup();
    let sprint = kf_file(&mut e, 1, "characters\\_male\\sprint.kf", SPRINT);
    let run = kf_file(&mut e, 2, "characters\\_male\\run.kf", RUN);
    assert_eq!(
        set(
            &mut e,
            ACTOR,
            &[n(0), n(1), s("Characters/_Male/Sprint.KF")]
        ),
        1.0
    );
    let o = state(&mut e)
        .store
        .lookup(ACTOR_ID, false, SPRINT)
        .unwrap()
        .clone();
    assert_eq!((o.owner, o.kf, o.condition, o.poll), (7, sprint, 0, false));
    assert_eq!(o.path, b"characters\\_male\\sprint.kf");
    assert_eq!(refs(&e, sprint), 1, "the load's reference");
    assert_eq!(
        set(&mut e, 0, &[n(1), n(1), s("characters\\_male\\run.kf")]),
        1.0
    );
    let every = state(&mut e).store.lookup(0x14, true, RUN).map(|o| o.kf);
    assert_eq!(every, Some(run));
    assert_eq!(
        state(&mut e).store.lookup(ACTOR_ID, false, RUN),
        None,
        "third person"
    );
    assert_eq!(e.ext_free::<crate::Active>().count(), 2);
}

/// Disabled: the calling mod's override of that path comes off (its reference given back) and it
/// answers 1; nothing to take off answers 0.
#[test]
fn disabling_takes_the_override_off_again() {
    covers(&[ids::extenders::knvse::SETACTORANIMATIONPATH]);
    let mut e = setup();
    let sprint = kf_file(&mut e, 1, "a\\sprint.kf", SPRINT);
    set(&mut e, ACTOR, &[n(0), n(1), s("a\\sprint.kf")]);
    assert_eq!(
        set(&mut e, 0, &[n(0), n(0), s("a\\sprint.kf")]),
        0.0,
        "not every actor's"
    );
    assert_eq!(
        set(&mut e, ACTOR, &[n(1), n(0), s("a\\sprint.kf")]),
        0.0,
        "not first person"
    );
    assert_eq!(set(&mut e, ACTOR, &[n(0), n(0), s("A/Sprint.kf")]), 1.0);
    assert!(state(&mut e).store.is_empty());
    assert_eq!(refs(&e, sprint), 0);
    assert_eq!(set(&mut e, ACTOR, &[n(0), n(0), s("a\\sprint.kf")]), 0.0);
}

/// Enabling the same path again keeps one entry (the old reference given back); a second mod's
/// override of the same group stacks on top and its removal brings the first back.
#[test]
fn the_same_path_again_replaces_and_another_mod_stacks() {
    covers(&[ids::extenders::knvse::SETACTORANIMATIONPATH]);
    let mut e = setup();
    let one = kf_file(&mut e, 1, "one\\sprint.kf", SPRINT);
    let two = kf_file(&mut e, 2, "two\\sprint.kf", SPRINT);
    set(&mut e, ACTOR, &[n(0), n(1), s("one\\sprint.kf")]);
    set(&mut e, ACTOR, &[n(0), n(1), s("one\\sprint.kf")]);
    assert_eq!(state(&mut e).store.len(), 1);
    assert_eq!(refs(&e, one), 1);
    set_script_mod(&mut e, 8);
    set(&mut e, ACTOR, &[n(0), n(1), s("two\\sprint.kf")]);
    assert_eq!(
        state(&mut e)
            .store
            .lookup(ACTOR_ID, false, SPRINT)
            .map(|o| o.kf),
        Some(two)
    );
    set(&mut e, ACTOR, &[n(0), n(0), s("two\\sprint.kf")]);
    assert_eq!(
        state(&mut e)
            .store
            .lookup(ACTOR_ID, false, SPRINT)
            .map(|o| o.kf),
        Some(one)
    );
}

/// The poll flag and the condition function are kept with the override, and the condition's
/// variables are saved while the store holds it and given back when it comes off.
#[test]
fn a_condition_and_its_poll_flag_are_kept_with_the_override() {
    covers(&[ids::extenders::knvse::SETACTORANIMATIONPATH]);
    let mut e = setup();
    kf_file(&mut e, 1, "a\\sprint.kf", SPRINT);
    set_script_ref(&mut e, 1, LAMBDA);
    assert_eq!(
        set(&mut e, 0, &[n(0), n(1), s("a\\sprint.kf"), n(1), r(1)]),
        1.0
    );
    let o = state(&mut e)
        .store
        .lookup(ACTOR_ID, false, SPRINT)
        .unwrap()
        .clone();
    assert_eq!((o.condition, o.poll), (LAMBDA, true));
    assert_eq!(crate::SAVED.with(|s| s.take()), [(LAMBDA, true)]);
    set(&mut e, 0, &[n(0), n(0), s("a\\sprint.kf")]);
    assert_eq!(crate::SAVED.with(|s| s.take()), [(LAMBDA, false)]);
}

/// A file the loader does not find, a model without a group, a folder with no `.kf` file in it
/// and a path too long for the game's buffers set nothing and answer 0; no reference is left
/// behind.
#[test]
fn what_cannot_be_loaded_sets_nothing() {
    covers(&[ids::extenders::knvse::SETACTORANIMATIONPATH]);
    let mut e = setup();
    let groupless = kf_file(&mut e, 1, "a\\nogroup.kf", 0);
    e.mem.set_u32(groupless + 8, 0);
    let long = format!("{}.kf", "x".repeat(0x110));
    for path in [
        "a\\missing.kf",
        "a\\nogroup.kf",
        "characters\\_male\\attacking",
        &long,
    ] {
        assert_eq!(set(&mut e, ACTOR, &[n(0), n(1), s(path)]), 0.0, "{path}");
    }
    assert!(state(&mut e).store.is_empty());
    assert_eq!(refs(&e, groupless), 0);
}

/// Through the command and the seam together: the override set by the script is the sequence
/// `Animation::StartGroup` gets for that actor's group.
#[test]
fn an_override_set_by_the_command_answers_at_the_seam() {
    covers(&[ids::extenders::knvse::SETACTORANIMATIONPATH]);
    let mut e = setup();
    let kf = kf_file(&mut e, 1, "a\\sprint.kf", SPRINT);
    let animation = 0x0303_a000;
    e.mem.set_u32(animation + 4, ACTOR);
    e.mem.set_u32(animation + 0xd8, 0x0303_b000);
    assert_eq!(
        e.site(0x0049_48c0, &[animation, u32::from(SPRINT), 4]),
        None
    );
    assert_eq!(set(&mut e, ACTOR, &[n(0), n(1), s("a\\sprint.kf")]), 1.0);
    let sequence = e.site(0x0049_48c0, &[animation, u32::from(SPRINT), 4]);
    assert!(sequence.is_some());
    assert_eq!(refs(&e, kf), 2, "the store's and the kept sequence's");
}

/// A folder: every `.kf` file directly in it (loose or archived, any case, listed once) is set
/// for the group it names, under the folder's path; the first file in name order takes a group
/// two files name (the other's load given back). The finder is asked as `BuildKFFileList` asks
/// it, without the idle manager, and its list is given back name by name.
#[test]
fn a_folder_sets_each_of_its_files_for_its_group() {
    covers(&[ids::extenders::knvse::SETACTORANIMATIONPATH]);
    let mut e = setup();
    let a_sprint = kf_file(&mut e, 1, "em\\move\\a_sprint.kf", SPRINT);
    let b_sprint = kf_file(&mut e, 2, "em\\move\\b_sprint.kf", SPRINT);
    let run = kf_file(&mut e, 3, "em\\move\\run.kf", RUN);
    let deeper = kf_file(&mut e, 4, "em\\move\\more\\walk.kf", 0x0215);
    WORLD.with(|w| {
        let mut w = w.borrow_mut();
        w.listed.push(b"EM\\Move\\Run.KF".to_vec());
        w.listed.push(b"em\\move\\readme.txt".to_vec());
    });
    assert_eq!(set(&mut e, ACTOR, &[n(0), n(1), s("EM/Move/")]), 1.0);
    let searched = WORLD.with(|w| w.borrow().searched.clone());
    assert_eq!(
        searched,
        [(
            b"Data\\Meshes\\em\\move\\*.kf".to_vec(),
            b"em\\move\\*.kf".to_vec()
        )]
    );
    let top = |e: &mut Engine, group| {
        state(e)
            .store
            .lookup(ACTOR_ID, false, group)
            .map(|o| (o.kf, o.path.clone()))
    };
    assert_eq!(top(&mut e, SPRINT), Some((a_sprint, b"em\\move".to_vec())));
    assert_eq!(top(&mut e, RUN), Some((run, b"em\\move".to_vec())));
    assert_eq!(top(&mut e, 0x0215), None, "not in a folder below");
    assert_eq!(state(&mut e).store.len(), 2);
    assert_eq!(
        (
            refs(&e, a_sprint),
            refs(&e, b_sprint),
            refs(&e, run),
            refs(&e, deeper)
        ),
        (1, 0, 1, 0)
    );
    let (freed, deleted) = WORLD.with(|w| (w.borrow().freed.len(), w.borrow().deleted.len()));
    assert_eq!(
        (freed, deleted),
        (5, 1),
        "each listed name freed, the list deleted"
    );
    // The same folder written another way is the same override: it comes off whole.
    assert_eq!(set(&mut e, ACTOR, &[n(0), n(0), s("em\\move")]), 1.0);
    assert!(state(&mut e).store.is_empty());
    assert_eq!((refs(&e, a_sprint), refs(&e, run)), (0, 0));
}

/// A folder with a condition: each file's entry holds it, and its variables are saved once per
/// entry and given back once per entry.
#[test]
fn a_folders_condition_is_kept_with_each_file() {
    covers(&[ids::extenders::knvse::SETACTORANIMATIONPATH]);
    let mut e = setup();
    kf_file(&mut e, 1, "em\\stop\\sprint.kf", SPRINT);
    kf_file(&mut e, 2, "em\\stop\\run.kf", RUN);
    set_script_ref(&mut e, 1, LAMBDA);
    assert_eq!(
        set(&mut e, 0, &[n(1), n(1), s("em\\stop"), n(1), r(1)]),
        1.0
    );
    for group in [SPRINT, RUN] {
        let o = state(&mut e)
            .store
            .lookup(0x14, true, group)
            .unwrap()
            .clone();
        assert_eq!((o.condition, o.poll), (LAMBDA, true));
    }
    assert_eq!(
        crate::SAVED.with(|s| s.take()),
        [(LAMBDA, true), (LAMBDA, true)]
    );
    set(&mut e, 0, &[n(1), n(0), s("em\\stop")]);
    assert_eq!(
        crate::SAVED.with(|s| s.take()),
        [(LAMBDA, false), (LAMBDA, false)]
    );
}

/// Through the command, the seam and the frame together: an override set with a polled condition
/// takes over the playing group when the condition starts to hold, and gives it back when it
/// stops.
#[test]
fn a_polled_override_set_by_the_command_switches_while_the_group_plays() {
    covers(&[ids::extenders::knvse::SETACTORANIMATIONPATH]);
    use crate::test_support::{play, playing, started, GAME_SEQUENCE};
    let mut e = setup();
    kf_file(&mut e, 1, "a\\sprint.kf", SPRINT);
    set_script_ref(&mut e, 1, LAMBDA);
    let animation = 0x0303_a000;
    e.mem.set_u32(animation + 4, ACTOR);
    e.mem.set_u32(animation + 0xd8, 0x0303_b000);
    let holds = |on: bool| {
        crate::seam::CONDITIONS.with(|c| c.borrow_mut().1 = if on { vec![LAMBDA] } else { vec![] })
    };
    assert_eq!(
        set(&mut e, ACTOR, &[n(0), n(1), s("a\\sprint.kf"), n(1), r(1)]),
        1.0
    );
    holds(false);
    assert_eq!(play(&mut e, animation, SPRINT, 0), GAME_SEQUENCE);
    holds(true);
    nvse::plugins::dispatch(&mut e, nvse::plugins::msg::MAIN_GAME_LOOP, 0, 0);
    assert_eq!(started(), [[animation, u32::from(SPRINT), 0]]);
    assert_ne!(playing(&e, animation, 0), GAME_SEQUENCE);
    holds(false);
    nvse::plugins::dispatch(&mut e, nvse::plugins::msg::MAIN_GAME_LOOP, 0, 0);
    assert_eq!(playing(&e, animation, 0), GAME_SEQUENCE);
}
