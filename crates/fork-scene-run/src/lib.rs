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
    /// The id of the top menu (`Interface::GetTopMenuID`), 0 for none.
    fn top_menu(&mut self) -> u32 {
        0
    }
    /// Holds the world still (idempotent, called every frame once the player has been sent): the
    /// game hour and a stopped clock, the weather, no AI processing, the interface hidden.
    fn freeze(&mut self, _hour: f32, _weather: u32) {}
    /// Closes the message box on top (`MessageMenu::ClearMessageQueueHead`); false when the top
    /// menu is not a message box.
    fn dismiss_message(&mut self) -> bool {
        false
    }
    /// Asks the drawing layer to write the next present's frame as a BMP at `path`.
    fn ask_picture(&mut self, path: &std::path::Path);
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
    /// The place was reached and the world is ready (logged): its name and where the player stands;
    /// with a picture asked for, the PNG's path.
    Reached {
        place: String,
        pos: [f64; 3],
        picture: Option<std::path::PathBuf>,
    },
    /// Every place has been reached.
    Finished,
    /// A condition failed: the one plain line the run ends with.
    Failed(String),
}

/// How far (game units) the player's height may move from where the count started and still be standing.
pub const STAND_TOLERANCE: f64 = 0.25;

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
    /// In the place's cell: ready once the player has stood still for `settle_frames` frames.
    Settling {
        since: f64,
        last_z: f64,
        stable: u32,
    },
    /// The picture was asked for; waiting for the frame's file.
    Waiting {
        since: f64,
        bmp: std::path::PathBuf,
        pos: [f64; 3],
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
    tick: u64,
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
            tick: 0,
        }
    }

    pub fn from_env() -> Result<Option<Runner>, String> {
        Ok(Request::from_env()?.map(Runner::new))
    }

    pub fn request(&self) -> &Request {
        &self.request
    }

    /// Before the engine starts: with a picture folder asked for, the drawing layer keeps its frame
    /// readback on (`NV_SHOT`, nothing captured by itself: `NV_SHOT_LIST=0`) so [`Runner::frame`] can
    /// ask for one frame.
    pub fn prepare_environment(&self) {
        if let Some(dir) = &self.request.out {
            if std::env::var_os("NV_SHOT").is_none() {
                std::env::set_var("NV_SHOT", dir);
                std::env::set_var("NV_SHOT_LIST", "0");
            }
        }
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
            Step::Reached {
                place,
                pos,
                picture,
            } => {
                fork_log::info!(target: "scene", "scene: place {place} ready; the player stands at {:.1} {:.1} {:.1}{}", pos[0], pos[1], pos[2],
                    picture.map_or(String::new(), |p| format!("; picture {}", p.display())));
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
        // New-game items and quest messages open message boxes the whole time; they end every
        // place's wait unless closed (the player is in the world once a place has been sent).
        if (self.next > 0 || !matches!(self.phase, Phase::WaitMenu))
            && w.screen() == Screen::Menu
            && w.dismiss_message()
        {
            fork_log::info!(target: "scene", "scene: a message box was closed");
            self.steady = 0;
            return Step::Continue;
        }
        if let Some((hour, weather)) = self.request.freeze {
            w.freeze(hour, weather);
        }
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
                self.phase = Phase::Settling {
                    since,
                    last_z: at.pos[2],
                    stable: 0,
                };
                Step::Continue
            }
            Phase::Settling {
                since,
                last_z,
                stable,
            } => {
                if now - since > f64::from(self.request.timeout_s) {
                    return Step::Failed(format!(
                        "scene: place {} was reached but the player did not stand still for {} frames within {} s",
                        place.name, self.request.settle_frames, self.request.timeout_s
                    ));
                }
                let at = match (w.screen(), w.player()) {
                    (Screen::Play, Some(at)) => at,
                    _ => {
                        if self.tick % 60 == 0 {
                            fork_log::debug!(target: "scene", "scene: settling interrupted, screen {:?}, top menu {:#x}", w.screen(), w.top_menu());
                        }
                        self.tick += 1;
                        self.phase = Phase::Settling {
                            since,
                            last_z,
                            stable: 0,
                        };
                        return Step::Continue;
                    }
                };
                self.tick += 1;
                if self.tick % 60 == 1 {
                    fork_log::debug!(target: "scene", "scene: settling at {:.2} {:.2} {:.2}, {stable} steady frames", at.pos[0], at.pos[1], at.pos[2]);
                }
                let stable = if (at.pos[2] - last_z).abs() < 0.05 {
                    stable + 1
                } else {
                    0
                };
                if stable < self.request.settle_frames {
                    // The anchor moves only when the count starts again: slow drift is not standing.
                    let anchor = if stable == 0 { at.pos[2] } else { last_z };
                    self.phase = Phase::Settling {
                        since,
                        last_z: anchor,
                        stable,
                    };
                    return Step::Continue;
                }
                match &self.request.out {
                    None => self.finish(place.name.clone(), at.pos, None),
                    Some(dir) => {
                        if let Err(e) = std::fs::create_dir_all(dir) {
                            return Step::Failed(format!("scene: {}: {e}", dir.display()));
                        }
                        let bmp = dir.join(format!("{}.bmp", place.name));
                        let _ = std::fs::remove_file(&bmp);
                        w.ask_picture(&bmp);
                        self.phase = Phase::Waiting {
                            since,
                            bmp,
                            pos: at.pos,
                        };
                        Step::Continue
                    }
                }
            }
            Phase::Waiting {
                since,
                ref bmp,
                pos,
            } => {
                if now - since > f64::from(self.request.timeout_s) {
                    return Step::Failed(format!(
                        "scene: place {}: no picture was written within {} s",
                        place.name, self.request.timeout_s
                    ));
                }
                // The file is written by the drawing thread; a partly written one reads as truncated.
                let Ok(bytes) = std::fs::read(bmp) else {
                    return Step::Continue;
                };
                let Ok(png) = fork_scene::png::bmp_to_png(&bytes) else {
                    return Step::Continue;
                };
                let out = bmp.with_extension("png");
                if let Err(e) = std::fs::write(&out, png) {
                    return Step::Failed(format!("scene: {}: {e}", out.display()));
                }
                let _ = std::fs::remove_file(bmp);
                self.finish(place.name.clone(), pos, Some(out))
            }
        }
    }

    fn finish(&mut self, name: String, pos: [f64; 3], picture: Option<std::path::PathBuf>) -> Step {
        self.next += 1;
        self.phase = Phase::WaitMenu;
        Step::Reached {
            place: name,
            pos,
            picture,
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
        asked: Vec<std::path::PathBuf>,
        /// What the "drawing layer" writes when asked for a picture.
        draw: Option<Vec<u8>>,
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
        fn ask_picture(&mut self, path: &std::path::Path) {
            self.asked.push(path.to_path_buf());
            if let Some(bytes) = &self.draw {
                std::fs::write(path, bytes).unwrap();
            }
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
            freeze: None,
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
            asked: vec![],
            draw: None,
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
        assert_eq!(r.step(&mut w, 3.0), Step::Continue, "now settling");
        assert_eq!(
            r.step(&mut w, 4.0),
            Step::Reached {
                place: "a".into(),
                pos: [100.0, 100.0, 5.0],
                picture: None
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
        r.step(&mut w, 1.0);
        assert!(matches!(r.step(&mut w, 1.5), Step::Reached { .. }));
        send(&mut r, &mut w, 2.0);
        assert_eq!(w.sent, ["a", "b"]);
        w.standing = Some(Standing {
            pos: [5000.0, 100.0, 5.0],
        });
        r.step(&mut w, 3.0);
        assert!(matches!(r.step(&mut w, 3.5), Step::Reached { place, .. } if place == "b"));
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

    fn bmp_2x1() -> Vec<u8> {
        let mut b = b"BM".to_vec();
        b.extend_from_slice(&62u32.to_le_bytes());
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(&54u32.to_le_bytes());
        b.extend_from_slice(&40u32.to_le_bytes());
        b.extend_from_slice(&2i32.to_le_bytes());
        b.extend_from_slice(&(-1i32).to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&32u16.to_le_bytes());
        b.extend_from_slice(&[0; 24]);
        b.extend_from_slice(&[1, 2, 3, 0, 4, 5, 6, 0]);
        b
    }

    #[test]
    fn a_picture_is_asked_for_only_once_the_player_stands_still_and_ends_as_a_png() {
        let dir = std::env::temp_dir().join(format!("scene-run-png-{}", std::process::id()));
        let mut req = request(60);
        req.out = Some(dir.clone());
        req.settle_frames = 3;
        let mut r = Runner::new(req);
        let mut w = fake(Screen::Menu);
        send(&mut r, &mut w, 0.0);
        w.screen = Screen::Play;
        w.standing = Some(Standing {
            pos: [100.0, 100.0, 5.0],
        });
        r.step(&mut w, 1.0); // in the cell: settling starts
        w.standing = Some(Standing {
            pos: [100.0, 100.0, 9.0],
        }); // still falling: the count starts again
        for t in 0..3 {
            r.step(&mut w, 2.0 + f64::from(t));
            assert!(w.asked.is_empty(), "not while the height still changes");
        }
        // Falls once more, then rests.
        w.standing = Some(Standing {
            pos: [100.0, 100.0, 8.0],
        });
        r.step(&mut w, 6.0);
        for t in 0..2 {
            assert_eq!(r.step(&mut w, 7.0 + f64::from(t)), Step::Continue);
            assert!(w.asked.is_empty());
        }
        assert_eq!(r.step(&mut w, 9.0), Step::Continue);
        assert_eq!(w.asked, [dir.join("a.bmp")]);
        // The drawing layer has not written the frame yet.
        assert_eq!(r.step(&mut w, 10.0), Step::Continue);
        std::fs::write(dir.join("a.bmp"), bmp_2x1()).unwrap();
        let done = r.step(&mut w, 11.0);
        assert_eq!(
            done,
            Step::Reached {
                place: "a".into(),
                pos: [100.0, 100.0, 8.0],
                picture: Some(dir.join("a.png"))
            }
        );
        assert_eq!(&std::fs::read(dir.join("a.png")).unwrap()[..4], b"\x89PNG");
        assert!(!dir.join("a.bmp").exists(), "the BMP is removed");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn no_picture_in_time_fails_with_one_plain_line() {
        let dir = std::env::temp_dir().join(format!("scene-run-none-{}", std::process::id()));
        let mut req = request(10);
        req.out = Some(dir.clone());
        req.settle_frames = 1;
        let mut r = Runner::new(req);
        let mut w = fake(Screen::Menu);
        send(&mut r, &mut w, 0.0);
        w.screen = Screen::Play;
        w.standing = Some(Standing {
            pos: [100.0, 100.0, 5.0],
        });
        for t in 1..4 {
            r.step(&mut w, f64::from(t));
        }
        let Step::Failed(line) = r.step(&mut w, 11.0) else {
            panic!("no failure")
        };
        assert!(
            line.contains("no picture") && line.contains("10 s") && !line.contains('\n'),
            "{line}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
