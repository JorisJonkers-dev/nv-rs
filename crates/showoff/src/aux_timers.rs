// SPDX-License-Identifier: GPL-3.0-only
//! ShowOff xNVSE's auxiliary timers (group 6 of the profile's missing commands: timers; 42 calls
//! in one plugin of the profile). Written from the commands' GECK Wiki pages (see `NOTICE`,
//! `provenance/06-aux-timers.md`); no plugin code was read.
//!
//! An auxiliary timer is a named countdown on an owner form:
//! - `AuxTimerStart name [time] [flags] [owner]` makes it, or restarts one that exists (a paused
//!   one runs again); `AuxTimerStop name [fireEvent] [owner]` (alias `AuxTimerErase`) erases it;
//!   `AuxTimerPaused`, `AuxTimerTimeElapsed`, `AuxTimerTimeToCountdown` and `AuxTimerTimeLeft`
//!   read it (-1 for a timer that does not exist);
//! - `SetOnAuxTimerStartHandler`, `SetOnAuxTimerStopHandler` and `SetOnAuxTimerUpdateHandler`
//!   (aliases `SetOnTimer...Handler`) add or remove a function called with the timer's name and
//!   owner (and, for the update, the time counted) when such a timer starts, stops or counts.
//!
//! The owner is the form argument, else the calling reference, else `PlayerRef`. The name follows
//! the auxiliary variables' rule: a leading `*` makes the timer temporary, a `_` after it shares
//! it between mods (else it belongs to the calling script's mod). Names compare without case.
//!
//! Every frame (xNVSE's `MainGameLoop`), each timer that may run counts down: by one in a
//! frame-based timer, else by the frame's seconds: the game's own step (`fDeltaUnmodified`) when
//! it ignores the time multiplier in this mode, the step times the global multiplier (`fDelta`)
//! when it ignores turbo, and that times V.A.T.S.'s multiplier otherwise. A timer that runs out
//! fires its stop event and is erased, or, with the auto-restart flag, starts again and fires its
//! start event. A timer whose owner is gone is erased without an event.
//!
//! What the pages leave open is written down in the provenance note. One part is not built yet:
//! permanent timers (a name without `*`) are not written into the co-save; they last until the
//! next load, new game or main menu.

use crate::commands::Cmd;
use crate::State;
use engine_core::prelude::*;
use nvse::args::Arg;
use nvse::commands::Read;
use nvse::plugins::{msg, Message};
use nvse::table::{Param, Returns};
use nvse::{handled, CommandArgs};

/// The flags of a timer (the `AuxTimerStart` page).
pub mod flag {
    pub const RUN_IN_MENU_MODE: u32 = 1;
    pub const RUN_IN_GAME_MODE: u32 = 2;
    pub const COUNT_IN_SECONDS: u32 = 4;
    pub const AUTO_REMOVE_ON_LOAD: u32 = 8;
    pub const AUTO_RESTARTS: u32 = 16;
    pub const IS_PAUSED: u32 = 32;
    pub const IGNORE_TIME_MULT_IN_MENU_MODE: u32 = 64;
    pub const NOT_WHEN_PAUSED: u32 = 128;
    pub const RUN_UPDATE_EVENT: u32 = 256;
    pub const IGNORE_TURBO: u32 = 512;
    pub const IGNORE_TIME_MULT_IN_GAME_MODE: u32 = 1024;
    /// The page's default: menu mode, game mode, in seconds.
    pub const DEFAULT: u32 = RUN_IN_MENU_MODE | RUN_IN_GAME_MODE | COUNT_IN_SECONDS;
}

/// `PlayerRef`'s form id: the owner when none is given.
pub const PLAYER_REF_ID: u32 = 0x14;

/// Which timer: temporary or permanent, the mod it belongs to (0xFF: shared), the owner's form
/// id, the name in lower case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub temp: bool,
    pub mod_index: u32,
    pub owner: u32,
    pub name: Vec<u8>,
}

/// One timer.
#[derive(Clone, Debug, PartialEq)]
pub struct Timer {
    pub key: Key,
    /// The name as it was first given (the events pass it).
    pub name: Vec<u8>,
    /// The time it was told to count down from.
    pub countdown: f64,
    /// The time left.
    pub left: f64,
    pub flags: u32,
}

/// Which event a handler listens to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Start,
    Stop,
    Update,
}

/// A handler: the function, the event, the timer.
#[derive(Clone, Debug, PartialEq)]
pub struct Handler {
    pub event: Event,
    pub key: Key,
    pub script: u32,
}

/// The timers and their handlers, in the order they were made.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Timers {
    pub timers: Vec<Timer>,
    pub handlers: Vec<Handler>,
}

/// `kParamType_String`, `kParamType_Integer`, `kParamType_AnyForm`, `kParamType_Double`.
const STRING: u32 = 0x00;
const INT: u32 = 0x01;
const ANY_FORM: u32 = 0x3d;
const DOUBLE: u32 = 0x2c;

const fn param(name: &'static str, kind: u32, optional: bool) -> Param {
    Param {
        name,
        kind,
        optional,
    }
}

/// The parameter lists of the declarations.
const START: &[Param] = &[
    param("string", STRING, false),
    param("double", DOUBLE, true),
    param("int", INT, true),
    param("form", ANY_FORM, true),
];
const NAME_INT_FORM: &[Param] = &[
    param("string", STRING, false),
    param("int", INT, true),
    param("form", ANY_FORM, true),
];
const NAME_FORM: &[Param] = &[
    param("string", STRING, false),
    param("form", ANY_FORM, true),
];
const EVENT: &[Param] = &[
    param("setOrRemove", INT, false),
    param("script", ANY_FORM, false),
    param("string", STRING, false),
    param("form", ANY_FORM, true),
];

const fn cmd(
    opcode: u32,
    name: &'static str,
    short: &'static str,
    params: &'static [Param],
    execute: AbiFn,
) -> Cmd {
    Cmd {
        opcode,
        name,
        short,
        params,
        execute,
        eval: None,
        needs_ref: false,
        expression: false,
        returns: Returns::Default,
    }
}

/// The commands, in the order of their opcodes.
pub static COMMANDS: [Cmd; 9] = [
    cmd(0x3d3e, "AuxTimerStart", "", START, aux_timer_start),
    cmd(
        0x3d3f,
        "AuxTimerStop",
        "AuxTimerErase",
        NAME_INT_FORM,
        aux_timer_stop,
    ),
    cmd(
        0x3d40,
        "AuxTimerPaused",
        "",
        NAME_INT_FORM,
        aux_timer_paused,
    ),
    cmd(
        0x3d41,
        "AuxTimerTimeElapsed",
        "",
        NAME_INT_FORM,
        aux_timer_time_elapsed,
    ),
    cmd(
        0x3d42,
        "AuxTimerTimeToCountdown",
        "",
        NAME_FORM,
        aux_timer_time_to_countdown,
    ),
    cmd(
        0x3d43,
        "AuxTimerTimeLeft",
        "",
        NAME_FORM,
        aux_timer_time_left,
    ),
    cmd(
        0x3d44,
        "SetOnAuxTimerStartHandler",
        "SetOnTimerStartHandler",
        EVENT,
        set_on_start,
    ),
    cmd(
        0x3d45,
        "SetOnAuxTimerStopHandler",
        "SetOnTimerStopHandler",
        EVENT,
        set_on_stop,
    ),
    cmd(
        0x3d4c,
        "SetOnAuxTimerUpdateHandler",
        "SetOnTimerUpdateHandler",
        EVENT,
        set_on_update,
    ),
];

/// `TimeGlobal` (a `BSTimer`): `fDelta` (the step times the global multiplier) and
/// `fDeltaUnmodified` (the step).
const TIME_GLOBAL: u32 = 0x011f_6394;
const DELTA: u32 = 0xc;
const DELTA_UNMODIFIED: u32 = 0x10;
/// V.A.T.S.'s time multiplier (`thiscall`, returns a `double`) and its object.
const VATS_TIME_MULT: u32 = 0x009c_8cc0;
const VATS_TIME_OBJECT: u32 = 0x011f_2250;
/// `MenuMode`; the start menu (`g_startMenu`); the console's getter and `IsConsoleOpen`.
const MENU_MODE: u32 = 0x0070_2360;
const START_MENU: u32 = 0x011d_aac0;
const CONSOLE_INSTANCE: u32 = 0x0071_b160;
const CONSOLE_IS_OPEN: u32 = 0x004a_4020;
/// `LookupFormByID`.
pub const LOOKUP_FORM: u32 = 0x0048_39c0;

fn timers(e: &mut Engine) -> &mut Timers {
    &mut e.ext::<State>().timers
}

/// The mod index of the last file in the form's list of files (`TESForm`'s `+0x10`, the file's
/// index at `+0x40C`), 0xFF for none.
fn overriding_mod(e: &Engine, form: u32) -> u32 {
    if form == 0 {
        return 0xff;
    }
    let mut node = form + 0x10;
    while e.mem.u32(node + 4) != 0 {
        node = e.mem.u32(node + 4);
    }
    let file = e.mem.u32(node);
    if file == 0 {
        0xff
    } else {
        u32::from(e.mem.u8(file + 0x40c))
    }
}

/// The timer a command names: the name's scope, the owner (the form argument, else the calling
/// reference, else `PlayerRef`).
fn key_of(e: &Engine, a: &CommandArgs, name: &[u8], owner: u32) -> Key {
    let temp = name.first() == Some(&b'*');
    let shared = name.get(usize::from(temp)) == Some(&b'_');
    let owner = match (owner, a.this_obj) {
        (0, 0) => PLAYER_REF_ID,
        (0, this) => e.mem.u32(this + 0xc),
        (form, _) => e.mem.u32(form + 0xc),
    };
    Key {
        temp,
        mod_index: if shared {
            0xff
        } else {
            overriding_mod(e, a.script)
        },
        owner,
        name: name.to_ascii_lowercase(),
    }
}

/// A number argument as a `double` (an omitted one: `default`).
fn number(read: &Read, i: usize, default: f64) -> f64 {
    match read.args.get(i) {
        Some(Arg::Double(v)) => *v,
        Some(Arg::Float(v)) => f64::from(*v),
        Some(Arg::Int(v)) => f64::from(*v),
        _ => default,
    }
}

/// An integer argument as a signed value (an omitted one: `default`).
fn integer(read: &Read, i: usize, default: i32) -> i32 {
    match read.args.get(i) {
        Some(Arg::Int(v)) => *v,
        _ => default,
    }
}

fn find(t: &Timers, key: &Key) -> Option<usize> {
    t.timers.iter().position(|x| x.key == *key)
}

/// A timer's value, or -1 for none.
fn read_timer(e: &mut Engine, w: &[u32], form_at: usize, value: fn(&Timer) -> f64) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, -1.0);
    let read = Read::of(e, &a);
    if !read.ok {
        return handled(true);
    }
    let key = key_of(e, &a, &read.text(0), read.form(form_at));
    let t = timers(e);
    if let Some(i) = find(t, &key) {
        let v = value(&t.timers[i]);
        a.set_result(e, v);
    }
    handled(true)
}

/// `AuxTimerStart name [time] [flags] [owner]`: 1 when a timer was started or restarted.
fn aux_timer_start(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if !read.ok {
        return handled(true);
    }
    let name = read.text(0);
    let time = number(&read, 1, -1.0);
    let flags = integer(&read, 2, flag::DEFAULT as i32) as u32;
    let key = key_of(e, &a, &name, read.form(3));
    let t = timers(e);
    let started = match find(t, &key) {
        Some(i) => {
            let timer = &mut t.timers[i];
            if time >= 0.0 {
                timer.countdown = time;
            }
            timer.left = timer.countdown;
            timer.flags = flags & !flag::IS_PAUSED;
            true
        }
        None if time >= 0.0 => {
            t.timers.push(Timer {
                key: key.clone(),
                name: name.clone(),
                countdown: time,
                left: time,
                flags,
            });
            true
        }
        None => false,
    };
    if started {
        a.set_result(e, 1.0);
        fire(e, &[(Event::Start, key, name, 0.0)]);
    }
    handled(true)
}

/// `AuxTimerStop name [fireEvent] [owner]`: 1 when a timer was erased; its stop event only with
/// `fireEvent`.
fn aux_timer_stop(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if !read.ok {
        return handled(true);
    }
    let fire_event = integer(&read, 1, 0) != 0;
    let key = key_of(e, &a, &read.text(0), read.form(2));
    let t = timers(e);
    if let Some(i) = find(t, &key) {
        let gone = t.timers.remove(i);
        a.set_result(e, 1.0);
        if fire_event {
            fire(e, &[(Event::Stop, gone.key, gone.name, 0.0)]);
        }
    }
    handled(true)
}

/// `AuxTimerPaused name [setPaused] [owner]`: 1 paused, 0 running, -1 none; then a `setPaused`
/// of 0 or 1 sets it.
fn aux_timer_paused(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, -1.0);
    let read = Read::of(e, &a);
    if !read.ok {
        return handled(true);
    }
    let set = integer(&read, 1, -1);
    let key = key_of(e, &a, &read.text(0), read.form(2));
    let t = timers(e);
    if let Some(i) = find(t, &key) {
        let timer = &mut t.timers[i];
        let was = timer.flags & flag::IS_PAUSED != 0;
        match set {
            0 => timer.flags &= !flag::IS_PAUSED,
            1 => timer.flags |= flag::IS_PAUSED,
            _ => {}
        }
        a.set_result(e, f64::from(u8::from(was)));
    }
    handled(true)
}

fn aux_timer_time_elapsed(e: &mut Engine, w: &[u32]) -> Ret {
    read_timer(e, w, 2, |t| t.countdown - t.left)
}

fn aux_timer_time_to_countdown(e: &mut Engine, w: &[u32]) -> Ret {
    read_timer(e, w, 1, |t| t.countdown)
}

fn aux_timer_time_left(e: &mut Engine, w: &[u32]) -> Ret {
    read_timer(e, w, 1, |t| t.left)
}

/// `SetOnAuxTimer...Handler setOrRemove script name [owner]`.
fn set_handler(e: &mut Engine, w: &[u32], event: Event) -> Ret {
    let a = CommandArgs::new(w);
    let read = Read::of(e, &a);
    if !read.ok {
        return handled(true);
    }
    let add = read.int(0, 0) != 0;
    let script = read.form(1);
    if script == 0 {
        return handled(true);
    }
    let key = key_of(e, &a, &read.text(2), read.form(3));
    let handler = Handler { event, key, script };
    let t = timers(e);
    let at = t.handlers.iter().position(|h| *h == handler);
    match (add, at) {
        (true, None) => {
            t.handlers.push(handler);
            nvse::lambda::save_variables(e, script);
        }
        (false, Some(i)) => {
            t.handlers.remove(i);
            nvse::lambda::unsave_variables(e, script);
        }
        _ => {}
    }
    handled(true)
}

fn set_on_start(e: &mut Engine, w: &[u32]) -> Ret {
    set_handler(e, w, Event::Start)
}

fn set_on_stop(e: &mut Engine, w: &[u32]) -> Ret {
    set_handler(e, w, Event::Stop)
}

fn set_on_update(e: &mut Engine, w: &[u32]) -> Ret {
    set_handler(e, w, Event::Update)
}

/// Calls the handlers of each event, in order: with the timer's name and owner, and for an update
/// the time counted.
fn fire(e: &mut Engine, events: &[(Event, Key, Vec<u8>, f64)]) {
    for (event, key, name, delta) in events {
        let scripts: Vec<u32> = timers(e)
            .handlers
            .iter()
            .filter(|h| h.event == *event && h.key == *key)
            .map(|h| h.script)
            .collect();
        if scripts.is_empty() {
            continue;
        }
        let Some(text) = nvse::table::try_guest_str(e, name) else {
            continue;
        };
        let owner = e.call(LOOKUP_FORM, &args![key.owner]).u32();
        let mut words = vec![text, owner];
        if *event == Event::Update {
            words.push((*delta as f32).to_bits());
        }
        for script in scripts {
            call(e, script, words.clone());
        }
    }
}

/// Calls `script` on no reference with `args` (xNVSE's script interface).
#[cfg(not(test))]
fn call(e: &mut Engine, script: u32, args: Vec<u32>) {
    nvse::functions::call(e, nvse::functions::Caller::internal(script, 0, 0, args));
}

#[cfg(test)]
thread_local! {
    static CALLS: std::cell::RefCell<Vec<(u32, Vec<u32>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The tests' stand-in: the call recorded.
#[cfg(test)]
fn call(_: &mut Engine, script: u32, args: Vec<u32>) {
    CALLS.with(|c| c.borrow_mut().push((script, args)));
}

/// The calls recorded since the last look (script, arguments).
#[cfg(test)]
pub fn calls() -> Vec<(u32, Vec<u32>)> {
    CALLS.with(|c| std::mem::take(&mut *c.borrow_mut()))
}

/// The start menu or the console is open (`IsGamePaused`, as xNVSE's tick reads it).
fn is_paused(e: &mut Engine) -> bool {
    if e.mem.u32(START_MENU) != 0 {
        return true;
    }
    let console = e.call(CONSOLE_INSTANCE, &args![1u32]).u32();
    console != 0 && e.call(CONSOLE_IS_OPEN, &args![console]).bool()
}

/// One frame of every timer.
pub fn tick(e: &mut Engine) {
    if timers(e).timers.is_empty() {
        return;
    }
    let menu = e.call(MENU_MODE, &[]).bool();
    let paused = is_paused(e);
    let delta = f64::from(e.mem.f32(TIME_GLOBAL + DELTA));
    let unmodified = f64::from(e.mem.f32(TIME_GLOBAL + DELTA_UNMODIFIED));
    let vats = e.call(VATS_TIME_MULT, &args![VATS_TIME_OBJECT]).f64();
    let owners: Vec<u32> = timers(e).timers.iter().map(|t| t.key.owner).collect();
    let alive: Vec<bool> = owners
        .into_iter()
        .map(|id| e.call(LOOKUP_FORM, &args![id]).u32() != 0)
        .collect();
    let mut events = Vec::new();
    let mut kept = Vec::new();
    let all = std::mem::take(&mut timers(e).timers);
    for (mut t, alive) in all.into_iter().zip(alive) {
        if !alive {
            continue;
        }
        let runs = t.flags & flag::IS_PAUSED == 0
            && t.flags
                & if menu {
                    flag::RUN_IN_MENU_MODE
                } else {
                    flag::RUN_IN_GAME_MODE
                }
                != 0
            && !(paused && t.flags & flag::NOT_WHEN_PAUSED != 0);
        if !runs {
            kept.push(t);
            continue;
        }
        let step = if t.flags & flag::COUNT_IN_SECONDS == 0 {
            1.0
        } else if t.flags
            & if menu {
                flag::IGNORE_TIME_MULT_IN_MENU_MODE
            } else {
                flag::IGNORE_TIME_MULT_IN_GAME_MODE
            }
            != 0
        {
            unmodified
        } else if t.flags & flag::IGNORE_TURBO != 0 {
            delta
        } else {
            delta * vats
        };
        t.left -= step;
        if t.flags & flag::RUN_UPDATE_EVENT != 0 {
            events.push((Event::Update, t.key.clone(), t.name.clone(), step));
        }
        if t.left > 0.0 {
            kept.push(t);
            continue;
        }
        events.push((Event::Stop, t.key.clone(), t.name.clone(), 0.0));
        if t.flags & flag::AUTO_RESTARTS != 0 {
            t.left = t.countdown;
            events.push((Event::Start, t.key.clone(), t.name.clone(), 0.0));
            kept.push(t);
        } else {
            t.left = 0.0;
        }
    }
    // Timers a handler made while the list was out stay after the kept ones.
    let made = std::mem::take(&mut timers(e).timers);
    kept.extend(made);
    timers(e).timers = kept;
    fire(e, &events);
}

/// A load or the main menu: every handler goes, and the timers that do not outlast it (the
/// temporary ones with the auto-remove flag; the permanent ones, which a save would bring back).
fn clear_for_load(e: &mut Engine) {
    let handlers = std::mem::take(&mut timers(e).handlers);
    for h in handlers {
        nvse::lambda::unsave_variables(e, h.script);
    }
    drop_permanent_and_auto_removed(e);
}

fn drop_permanent_and_auto_removed(e: &mut Engine) {
    timers(e)
        .timers
        .retain(|t| t.key.temp && t.flags & flag::AUTO_REMOVE_ON_LOAD == 0);
}

/// The plugin's listener: the frame, a load, the main menu, a new game.
pub fn listen(e: &mut Engine, m: &Message) {
    match m.kind {
        msg::MAIN_GAME_LOOP => tick(e),
        msg::PRE_LOAD_GAME | msg::EXIT_TO_MAIN_MENU => clear_for_load(e),
        // A new game has none of a save's permanent timers.
        msg::NEW_GAME => timers(e).timers.retain(|t| t.key.temp),
        _ => {}
    }
}

#[cfg(test)]
#[path = "aux_timers_tests.rs"]
mod tests;
