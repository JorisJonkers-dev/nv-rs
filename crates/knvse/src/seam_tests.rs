// SPDX-License-Identifier: GPL-3.0-only
use super::*;
use crate::store::{Key, Override, Target};
use crate::test_support::{engine_as, kf_file, refs, WORLD};

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
            poll: false,
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

/// No manager, a manager that refuses the sequence: nothing answers and no reference is kept.
#[test]
fn without_a_manager_or_when_it_refuses_nothing_answers() {
    let mut e = engine(true);
    let kf = put(&mut e, Target::Every, false, 1, 0);
    e.mem.set_u32(THIRD + ANIMATION_MANAGER, 0);
    assert_eq!(ask(&mut e, THIRD, SPRINT), None);
    assert!(built().is_empty());
    WORLD.with(|w| w.borrow_mut().refuse_add = true);
    assert_eq!(ask(&mut e, NPC_ANIMATION, SPRINT), None);
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
