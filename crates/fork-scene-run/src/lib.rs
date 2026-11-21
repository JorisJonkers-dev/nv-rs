// SPDX-License-Identifier: GPL-3.0-only
//! The engine's side of a scene run (nv-rs#479): between two of main's frames the runner sends the
//! player to each place of the request the game's own way and says when it has arrived. The state
//! machine ([`Runner`]) is pure and tested without a game; [`EngineWorld`] is the one piece that
//! calls the game's functions.

mod hook;
mod world;

pub use hook::{exit_status, install};
pub use world::EngineWorld;

use fork_scene::{Place, Request};

/// What main's frame shows (the viewer maps `host::uncap::Screen` to this).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Play,
    Menu,
    Loading,
}

/// What the runner needs of the game; [`EngineWorld`] is the real one.
pub trait World {
    fn screen(&self) -> Screen;
    /// Sends the player to the place through the game's own exterior placement.
    fn go_to(&mut self, place: &Place) -> Result<(), String>;
    /// The player's position and the grid cell of its parent cell's coordinates, when it has a cell.
    fn player(&self) -> Option<Standing>;
}

/// Where the player is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Standing {
    pub pos: [f64; 3],
}

/// What the viewer's loop does next.
#[derive(Debug, PartialEq)]
pub enum Step {
    /// Run the next frame.
    Continue,
    /// The place was reached (logged): its name and the position the player stands at.
    Reached { place: String, pos: [f64; 3] },
    /// Every place has been reached.
    Finished,
    /// A condition failed: the one plain line the run ends with.
    Failed(String),
}

/// The exit status of a scene run that failed (the table in docs/fork/PLAY-MODE.md).
pub const EXIT_SCENE_FAILED: i32 = 70;

#[derive(Debug, PartialEq)]
enum Phase {
    /// Waiting for the main menu to be up so the game can be sent from it.
    WaitMenu,
    /// `go_to` was called at this time (seconds); waiting for the player to stand in the cell.
    Going {
        since: f64,
    },
    Done,
}

/// Visits the request's places in order.
pub struct Runner {
    request: Request,
    next: usize,
    phase: Phase,
    /// Frames the screen has shown the menu (or play, for a later place) since the last send.
    steady: u32,
}

/// Frames the screen must have been steady before a place is sent: the game's first frames still
/// start its threads and read its data.
pub const STEADY_FRAMES: u32 = 120;

impl Runner {
    pub fn new(request: Request) -> Runner {
        Runner {
            request,
            next: 0,
            phase: Phase::WaitMenu,
            steady: 0,
        }
    }

    pub fn from_env() -> Result<Option<Runner>, String> {
        Ok(Request::from_env()?.map(Runner::new))
    }

    pub fn request(&self) -> &Request {
        &self.request
    }

    /// One call per `Main::OnIdle`, on the game's main thread inside its frame ([`install`]); returns
    /// the status the run ends with when it ends now (0 when every place was reached,
    /// [`EXIT_SCENE_FAILED`] with the plain line logged when a condition failed).
    pub fn frame(&mut self, engine: &mut engine_core::Engine, now: f64) -> Option<i32> {
        let screen = match host::uncap::screen(engine) {
            host::uncap::Screen::Play => Screen::Play,
            host::uncap::Screen::Menu => Screen::Menu,
            host::uncap::Screen::Loading => Screen::Loading,
        };
        match self.step(&mut EngineWorld { engine, screen }, now) {
            Step::Continue => None,
            Step::Reached { place, pos } => {
                fork_log::info!(target: "scene", "scene: place {place} reached; the player stands at {:.1} {:.1} {:.1}", pos[0], pos[1], pos[2]);
                None
            }
            Step::Finished => {
                fork_log::info!(target: "scene", "scene: every place reached; exit status 0");
                Some(0)
            }
            Step::Failed(line) => {
                fork_log::fatal!(target: "scene", "{line}");
                Some(EXIT_SCENE_FAILED)
            }
        }
    }

    /// One call between main's frames; `now` is a clock in seconds.
    pub fn step(&mut self, w: &mut dyn World, now: f64) -> Step {
        let Some(place) = self.request.places.get(self.next) else {
            self.phase = Phase::Done;
            return Step::Finished;
        };
        match self.phase {
            Phase::Done => Step::Finished,
            Phase::WaitMenu => {
                // The first place is sent from the main menu; a later one from play.
                let ready = if self.next == 0 {
                    w.screen() == Screen::Menu
                } else {
                    w.screen() == Screen::Play
                };
                self.steady = if ready { self.steady + 1 } else { 0 };
                if self.steady < STEADY_FRAMES {
                    return Step::Continue;
                }
                self.steady = 0;
                fork_log::info!(target: "scene", "scene: sending the player to {} from the {:?} screen", place.name, w.screen());
                match w.go_to(place) {
                    Ok(()) => {
                        self.phase = Phase::Going { since: now };
                        Step::Continue
                    }
                    Err(e) => Step::Failed(format!("scene: place {}: {e}", place.name)),
                }
            }
            Phase::Going { since } => {
                if now - since > f64::from(self.request.timeout_s) {
                    return Step::Failed(format!(
                        "scene: place {} was not reached within {} s (screen {:?})",
                        place.name,
                        self.request.timeout_s,
                        w.screen()
                    ));
                }
                if w.screen() != Screen::Play {
                    return Step::Continue;
                }
                let Some(at) = w.player() else {
                    return Step::Continue;
                };
                let cell = (
                    (at.pos[0] / 4096.0).floor() as i32,
                    (at.pos[1] / 4096.0).floor() as i32,
                );
                if cell != place.cell() {
                    return Step::Continue;
                }
                let name = place.name.clone();
                self.next += 1;
                self.phase = Phase::WaitMenu;
                Step::Reached {
                    place: name,
                    pos: at.pos,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fork_scene::PlaceFile;

    struct Fake {
        screen: Screen,
        standing: Option<Standing>,
        sent: Vec<String>,
        refuse: bool,
    }
    impl World for Fake {
        fn screen(&self) -> Screen {
            self.screen
        }
        fn go_to(&mut self, p: &Place) -> Result<(), String> {
            if self.refuse {
                return Err("no world".into());
            }
            self.sent.push(p.name.clone());
            Ok(())
        }
        fn player(&self) -> Option<Standing> {
            self.standing
        }
    }

    fn request(timeout_s: u32) -> Request {
        let f = PlaceFile::parse(
            "version = 1\n[[place]]\nname = \"a\"\nworld = \"W\"\nx = 100\ny = 100\nz = 5\n\
             [[place]]\nname = \"b\"\nworld = \"W\"\nx = 5000\ny = 100\nz = 5\n",
        )
        .unwrap();
        Request {
            places: f.places,
            out: None,
            settle_frames: 1,
            timeout_s,
        }
    }

    /// Steps until the place is sent (the screen steady for [`STEADY_FRAMES`] frames).
    fn send(r: &mut Runner, w: &mut Fake, now: f64) {
        for _ in 0..STEADY_FRAMES {
            r.step(w, now);
        }
    }

    fn fake(screen: Screen) -> Fake {
        Fake {
            screen,
            standing: None,
            sent: vec![],
            refuse: false,
        }
    }

    #[test]
    fn waits_for_the_menu_then_sends_the_first_place_once() {
        let mut r = Runner::new(request(60));
        let mut w = fake(Screen::Loading);
        assert_eq!(r.step(&mut w, 0.0), Step::Continue);
        assert!(w.sent.is_empty());
        w.screen = Screen::Menu;
        for _ in 0..STEADY_FRAMES - 1 {
            assert_eq!(r.step(&mut w, 1.0), Step::Continue);
        }
        assert!(w.sent.is_empty(), "not before the screen has been steady");
        assert_eq!(r.step(&mut w, 2.0), Step::Continue);
        assert_eq!(r.step(&mut w, 2.0), Step::Continue);
        assert_eq!(w.sent, ["a"]);
    }

    #[test]
    fn arrival_needs_play_and_the_cell() {
        let mut r = Runner::new(request(60));
        let mut w = fake(Screen::Menu);
        send(&mut r, &mut w, 0.0);
        w.screen = Screen::Loading;
        w.standing = Some(Standing {
            pos: [100.0, 100.0, 5.0],
        });
        assert_eq!(r.step(&mut w, 1.0), Step::Continue);
        w.screen = Screen::Play;
        w.standing = Some(Standing {
            pos: [9000.0, 100.0, 5.0],
        });
        assert_eq!(r.step(&mut w, 2.0), Step::Continue);
        w.standing = Some(Standing {
            pos: [100.0, 100.0, 5.0],
        });
        assert_eq!(
            r.step(&mut w, 3.0),
            Step::Reached {
                place: "a".into(),
                pos: [100.0, 100.0, 5.0]
            }
        );
    }

    #[test]
    fn the_second_place_is_sent_from_play_and_the_run_finishes() {
        let mut r = Runner::new(request(60));
        let mut w = fake(Screen::Menu);
        send(&mut r, &mut w, 0.0);
        w.screen = Screen::Play;
        w.standing = Some(Standing {
            pos: [100.0, 100.0, 5.0],
        });
        assert!(matches!(r.step(&mut w, 1.0), Step::Reached { .. }));
        send(&mut r, &mut w, 2.0);
        assert_eq!(w.sent, ["a", "b"]);
        w.standing = Some(Standing {
            pos: [5000.0, 100.0, 5.0],
        });
        assert!(matches!(r.step(&mut w, 3.0), Step::Reached { place, .. } if place == "b"));
        assert_eq!(r.step(&mut w, 4.0), Step::Finished);
    }

    #[test]
    fn a_place_not_reached_in_time_fails_with_one_plain_line() {
        let mut r = Runner::new(request(10));
        let mut w = fake(Screen::Menu);
        send(&mut r, &mut w, 0.0);
        w.screen = Screen::Loading;
        assert_eq!(r.step(&mut w, 5.0), Step::Continue);
        let Step::Failed(line) = r.step(&mut w, 11.0) else {
            panic!("no failure")
        };
        assert!(
            line.contains("place a") && line.contains("10 s") && !line.contains('\n'),
            "{line}"
        );
    }

    #[test]
    fn a_refused_send_fails_naming_the_place() {
        let mut r = Runner::new(request(10));
        let mut w = fake(Screen::Menu);
        w.refuse = true;
        for _ in 1..STEADY_FRAMES {
            r.step(&mut w, 0.0);
        }
        assert_eq!(
            r.step(&mut w, 0.0),
            Step::Failed("scene: place a: no world".into())
        );
    }
}
