// SPDX-License-Identifier: GPL-3.0-only
//! The runner's place in the game's frame. The game's own functions (cell loads, placement) must run
//! on the game's main thread inside a frame: called from the host's thread between frames they wait
//! for loader threads while the host holds the kernel lock (the first try of this slice spun there
//! for minutes). So the runner is called at the entry of `Main::OnIdle` (`0086e650`): this module
//! registers a function there that runs the runner and then the translation itself.

use crate::Runner;
use engine::prelude::*;
use engine::units::fallout_misc::main::fn_0086e650;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// `Main::OnIdle`.
const ON_IDLE: u32 = 0x0086_e650;
/// Not ended.
const RUNNING: i32 = i32::MIN;

static RUNNER: Mutex<Option<(Runner, Instant)>> = Mutex::new(None);
static EXIT: AtomicI32 = AtomicI32::new(RUNNING);

/// `main`'s call of `StartMenu::Create(0, 0)` (`007053f0`, no argument) before its message loop.
const START_MENU_CREATE_AT_START: u32 = 0x0070_53f0;

/// Before the game's code starts ([`host::fork_prestart`]): no main menu is built (so no menu music or menu assets are
/// loaded either); the runner sends the player to the first place from the first frame. Only called for `NV_SCENE_MENU=0`.
pub fn skip_main_menu() {
    host::fork_prestart::add(|e| e.register(START_MENU_CREATE_AT_START, |_, _| Ret::default()));
}

/// `MoviePlayer::StartSequence` (`00ec16d0`, thiscall): `main`'s start of the intro movie sequence (it creates the sequence thread
/// and waits for it).
const MOVIE_START_SEQUENCE: u32 = 0x00ec_16d0;

/// Before the game's code starts: the intro movie sequence is not started. Measured in docs/fork/SCENE.md ("No main menu").
pub fn skip_intro_movie() {
    host::fork_prestart::add(|e| e.register(MOVIE_START_SEQUENCE, |_, _| Ret::default()));
}

/// Registers the runner at `Main::OnIdle`. Call once, after the host started (real threads take the
/// registration too) and before main's next frame.
pub fn install(e: &mut Engine, runner: Runner) {
    crate::view::install(e);
    *RUNNER.lock().unwrap_or_else(|p| p.into_inner()) = Some((runner, Instant::now()));
    e.register(ON_IDLE, |e, a| {
        {
            let mut slot = RUNNER.lock().unwrap_or_else(|p| p.into_inner());
            if let Some((runner, started)) = slot.as_mut() {
                if EXIT.load(Ordering::Relaxed) == RUNNING {
                    if let Some(status) = runner.frame(e, started.elapsed().as_secs_f64()) {
                        EXIT.store(status, Ordering::Relaxed);
                    }
                }
            }
        }
        fn_0086e650(e, Ptr::new(a[0]));
        Ret::default()
    });
}

/// The status the scene run ends with, once it has ended; the viewer's loop checks it after each frame.
pub fn exit_status() -> Option<i32> {
    match EXIT.load(Ordering::Relaxed) {
        RUNNING => None,
        status => Some(status),
    }
}
