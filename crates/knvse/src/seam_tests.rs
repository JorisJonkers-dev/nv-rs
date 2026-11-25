// SPDX-License-Identifier: GPL-3.0-only
use super::*;
use crate::store::{Key, Override, Target};
use crate::test_support::{engine_as, kf_file, play, playing, refs, started, GAME_SEQUENCE, WORLD};
use nvse::plugins::{dispatch, msg};

const PLAYER: u32 = 0x0303_9000;
const NPC: u32 = 0x0303_9100;
const FIRST: u32 = 0x0303_a000;
const THIRD: u32 = 0x0303_a200;
const NPC_ANIMATION: u32 = 0x0303_a400;
const NO_ACTOR: u32 = 0x0303_a600;
const MANAGER: u32 = 0x0303_b000;
const OTHER_MANAGER: u32 = 0x0303_b100;
const PLAYER_ID: u32 = 0x14;
const NPC_ID: u32 = 0x0010_0abc;
const SPRINT: u16 = 0x0213;
const LAMBDA: u32 = 0x0303_c000;
const LAMBDA_2: u32 = 0x0303_c100;

/// An engine with the player (first- and third-person animations), an NPC with one animation, an
/// animation of no actor, each animation with a controller manager, and kNVSE installed as `on`
/// says.
fn engine(on: bool) -> Engine {
    let mut e = engine_as(on);
    e.set_global(PLAYER_SINGLETON, PLAYER);
    e.mem.set_u32(PLAYER + FORM_ID, PLAYER_ID);
    e.mem.set_u32(NPC + FORM_ID, NPC_ID);
    e.mem.set_u32(FIRST + ANIMATION_ACTOR, PLAYER);
    e.mem.set_u32(THIRD + ANIMATION_ACTOR, PLAYER);
    e.mem.set_u32(NPC_ANIMATION + ANIMATION_ACTOR, NPC);
    for animation in [FIRST, THIRD, NPC_ANIMATION, NO_ACTOR] {
        e.mem.set_u32(animation + ANIMATION_MANAGER, MANAGER);
    }
    e.register(PLAYER_GET_ANIMATION, |_, a| Ret {
        eax: if a[1] == 1 { FIRST } else { THIRD },
        ..Ret::default()
    });
    e
}

fn ask(e: &mut Engine, animation: u32, group: u16) -> Option<u32> {
    e.site(0x0049_48c0, &[animation, u32::from(group), 4])
}

/// A `.kf` model for `SPRINT` as the command would hold it (loaded: one manual reference) put on
/// the stack of `target` and person for `owner`, with `condition`.
fn put(e: &mut Engine, target: Target, first_person: bool, owner: u32, condition: u32) -> u32 {
    put_with(e, target, first_person, owner, condition, false)
}

/// [`put`] with the poll flag given.
fn put_with(
    e: &mut Engine,
    target: Target,
    first_person: bool,
    owner: u32,
    condition: u32,
    poll: bool,
) -> u32 {
    let path = format!("mod{owner}\\sprint{}.kf", u8::from(first_person));
    let kf = kf_file(
        e,
        owner * 4 + u32::from(first_person) + 2 * u32::from(target == Target::Every),
        &path,
        SPRINT,
    );
    let r = refs(e, kf);
    e.mem.set_u32(kf + crate::test_support::KF_REFS, r + 1);
    let key = Key {
        target,
        first_person,
        group: SPRINT,
    };
    crate::enable(
        e,
        key,
        Override {
            owner,
            path: path.into_bytes(),
            kf,
            condition,
            poll,
        },
    );
    kf
}

/// The sequences built so far: (block, group, source).
fn built() -> Vec<[u32; 3]> {
    WORLD.with(|w| w.borrow().built.clone())
}

fn holds(scripts: &[u32]) {
    CONDITIONS.with(|c| c.borrow_mut().1 = scripts.to_vec());
}

fn conditions_asked() -> Vec<(u32, u32)> {
    CONDITIONS.with(|c| std::mem::take(&mut c.borrow_mut().0))
}

/// Installed with an empty store, and not installed with overrides set, the seam answers nothing:
/// the game's own path.
#[test]
fn the_seam_answers_nothing_while_the_store_is_empty_or_the_plugin_is_off() {
    let mut e = engine(true);
    assert_eq!(e.ext_free::<crate::Active>().count(), 0);
    for animation in [FIRST, THIRD, NPC_ANIMATION, NO_ACTOR] {
        assert_eq!(ask(&mut e, animation, SPRINT), None);
    }
    let mut e = engine(false);
    put(&mut e, Target::Every, false, 1, 0);
    put(&mut e, Target::Every, true, 1, 0);
    for animation in [FIRST, THIRD, NPC_ANIMATION] {
        assert_eq!(ask(&mut e, animation, SPRINT), None, "not installed");
    }
    assert!(built().is_empty());
}

/// The player's first-person animation takes first-person overrides, its third-person one and an
/// NPC's take third-person ones, each for the actor set; another group answers nothing. Each
/// answer is a sequence made from the override's model as `AddAnimation` makes one and added to
/// the animation's manager.
#[test]
fn the_seam_answers_the_override_for_the_actor_and_person() {
    let mut e = engine(true);
    let kf = put(&mut e, Target::Actor(PLAYER_ID), true, 1, 0);
    let first = ask(&mut e, FIRST, SPRINT).expect("first person");
    let [block, group, source] = built()[0];
    assert_eq!(block, first);
    assert_eq!((group, source), (e.mem.u32(kf + 8), e.mem.u32(kf + 4)));
    assert_eq!(
        WORLD.with(|w| w.borrow().added.clone()),
        [[MANAGER, first, ACCUM_ROOT_NAME, 1]]
    );
    assert_eq!(ask(&mut e, THIRD, SPRINT), None);
    assert_eq!(ask(&mut e, NPC_ANIMATION, SPRINT), None);
    assert_eq!(ask(&mut e, FIRST, SPRINT + 1), None, "another group");
    put(&mut e, Target::Actor(NPC_ID), false, 2, 0);
    let npc = ask(&mut e, NPC_ANIMATION, SPRINT).expect("the NPC's");
    assert_eq!(ask(&mut e, THIRD, SPRINT), None);
    put(&mut e, Target::Every, false, 3, 0);
    let every = ask(&mut e, THIRD, SPRINT).expect("every actor's");
    assert_ne!(every, npc);
    assert_eq!(
        ask(&mut e, NPC_ANIMATION, SPRINT),
        Some(npc),
        "its own first"
    );
    assert_eq!(ask(&mut e, NO_ACTOR, SPRINT), None, "no actor");
}

/// One sequence per animation and model: asked again, the same sequence answers and nothing is
/// built; another animation with the same model gets its own; the cache holds one reference on
/// the model per sequence.
#[test]
fn a_sequence_is_made_once_per_animation_and_model() {
    let mut e = engine(true);
    let kf = put(&mut e, Target::Every, false, 1, 0);
    assert_eq!(refs(&e, kf), 1);
    let s1 = ask(&mut e, THIRD, SPRINT).unwrap();
    assert_eq!(ask(&mut e, THIRD, SPRINT), Some(s1));
    assert_eq!(built().len(), 1);
    assert_eq!(refs(&e, kf), 2);
    let s2 = ask(&mut e, NPC_ANIMATION, SPRINT).unwrap();
    assert_ne!(s1, s2);
    assert_eq!(built().len(), 2);
    assert_eq!(refs(&e, kf), 3);
    assert_eq!(e.ext_free::<crate::Active>().cached(), 2);
}

/// Regression guard (nv-rs#865): when the animation's controller manager is made again (the seam
/// in `0048ffd0`) or the animation torn down (`0048fb50`), its kept sequences are forgotten and
/// their references given back, and the next start makes a new one for the new manager; other
/// animations keep theirs.
#[test]
fn a_rebuilt_manager_or_torn_down_animation_forgets_its_sequences() {
    let mut e = engine(true);
    let kf = put(&mut e, Target::Every, false, 1, 0);
    let old = ask(&mut e, THIRD, SPRINT).unwrap();
    let npc = ask(&mut e, NPC_ANIMATION, SPRINT).unwrap();
    assert_eq!(refs(&e, kf), 3);
    // The 3D is rebuilt: a new manager is stored, then the seam is asked.
    e.mem.set_u32(THIRD + ANIMATION_MANAGER, OTHER_MANAGER);
    assert_eq!(e.site(0x0048_ffd0, &[THIRD]), None);
    assert_eq!(refs(&e, kf), 2);
    let new = ask(&mut e, THIRD, SPRINT).unwrap();
    assert_ne!(new, old);
    assert_eq!(
        WORLD.with(|w| w.borrow().added.last().copied()),
        Some([OTHER_MANAGER, new, ACCUM_ROOT_NAME, 1])
    );
    assert_eq!(ask(&mut e, NPC_ANIMATION, SPRINT), Some(npc), "untouched");
    // The NPC's animation is torn down.
    assert_eq!(e.site(0x0048_fb50, &[NPC_ANIMATION]), None);
    assert_eq!(e.ext_free::<crate::Active>().cached(), 1);
    assert_eq!(refs(&e, kf), 2);
    // Taking the override off and tearing the last animation down gives every reference back.
    let path = b"mod1\\sprint0.kf";
    assert_eq!(
        crate::disable_path(&mut e, Target::Every, false, 1, path),
        1
    );
    e.site(0x0048_fb50, &[THIRD]);
    assert_eq!(refs(&e, kf), 0);
    assert_eq!(e.ext_free::<crate::Active>().cached(), 0);
}

/// A manager changed without the seam being asked (any other path that rebuilds it) is noticed:
/// the stale sequence is forgotten and a new one made for the manager the animation holds.
#[test]
fn a_kept_sequence_of_another_manager_is_not_answered() {
    let mut e = engine(true);
    let kf = put(&mut e, Target::Every, false, 1, 0);
    let old = ask(&mut e, THIRD, SPRINT).unwrap();
    e.mem.set_u32(THIRD + ANIMATION_MANAGER, OTHER_MANAGER);
    let new = ask(&mut e, THIRD, SPRINT).unwrap();
    assert_ne!(new, old);
    assert_eq!(
        refs(&e, kf),
        2,
        "one for the store, one for the new sequence"
    );
    assert_eq!(e.ext_free::<crate::Active>().cached(), 1);
}

/// No manager: nothing answers and nothing is built. A manager that refuses the sequence: nothing
/// answers, and the refusal is kept, so later starts build no new block for that manager; the
/// teardown gives the kept reference back.
#[test]
fn without_a_manager_or_when_it_refuses_nothing_answers() {
    let mut e = engine(true);
    let kf = put(&mut e, Target::Every, false, 1, 0);
    e.mem.set_u32(THIRD + ANIMATION_MANAGER, 0);
    assert_eq!(ask(&mut e, THIRD, SPRINT), None);
    assert!(built().is_empty());
    WORLD.with(|w| w.borrow_mut().refuse_add = true);
    assert_eq!(ask(&mut e, NPC_ANIMATION, SPRINT), None);
    assert_eq!(ask(&mut e, NPC_ANIMATION, SPRINT), None);
    assert_eq!(built().len(), 1, "a refused sequence is not built again");
    assert_eq!(refs(&e, kf), 2);
    assert_eq!(e.ext_free::<crate::Active>().cached(), 1);
    assert_eq!(e.site(0x0048_fb50, &[NPC_ANIMATION]), None);
    assert_eq!(refs(&e, kf), 1);
    assert_eq!(e.ext_free::<crate::Active>().cached(), 0);
}

/// A condition is asked with the actor as its calling reference; while it does not hold, the next
/// candidate answers (the actor's stack, then every actor's), and with none holding the game's
/// path goes on.
#[test]
fn conditions_pick_the_override_that_answers() {
    let mut e = engine(true);
    let every = put(&mut e, Target::Every, false, 1, LAMBDA_2);
    let own = put(&mut e, Target::Actor(NPC_ID), false, 2, LAMBDA);
    holds(&[]);
    assert_eq!(ask(&mut e, NPC_ANIMATION, SPRINT), None);
    assert_eq!(conditions_asked(), [(LAMBDA, NPC), (LAMBDA_2, NPC)]);
    holds(&[LAMBDA_2]);
    let s = ask(&mut e, NPC_ANIMATION, SPRINT).unwrap();
    assert_eq!(built().last().unwrap()[2], e.mem.u32(every + 4));
    holds(&[LAMBDA, LAMBDA_2]);
    let t = ask(&mut e, NPC_ANIMATION, SPRINT).unwrap();
    assert_ne!(s, t);
    assert_eq!(built().last().unwrap()[2], e.mem.u32(own + 4));
    assert_eq!(conditions_asked().last(), Some(&(LAMBDA, NPC)));
}

/// Stacked overrides answer through the seam newest first and come back as they are disabled;
/// clearing empties the count and gives every reference back.
#[test]
fn the_seam_follows_the_stack() {
    let mut e = engine(true);
    let one = put(&mut e, Target::Actor(NPC_ID), false, 1, 0);
    let two = put(&mut e, Target::Actor(NPC_ID), false, 2, 0);
    assert_eq!(e.ext_free::<crate::Active>().count(), 2);
    ask(&mut e, NPC_ANIMATION, SPRINT).unwrap();
    assert_eq!(built().last().unwrap()[2], e.mem.u32(two + 4));
    let key = Key {
        target: Target::Actor(NPC_ID),
        first_person: false,
        group: SPRINT,
    };
    assert!(crate::disable(&mut e, key, 2, b"mod2\\sprint0.kf"));
    ask(&mut e, NPC_ANIMATION, SPRINT).unwrap();
    assert_eq!(built().last().unwrap()[2], e.mem.u32(one + 4));
    assert_eq!(e.ext_free::<crate::Active>().count(), 1);
    crate::clear(&mut e);
    assert_eq!(e.ext_free::<crate::Active>().count(), 0);
    assert_eq!(ask(&mut e, NPC_ANIMATION, SPRINT), None);
    e.site(0x0048_fb50, &[NPC_ANIMATION]);
    assert_eq!((refs(&e, one), refs(&e, two)), (0, 0));
}

/// A second install does nothing (one handler at each seam).
#[test]
fn installing_twice_registers_each_seam_once() {
    let mut e = engine(true);
    crate::install_as(&mut e, true);
    for at in [0x0049_48c0, 0x0048_ffd0, 0x0048_fb50] {
        assert_eq!(e.site_owners(at), [Owner::Knvse], "{at:08x}");
    }
}

/// Regression guard (nv-rs#865): installed with an empty store, fresh or emptied again, the
/// handlers answer nothing, call nothing and leave every mapped byte as it was, so
/// `Animation::StartGroup` and the manager's set-up and teardown go on along the exe's path.
#[test]
fn an_empty_store_calls_nothing_and_writes_nothing() {
    let fresh = engine(true);
    let emptied = {
        let mut e = engine(true);
        put(&mut e, Target::Actor(NPC_ID), false, 1, 0);
        put(&mut e, Target::Every, true, 2, 0);
        crate::clear(&mut e);
        e
    };
    for mut e in [fresh, emptied] {
        let memory = |e: &Engine| {
            (
                e.mem.bytes(0x011d_e000, 0x1000),
                e.mem.bytes(0x0300_0000, 0x40000),
            )
        };
        let before = memory(&e);
        e.call_log = Some(vec![]);
        for animation in [FIRST, THIRD, NPC_ANIMATION, NO_ACTOR] {
            for group in [SPRINT, 0, 0xfe] {
                assert_eq!(ask(&mut e, animation, group), None);
            }
            assert_eq!(e.site(0x0048_ffd0, &[animation]), None);
            assert_eq!(e.site(0x0048_fb50, &[animation]), None);
        }
        assert_eq!(e.call_log.take().unwrap(), []);
        assert_eq!(memory(&e), before);
    }
}

fn watched(e: &mut Engine) -> u32 {
    e.ext_free::<crate::Active>().watched()
}

/// A polled condition is asked again every frame while its group plays: when it starts to hold
/// the group is started again in its slot and the override plays; when it stops, again, and the
/// game's own sequence plays. While the answer stays the same nothing is started.
#[test]
fn a_polled_condition_switches_the_playing_group_both_ways() {
    let mut e = engine(true);
    put_with(&mut e, Target::Every, false, 1, LAMBDA, true);
    holds(&[]);
    assert_eq!(play(&mut e, THIRD, SPRINT, 0), GAME_SEQUENCE);
    assert_eq!(watched(&mut e), 1);
    poll(&mut e);
    assert!(started().is_empty(), "the answer did not change");
    holds(&[LAMBDA]);
    poll(&mut e);
    assert_eq!(started(), [[THIRD, u32::from(SPRINT), 0]]);
    let ours = playing(&e, THIRD, 0);
    assert_ne!(ours, GAME_SEQUENCE);
    assert_eq!(built().len(), 1);
    poll(&mut e);
    assert!(started().is_empty());
    holds(&[]);
    poll(&mut e);
    assert_eq!(started(), [[THIRD, u32::from(SPRINT), 0]]);
    assert_eq!(playing(&e, THIRD, 0), GAME_SEQUENCE);
    assert_eq!(watched(&mut e), 1, "still playing");
}

/// The group is found in whatever slot it plays in, and one override taking another's place on
/// the stack while it plays is started too.
#[test]
fn polling_follows_the_slot_and_the_stack() {
    let mut e = engine(true);
    put_with(&mut e, Target::Every, false, 1, LAMBDA, true);
    holds(&[LAMBDA]);
    let first = play(&mut e, NPC_ANIMATION, SPRINT, 3);
    assert_ne!(first, GAME_SEQUENCE);
    put_with(&mut e, Target::Actor(NPC_ID), false, 2, LAMBDA_2, true);
    holds(&[LAMBDA, LAMBDA_2]);
    poll(&mut e);
    assert_eq!(started(), [[NPC_ANIMATION, u32::from(SPRINT), 3]]);
    assert_ne!(playing(&e, NPC_ANIMATION, 3), first);
}

/// Without the poll flag, or a polled flag without a condition, nothing is watched: the
/// condition is asked at the start only.
#[test]
fn an_unpolled_condition_is_asked_at_the_start_only() {
    let mut e = engine(true);
    put_with(&mut e, Target::Every, false, 1, LAMBDA, false);
    put_with(&mut e, Target::Actor(NPC_ID), false, 2, 0, true);
    holds(&[]);
    play(&mut e, THIRD, SPRINT, 0);
    play(&mut e, NPC_ANIMATION, SPRINT, 0);
    assert_eq!(watched(&mut e), 0);
    conditions_asked();
    holds(&[LAMBDA]);
    poll(&mut e);
    assert!(conditions_asked().is_empty());
    assert!(started().is_empty());
}

/// A watch ends when its group no longer plays in any slot, when the animation is torn down or
/// its manager made again, on a load and on the way to the main menu; with none left a frame
/// asks nothing.
#[test]
fn a_watch_ends_with_its_group_or_its_animation() {
    let mut e = engine(true);
    put_with(&mut e, Target::Every, false, 1, LAMBDA, true);
    holds(&[]);
    play(&mut e, THIRD, SPRINT, 0);
    play(&mut e, NPC_ANIMATION, SPRINT, 0);
    assert_eq!(watched(&mut e), 2);
    // Another group takes the third-person animation's slot.
    play(&mut e, THIRD, SPRINT + 1, 0);
    conditions_asked();
    poll(&mut e);
    assert_eq!(watched(&mut e), 1);
    assert_eq!(
        conditions_asked(),
        [(LAMBDA, NPC)],
        "only the group still playing"
    );
    e.site(0x0048_fb50, &[NPC_ANIMATION]);
    assert_eq!(watched(&mut e), 0);
    play(&mut e, NPC_ANIMATION, SPRINT, 0);
    e.site(0x0048_ffd0, &[NPC_ANIMATION]);
    assert_eq!(watched(&mut e), 0);
    for kind in [msg::PRE_LOAD_GAME, msg::EXIT_TO_MAIN_MENU] {
        play(&mut e, NPC_ANIMATION, SPRINT, 0);
        assert_eq!(watched(&mut e), 1);
        dispatch(&mut e, kind, 0, 0);
        assert_eq!(watched(&mut e), 0, "{kind}");
    }
    conditions_asked();
    holds(&[LAMBDA]);
    poll(&mut e);
    assert!(conditions_asked().is_empty());
    assert!(started().is_empty());
}

/// The plugin's listener polls on xNVSE's frame message; off, nothing listens.
#[test]
fn the_frame_message_polls() {
    let mut e = engine(true);
    put_with(&mut e, Target::Every, false, 1, LAMBDA, true);
    holds(&[]);
    play(&mut e, THIRD, SPRINT, 0);
    holds(&[LAMBDA]);
    dispatch(&mut e, msg::MAIN_GAME_LOOP, 0, 0);
    assert_eq!(started(), [[THIRD, u32::from(SPRINT), 0]]);
    let mut off = engine(false);
    put_with(&mut off, Target::Every, false, 1, LAMBDA, true);
    dispatch(&mut off, msg::MAIN_GAME_LOOP, 0, 0);
    assert!(started().is_empty());
}
