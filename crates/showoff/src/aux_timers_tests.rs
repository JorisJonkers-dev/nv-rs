// SPDX-License-Identifier: GPL-3.0-only
//! The auxiliary timers through their records and the frame: made, read, counted down by the
//! rules of their flags, stopped, paused, their events, the owner and name rules, and what a load
//! clears.

use super::*;
use crate::test_support::{call, engine, n, r, s, set_script_ref, z, SCRIPT};

const START: u32 = 0x3d3e;
const STOP: u32 = 0x3d3f;
const PAUSED: u32 = 0x3d40;
const ELAPSED: u32 = 0x3d41;
const TO_COUNTDOWN: u32 = 0x3d42;
const TIME_LEFT: u32 = 0x3d43;
const ON_START: u32 = 0x3d44;
const ON_STOP: u32 = 0x3d45;
const ON_UPDATE: u32 = 0x3d4c;

/// The stand-ins' settings: menu mode (a word), V.A.T.S.'s multiplier (a double), the forms that
/// `LookupFormByID` finds (pairs of id and address, ended by 0).
const MENU: u32 = 0x0300_f000;
const VATS: u32 = 0x0300_f008;
const FORMS: u32 = 0x0300_f100;

const PLAYER: u32 = 0x0300_d000;
const NPC: u32 = 0x0300_d100;
const OTHER: u32 = 0x0300_d200;
const HANDLER: u32 = 0x0300_d300;
const HANDLER_2: u32 = 0x0300_d400;
const FILE: u32 = 0x0300_d800;

/// Seconds of the frame: the step, and the step times the global multiplier.
const STEP: f32 = 0.5;
const GLOBAL_STEP: f32 = 0.25;

fn setup() -> Engine {
    let mut e = engine();
    e.map(0x011d_a000, 0x1000);
    e.map(0x011f_0000, 0x10000);
    e.mem.set_f32(TIME_GLOBAL + DELTA, GLOBAL_STEP);
    e.mem.set_f32(TIME_GLOBAL + DELTA_UNMODIFIED, STEP);
    e.mem.set_f64(VATS, 0.5);
    e.register(MENU_MODE, |e, _| Ret {
        eax: e.mem.u32(MENU),
        ..Ret::default()
    });
    e.register(VATS_TIME_MULT, |e, _| Ret {
        st0: e.mem.f64(VATS),
        ..Ret::default()
    });
    e.register(CONSOLE_INSTANCE, |_, _| Ret::default());
    e.register(LOOKUP_FORM, |e, a| {
        let mut at = FORMS;
        loop {
            let id = e.mem.u32(at);
            if id == 0 {
                return Ret::default();
            }
            if id == a[0] {
                return Ret {
                    eax: e.mem.u32(at + 4),
                    ..Ret::default()
                };
            }
            at += 8;
        }
    });
    for (i, (form, id)) in [
        (PLAYER, PLAYER_REF_ID),
        (NPC, 0x0001_0001),
        (OTHER, 0x0001_0002),
    ]
    .into_iter()
    .enumerate()
    {
        e.mem.set_u32(form + 0xc, id);
        e.mem.set_u32(FORMS + 8 * i as u32, id);
        e.mem.set_u32(FORMS + 8 * i as u32 + 4, form);
    }
    set_script_ref(&mut e, 1, NPC);
    set_script_ref(&mut e, 2, HANDLER);
    set_script_ref(&mut e, 3, OTHER);
    set_script_ref(&mut e, 4, HANDLER_2);
    let _ = calls();
    e
}

fn result(e: &mut Engine, opcode: u32, this_obj: u32, args: &[Vec<u8>]) -> f64 {
    let (ok, v) = call(e, opcode, this_obj, args);
    assert_eq!(ok, 1);
    v
}

fn left(e: &mut Engine, name: &str) -> f64 {
    result(e, TIME_LEFT, 0, &[s(name)])
}

fn frame(e: &mut Engine) {
    nvse::plugins::dispatch(e, msg::MAIN_GAME_LOOP, 0, 0);
}

/// The handler calls since the last look: the script, the name's text, the owner form, and the
/// third word (the update's time) when there is one.
fn events(e: &Engine) -> Vec<(u32, String, u32, Option<f32>)> {
    calls()
        .into_iter()
        .map(|(script, w)| {
            let name = String::from_utf8(nvse::table::guest_text(e, w[0])).unwrap();
            (script, name, w[1], w.get(2).map(|b| f32::from_bits(*b)))
        })
        .collect()
}

#[test]
fn records_sit_at_their_opcodes_with_their_names_and_aliases() {
    let e = setup();
    let names: Vec<&str> = COMMANDS.iter().map(|c| c.name).collect();
    assert_eq!(names, crate::COMMAND_NAMES[16..25]);
    let opcodes: Vec<u32> = COMMANDS.iter().map(|c| c.opcode).collect();
    assert_eq!(
        opcodes,
        [
            START,
            STOP,
            PAUSED,
            ELAPSED,
            TO_COUNTDOWN,
            TIME_LEFT,
            ON_START,
            ON_STOP,
            ON_UPDATE
        ]
    );
    for c in &COMMANDS {
        let record = nvse::table::record_of(&e, c.opcode);
        assert_ne!(record, 0, "{}", c.name);
        assert_eq!(
            nvse::table::guest_text(&e, e.mem.u32(record)),
            c.name.as_bytes()
        );
        assert!(!c.needs_ref);
    }
    let shorts: Vec<&str> = COMMANDS.iter().map(|c| c.short).collect();
    assert_eq!(
        shorts,
        [
            "",
            "AuxTimerErase",
            "",
            "",
            "",
            "",
            "SetOnTimerStartHandler",
            "SetOnTimerStopHandler",
            "SetOnTimerUpdateHandler"
        ]
    );
}

#[test]
fn the_plugin_listens_to_every_frame() {
    let e = setup();
    assert!(e.ext_get::<nvse::Nvse>().unwrap().plugins.per_frame);
    let plugin = nvse::plugins::built_ins(&e)
        .into_iter()
        .find(|p| p.name == crate::identity::NAME)
        .unwrap();
    assert!(plugin.listener.is_some());
}

#[test]
fn a_started_timer_answers_its_times_and_a_missing_one_minus_one() {
    let mut e = setup();
    assert_eq!(left(&mut e, "*_t"), -1.0);
    assert_eq!(result(&mut e, ELAPSED, 0, &[s("*_t")]), -1.0);
    assert_eq!(result(&mut e, TO_COUNTDOWN, 0, &[s("*_t")]), -1.0);
    assert_eq!(result(&mut e, PAUSED, 0, &[s("*_t")]), -1.0);
    assert_eq!(result(&mut e, START, 0, &[s("*_t"), z(2.0)]), 1.0);
    assert_eq!(left(&mut e, "*_t"), 2.0);
    assert_eq!(result(&mut e, TO_COUNTDOWN, 0, &[s("*_t")]), 2.0);
    assert_eq!(result(&mut e, ELAPSED, 0, &[s("*_t")]), 0.0);
    let t = &e.ext::<State>().timers.timers[0];
    assert_eq!(t.flags, flag::DEFAULT);
    assert_eq!(t.key.owner, PLAYER_REF_ID);
}

/// The default flags count seconds in both modes with both multipliers; the flags that ignore
/// turbo or the time multiplier take the other steps; a frame-based timer counts frames.
#[test]
fn each_timer_counts_by_the_rule_of_its_flags() {
    let mut e = setup();
    result(&mut e, START, 0, &[s("*_both"), z(10.0)]);
    result(&mut e, START, 0, &[s("*_turbo"), z(10.0), n(2 | 4 | 512)]);
    result(&mut e, START, 0, &[s("*_real"), z(10.0), n(2 | 4 | 1024)]);
    result(&mut e, START, 0, &[s("*_frames"), z(10.0), n(2)]);
    frame(&mut e);
    assert_eq!(left(&mut e, "*_both"), 10.0 - 0.125);
    assert_eq!(left(&mut e, "*_turbo"), 10.0 - 0.25);
    assert_eq!(left(&mut e, "*_real"), 10.0 - 0.5);
    assert_eq!(left(&mut e, "*_frames"), 9.0);
    assert_eq!(result(&mut e, ELAPSED, 0, &[s("*_frames")]), 1.0);
    // In menu mode: only the timers that run there count; the one ignoring the time multiplier in
    // menu mode takes the plain step.
    e.mem.set_u32(MENU, 1);
    result(&mut e, START, 0, &[s("*_menu"), z(10.0), n(1 | 4 | 64)]);
    frame(&mut e);
    assert_eq!(left(&mut e, "*_both"), 10.0 - 0.25);
    assert_eq!(left(&mut e, "*_turbo"), 10.0 - 0.25);
    assert_eq!(left(&mut e, "*_frames"), 9.0);
    assert_eq!(left(&mut e, "*_menu"), 10.0 - 0.5);
}

#[test]
fn paused_timers_and_timers_that_stop_while_the_game_is_paused_do_not_count() {
    let mut e = setup();
    result(&mut e, START, 0, &[s("*_p"), z(5.0), n(2 | 32)]);
    result(&mut e, START, 0, &[s("*_np"), z(5.0), n(1 | 2 | 128)]);
    result(&mut e, START, 0, &[s("*_any"), z(5.0), n(1 | 2)]);
    e.mem.set_u32(START_MENU, 1);
    frame(&mut e);
    assert_eq!(left(&mut e, "*_p"), 5.0);
    assert_eq!(left(&mut e, "*_np"), 5.0);
    assert_eq!(left(&mut e, "*_any"), 4.0);
    // AuxTimerPaused answers what it was, then sets.
    assert_eq!(result(&mut e, PAUSED, 0, &[s("*_p"), n(0)]), 1.0);
    assert_eq!(result(&mut e, PAUSED, 0, &[s("*_p")]), 0.0);
    e.mem.set_u32(START_MENU, 0);
    frame(&mut e);
    assert_eq!(left(&mut e, "*_p"), 4.0);
    assert_eq!(result(&mut e, PAUSED, 0, &[s("*_p"), n(1)]), 0.0);
    frame(&mut e);
    assert_eq!(left(&mut e, "*_p"), 4.0);
}

/// A timer that runs out calls its stop handlers with its name and owner and is erased; one with
/// the auto-restart flag starts again and calls its start handlers.
#[test]
fn a_timer_that_runs_out_fires_its_stop_event_and_is_erased_or_restarts() {
    let mut e = setup();
    result(&mut e, ON_STOP, 0, &[n(1), r(2), s("*_Once")]);
    result(&mut e, ON_STOP, 0, &[n(1), r(2), s("*_again")]);
    result(&mut e, ON_START, 0, &[n(1), r(4), s("*_again")]);
    result(&mut e, START, 0, &[s("*_once"), z(2.0), n(2)]);
    result(&mut e, START, 0, &[s("*_again"), z(2.0), n(2 | 16)]);
    assert_eq!(
        events(&e),
        vec![(HANDLER_2, "*_again".into(), PLAYER, None)]
    );
    frame(&mut e);
    assert!(events(&e).is_empty());
    frame(&mut e);
    assert_eq!(
        events(&e),
        vec![
            (HANDLER, "*_once".into(), PLAYER, None),
            (HANDLER, "*_again".into(), PLAYER, None),
            (HANDLER_2, "*_again".into(), PLAYER, None),
        ]
    );
    assert_eq!(left(&mut e, "*_once"), -1.0);
    assert_eq!(left(&mut e, "*_again"), 2.0);
}

#[test]
fn update_handlers_get_the_time_counted_each_frame() {
    let mut e = setup();
    result(&mut e, ON_UPDATE, 0, &[n(1), r(2), s("*_u")]);
    result(&mut e, ON_UPDATE, 0, &[n(1), r(2), s("*_f")]);
    result(&mut e, START, 0, &[s("*_u"), z(5.0), n(2 | 4 | 256 | 1024)]);
    result(&mut e, START, 0, &[s("*_f"), z(5.0), n(2 | 256)]);
    // No update without the flag.
    result(&mut e, ON_UPDATE, 0, &[n(1), r(2), s("*_none")]);
    result(&mut e, START, 0, &[s("*_none"), z(5.0), n(2)]);
    let _ = calls();
    frame(&mut e);
    assert_eq!(
        events(&e),
        vec![
            (HANDLER, "*_u".into(), PLAYER, Some(0.5)),
            (HANDLER, "*_f".into(), PLAYER, Some(1.0)),
        ]
    );
}

#[test]
fn stop_erases_and_fires_only_when_asked() {
    let mut e = setup();
    result(&mut e, ON_STOP, 0, &[n(1), r(2), s("*_a")]);
    result(&mut e, START, 0, &[s("*_a"), z(5.0)]);
    assert_eq!(result(&mut e, STOP, 0, &[s("*_a")]), 1.0);
    assert!(events(&e).is_empty());
    assert_eq!(result(&mut e, STOP, 0, &[s("*_a")]), 0.0);
    result(&mut e, START, 0, &[s("*_a"), z(5.0)]);
    assert_eq!(result(&mut e, STOP, 0, &[s("*_A"), n(1)]), 1.0);
    assert_eq!(events(&e), vec![(HANDLER, "*_a".into(), PLAYER, None)]);
    assert_eq!(left(&mut e, "*_a"), -1.0);
}

/// Removing a handler stops its calls; adding one twice calls it once.
#[test]
fn handlers_are_added_once_and_removed() {
    let mut e = setup();
    result(&mut e, ON_STOP, 0, &[n(1), r(2), s("*_a")]);
    result(&mut e, ON_STOP, 0, &[n(1), r(2), s("*_a")]);
    result(&mut e, START, 0, &[s("*_a"), z(1.0), n(2)]);
    frame(&mut e);
    assert_eq!(events(&e).len(), 1);
    result(&mut e, ON_STOP, 0, &[n(0), r(2), s("*_a")]);
    result(&mut e, START, 0, &[s("*_a"), z(1.0), n(2)]);
    frame(&mut e);
    assert!(events(&e).is_empty());
}

/// A restart takes the new time and runs a paused timer again; with no time given it counts its
/// own time again, and without a timer to restart nothing is made.
#[test]
fn start_restarts_a_timer_that_exists() {
    let mut e = setup();
    result(&mut e, START, 0, &[s("*_r"), z(4.0), n(2 | 32)]);
    assert_eq!(result(&mut e, PAUSED, 0, &[s("*_r")]), 1.0);
    assert_eq!(result(&mut e, START, 0, &[s("*_r"), z(3.0), n(2)]), 1.0);
    assert_eq!(result(&mut e, PAUSED, 0, &[s("*_r")]), 0.0);
    frame(&mut e);
    assert_eq!(left(&mut e, "*_r"), 2.0);
    assert_eq!(result(&mut e, START, 0, &[s("*_r")]), 1.0);
    assert_eq!(left(&mut e, "*_r"), 3.0);
    assert_eq!(result(&mut e, START, 0, &[s("*_new")]), 0.0);
    assert_eq!(left(&mut e, "*_new"), -1.0);
}

/// The owner: the form argument, else the calling reference, else `PlayerRef`. Each owner has its
/// own timer of a name; names compare without case.
#[test]
fn the_owner_and_the_name_choose_the_timer() {
    let mut e = setup();
    result(&mut e, START, 0, &[s("*_o"), z(1.0)]);
    result(&mut e, START, NPC, &[s("*_o"), z(2.0)]);
    result(&mut e, START, 0, &[s("*_o"), z(3.0), n(7), r(3)]);
    assert_eq!(left(&mut e, "*_O"), 1.0);
    assert_eq!(result(&mut e, TIME_LEFT, NPC, &[s("*_o")]), 2.0);
    assert_eq!(result(&mut e, TIME_LEFT, 0, &[s("*_o"), r(1)]), 2.0);
    assert_eq!(result(&mut e, TIME_LEFT, NPC, &[s("*_o"), r(3)]), 3.0);
    assert_eq!(e.ext::<State>().timers.timers.len(), 3);
}

/// A name without `_` belongs to the calling script's mod; with it, to every mod.
#[test]
fn a_private_name_belongs_to_the_scripts_mod() {
    let mut e = setup();
    result(&mut e, START, 0, &[s("*mine"), z(1.0)]);
    result(&mut e, START, 0, &[s("*_ours"), z(1.0)]);
    // The script now comes from mod 5.
    e.mem.set_u32(SCRIPT + 0x10, FILE);
    e.mem.set_u8(FILE + 0x40c, 5);
    assert_eq!(left(&mut e, "*mine"), -1.0);
    assert_eq!(left(&mut e, "*_ours"), 1.0);
    result(&mut e, START, 0, &[s("*mine"), z(2.0)]);
    assert_eq!(left(&mut e, "*mine"), 2.0);
    // A permanent name with `_` is shared too.
    result(&mut e, START, 0, &[s("_perm"), z(1.0)]);
    let mods: Vec<u32> = e
        .ext::<State>()
        .timers
        .timers
        .iter()
        .map(|t| t.key.mod_index)
        .collect();
    assert_eq!(mods, [0xff, 0xff, 5, 0xff]);
}

#[test]
fn a_timer_whose_owner_is_gone_is_erased_without_an_event() {
    let mut e = setup();
    result(&mut e, ON_STOP, 0, &[n(1), r(2), s("*_g"), r(3)]);
    result(&mut e, START, 0, &[s("*_g"), z(5.0), n(7), r(3)]);
    // `OTHER` is no longer found by its id.
    e.mem.set_u32(FORMS + 8 * 2, 0);
    frame(&mut e);
    assert!(events(&e).is_empty());
    assert!(e.ext::<State>().timers.timers.is_empty());
}

/// A load and the main menu take every handler and the timers that do not outlast them; a
/// temporary timer without the auto-remove flag stays. A new game drops only permanent timers.
#[test]
fn a_load_clears_handlers_and_the_timers_that_do_not_outlast_it() {
    let mut e = setup();
    for kind in [msg::PRE_LOAD_GAME, msg::EXIT_TO_MAIN_MENU] {
        result(&mut e, ON_STOP, 0, &[n(1), r(2), s("*_stays")]);
        result(&mut e, START, 0, &[s("*_stays"), z(5.0)]);
        result(&mut e, START, 0, &[s("*_goes"), z(5.0), n(1 | 2 | 4 | 8)]);
        result(&mut e, START, 0, &[s("permanent"), z(5.0)]);
        nvse::plugins::dispatch(&mut e, kind, 0, 0);
        let t = &e.ext::<State>().timers;
        assert!(t.handlers.is_empty());
        let names: Vec<&[u8]> = t.timers.iter().map(|t| t.name.as_slice()).collect();
        assert_eq!(names, [b"*_stays".as_slice()]);
    }
    result(&mut e, START, 0, &[s("permanent"), z(5.0)]);
    nvse::plugins::dispatch(&mut e, msg::NEW_GAME, 0, 0);
    assert_eq!(left(&mut e, "permanent"), -1.0);
    assert_eq!(left(&mut e, "*_stays"), 5.0);
}
