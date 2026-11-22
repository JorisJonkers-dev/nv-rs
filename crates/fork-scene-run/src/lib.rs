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
    fn go_to(&mut self, place: &Place) -> Result<Target, String>;
    /// The player's position and the grid cell of its parent cell's coordinates, when it has a cell.
    fn player(&self) -> Option<Standing>;
    /// The id of the top menu (`Interface::GetTopMenuID`), 0 for none.
    fn top_menu(&mut self) -> u32 {
        0
    }
    /// Holds the world still (idempotent, called every frame once the player has been sent): the
    /// game hour and a stopped clock, the weather, no AI processing, the interface hidden.
    fn freeze(&mut self, _hour: f32, _weather: u32) {}
    /// Hides the 3D of every actor but the player in the cells around the player (`NiAVObject`
    /// culled flag); the number hidden now.
    fn hide_actors(&mut self) -> u32 {
        0
    }
    /// Sets the float game setting `name` (the game's own settings collection, looked up by name);
    /// `Ok(false)` while the collection does not exist yet, an error for a name or type it does not know.
    fn set_float_setting(&mut self, _name: &str, _value: f32) -> Result<bool, String> {
        Ok(true)
    }
    /// Moves the player to `pos` (the console's `setpos`: the game's own position setters, one frame's step of a walk
    /// along a place's `path`); the world then streams the cells around it as it does for a walking player.
    fn move_player(&mut self, _pos: [f64; 3]) -> Result<(), String> {
        Err("this world cannot move the player".into())
    }
    /// Closes the message box on top (`MessageMenu::ClearMessageQueueHead`); false when the top
    /// menu is not a message box.
    fn dismiss_message(&mut self) -> bool {
        false
    }
    /// Sets the time that drives the trees' and brush's sway to a fixed value (once, as the place is reached): the
    /// accumulator the tree update adds each frame's time to. Without it the phase of the sway at the picture depends
    /// on how many loading-screen frames the move took, which varies by one between runs (nv-rs#484).
    fn pin_sway_time(&mut self) {}
    /// Puts the free camera where the place says (the console's `tfc` without the console: the game's own
    /// fly camera, the player not simulated, nothing falls or reacts) and aims it; `standing` is where the
    /// player stands. Idempotent: called again every frame until the picture. Returns where the camera is
    /// and its heading and pitch.
    fn camera(&mut self, _place: &Place, _standing: [f64; 3]) -> Result<Target, String> {
        Err("this world has no free camera".into())
    }
    /// Asks the drawing layer to write the next present's frame as a BMP at `path`.
    fn ask_picture(&mut self, path: &std::path::Path);
}

/// Where a place turned out to be: a place by reference is resolved by the game's data when it is sent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    pub pos: [f64; 3],
    pub heading: f64,
    pub pitch: f64,
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

/// The frame count `NV_REPEATABLE` is given (the run ends before it, at its last place).
const REPEATABLE_FRAMES: &str = "100000000";

/// The game setting that starts the idle camera, and the seconds it is set to.
pub const VANITY_SETTING: &str = "fVanityModeAutoDelay";
pub const VANITY_OFF_SECONDS: f32 = 1_000_000.0;

/// Frames the free camera is held before the picture (or the soak) of a place that has one.
pub const CAMERA_FRAMES: u32 = 30;

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
    /// Running `left` more frames at the place, timing each (`last` is the clock at the frame before).
    Soaking {
        since: f64,
        left: u32,
        last: f64,
        pos: [f64; 3],
    },
    /// The player stands; the free camera is held in place for `left` more frames so the world has drawn
    /// from there (terrain and objects around the camera load on its first frames).
    Camera {
        since: f64,
        left: u32,
        pos: [f64; 3],
    },
    /// Ready; the picture is asked for when `place_frames` reaches `until` (a fixed frame after the send).
    Holding {
        since: f64,
        until: u64,
        pos: [f64; 3],
    },
    /// The picture was asked for; waiting for the frame's file.
    Waiting {
        since: f64,
        bmp: std::path::PathBuf,
        pos: [f64; 3],
    },
    Done,
}

/// Average, 99th percentile and largest of the frame times (milliseconds); sorts `ms`.
fn frame_stats(ms: &mut [f64]) -> (f64, f64, f64) {
    if ms.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    ms.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let avg = ms.iter().sum::<f64>() / ms.len() as f64;
    let p99 = ms[((ms.len() - 1) as f64 * 0.99).round() as usize];
    (avg, p99, ms[ms.len() - 1])
}

/// Visits the request's places in order.
pub struct Runner {
    request: Request,
    next: usize,
    phase: Phase,
    /// Frames the screen has shown the menu (or play, for a later place) since the last send.
    steady: u32,
    /// Where the place being visited resolved to.
    target: [f64; 3],
    /// The frame times (ms) of the soak in progress.
    ms: Vec<f64>,
    /// `NV_SCENE_TRACE=1`.
    trace: bool,
    /// With the trace: the previous frame's player position (bits), and after a send the position it left and the frames
    /// still to scan for words that hold it (nv-rs#474, #486: who writes the old position back).
    prev_bits: [u32; 3],
    scan: Option<([u32; 3], u32)>,
    just_sent: bool,
    /// The idle camera's setting is set.
    vanity_off: bool,
    /// Main frames since the current place was sent.
    place_frames: u64,
    /// Frames in a row the soak has not been on the play screen.
    away: u32,
    tick: u64,
    hidden: u32,
}

/// Frames the screen must have been steady before a place is sent: the game's first frames still
/// start its threads and read its data.
pub const STEADY_FRAMES: u32 = 120;

/// Frames the play screen must be steady before a later place is sent (the first is sent from the menu,
/// whose first frames start the game's threads; the next ones from a world that is running).
pub const LATER_STEADY_FRAMES: u32 = 5;

/// Frames a soak may spend off the play screen in a row before it fails.
pub const AWAY_FRAMES: u32 = 600;

impl Runner {
    pub fn new(request: Request) -> Runner {
        Runner {
            request,
            next: 0,
            phase: Phase::WaitMenu,
            steady: 0,
            target: [0.0; 3],
            ms: Vec::new(),
            away: 0,
            place_frames: 0,
            vanity_off: false,
            trace: std::env::var_os("NV_SCENE_TRACE").is_some(),
            prev_bits: [0; 3],
            scan: None,
            just_sent: false,
            tick: 0,
            hidden: 0,
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
        // The game's own clock on the engine's virtual one (`NV_REPEATABLE`: one fixed step a frame),
        // so clouds, grass and water move the same way in every run: two runs give identical pictures.
        // `NV_SCENE_REALTIME=1` keeps the real clock.
        if std::env::var_os("NV_REPEATABLE").is_none()
            && std::env::var("NV_SCENE_REALTIME").as_deref() != Ok("1")
        {
            std::env::set_var("NV_REPEATABLE", REPEATABLE_FRAMES);
        }
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
        // `NV_SCENE_TRACE=1`: one log line a frame with the player's position as the bits of its floats, so two
        // runs can be diffed for the first frame where their game state differs (nv-rs#479, run-to-run noise).
        if self.trace {
            let player = engine.mem.u32(world::PLAYER_POINTER);
            if player != 0 {
                let at = [0, 4, 8].map(|o| engine.mem.u32(player + world::POSITION_OFFSET + o));
                fork_log::debug!(target: "scenetrace", "t {:08x} {:08x} {:08x}", at[0], at[1], at[2]);
                if let Some((x, y, z, flag)) = world::last_good_position(engine) {
                    fork_log::debug!(target: "scenetrace", "lkg {x:.1} {y:.1} {z:.1} return {flag}");
                }
                if let Some((old, left)) = self.scan {
                    world::scan_for_position(engine, old, left);
                    self.scan = if left > 1 {
                        Some((old, left - 1))
                    } else {
                        None
                    };
                }
                self.prev_bits = at;
            }
        }
        let screen = match host::uncap::screen(engine) {
            host::uncap::Screen::Play => Screen::Play,
            host::uncap::Screen::Menu => Screen::Menu,
            host::uncap::Screen::Loading => Screen::Loading,
        };
        let step = self.step(&mut EngineWorld { engine, screen }, now);
        if self.just_sent {
            self.just_sent = false;
            if self.trace {
                self.scan = Some((self.prev_bits, 6));
            }
        }
        match step {
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
        if self.request.freeze.is_some() && !self.vanity_off {
            // The idle ("vanity") camera orbits the player after `fVanityModeAutoDelay` seconds without input: set it
            // out of reach so a slow machine or a long place file cannot bring the orbit back.
            match w.set_float_setting(VANITY_SETTING, VANITY_OFF_SECONDS) {
                Ok(true) => {
                    self.vanity_off = true;
                    fork_log::info!(target: "scene", "scene: {VANITY_SETTING} set to {VANITY_OFF_SECONDS}");
                }
                Ok(false) => {}
                Err(e) => return Step::Failed(format!("scene: {e}")),
            }
        }
        if let Some((hour, weather)) = self.request.freeze {
            w.freeze(hour, weather);
        }
        if self.request.hide_actors && (self.next > 0 || !matches!(self.phase, Phase::WaitMenu)) {
            // Actors load in with their cells: hide again every 15 frames, and right before the picture.
            self.tick = self.tick.wrapping_add(1);
            if self.tick % 15 == 0 || matches!(self.phase, Phase::Waiting { .. }) {
                let n = w.hide_actors();
                if n != self.hidden {
                    fork_log::info!(target: "scene", "scene: {n} actors hidden");
                    self.hidden = n;
                }
            }
        }
        if !matches!(self.phase, Phase::WaitMenu | Phase::Done) {
            self.place_frames += 1;
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
                let need = if self.next == 0 {
                    STEADY_FRAMES
                } else {
                    LATER_STEADY_FRAMES
                };
                if self.steady < need {
                    return Step::Continue;
                }
                self.steady = 0;
                fork_log::info!(target: "scene", "scene: sending the player to {} from the {:?} screen", place.name, w.screen());
                match w.go_to(place) {
                    Ok(t) => {
                        self.just_sent = true;
                        self.target = t.pos;
                        self.place_frames = 0;
                        fork_log::info!(target: "scene", "scene: {} resolved to {:.1} {:.1} {:.1}, heading {:.1}, pitch {:.1}", place.name, t.pos[0], t.pos[1], t.pos[2], t.heading, t.pitch);
                        self.phase = Phase::Going { since: now };
                        Step::Continue
                    }
                    Err(e) => Step::Failed(format!("scene: place {}: {e}", place.name)),
                }
            }
            Phase::Going { since } => {
                if now - since > f64::from(self.request.timeout_s) {
                    let at = w.player().map_or("no player cell".to_string(), |a| {
                        format!("player at {:.0} {:.0} {:.0}", a.pos[0], a.pos[1], a.pos[2])
                    });
                    return Step::Failed(format!(
                        "scene: place {} was not reached within {} s (screen {:?}, top menu {:#x}, {at}, wanted {:.0} {:.0} {:.0})",
                        place.name,
                        self.request.timeout_s,
                        w.screen(),
                        w.top_menu(),
                        self.target[0],
                        self.target[1],
                        self.target[2]
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
                if cell != fork_scene::cell_of(self.target) {
                    return Step::Continue;
                }
                if self.request.freeze.is_some() {
                    w.pin_sway_time();
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
                        // A message box or a menu (the new game's item messages) holds the count; only a loading screen
                        // starts it again: the player is not moving while a box is up.
                        let keep = w.screen() == Screen::Menu;
                        self.phase = Phase::Settling {
                            since,
                            last_z,
                            stable: if keep { stable } else { 0 },
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
                if place.camera.is_some() {
                    match w.camera(place, at.pos) {
                        Ok(t) => {
                            fork_log::info!(target: "scene", "scene: {} free camera at {:.1} {:.1} {:.1}, heading {:.1}, pitch {:.1}", place.name, t.pos[0], t.pos[1], t.pos[2], t.heading, t.pitch);
                        }
                        Err(e) => return Step::Failed(format!("scene: place {}: {e}", place.name)),
                    }
                    self.phase = Phase::Camera {
                        since,
                        left: CAMERA_FRAMES,
                        pos: at.pos,
                    };
                    return Step::Continue;
                }
                self.ready(w, place.name.clone(), since, at.pos, now)
            }
            Phase::Camera { since, left, pos } => {
                if now - since > f64::from(self.request.timeout_s) {
                    return Step::Failed(format!(
                        "scene: place {}: the free camera was not held within {} s",
                        place.name, self.request.timeout_s
                    ));
                }
                if let Err(e) = w.camera(place, pos) {
                    return Step::Failed(format!("scene: place {}: {e}", place.name));
                }
                if left > 1 {
                    self.phase = Phase::Camera {
                        since,
                        left: left - 1,
                        pos,
                    };
                    return Step::Continue;
                }
                self.ready(w, place.name.clone(), since, pos, now)
            }
            Phase::Soaking {
                since,
                left,
                last,
                pos,
            } => {
                // A message box or a menu may be up for a moment; a menu that stays is a failure.
                if w.screen() != Screen::Play {
                    self.away += 1;
                    if self.away >= AWAY_FRAMES {
                        return Step::Failed(format!(
                            "scene: place {} left the play screen during the soak and did not come back in {AWAY_FRAMES} frames (screen {:?}, top menu {:#x})",
                            place.name,
                            w.screen(),
                            w.top_menu()
                        ));
                    }
                    self.phase = Phase::Soaking {
                        since,
                        left,
                        last: now,
                        pos,
                    };
                    return Step::Continue;
                }
                self.away = 0;
                if !place.path.is_empty() {
                    // A walking bench: this frame's step along the path.
                    let total = self.request.soak_warmup
                        + self.request.soak_frames.unwrap_or(1) * self.request.soak_passes;
                    let done = f64::from(total - left);
                    if let Some(at) = fork_scene::path_point(&place.path, done * place.path_speed) {
                        if let Err(e) = w.move_player(at) {
                            return Step::Failed(format!("scene: place {}: {e}", place.name));
                        }
                    }
                }
                self.ms.push((now - last) * 1000.0);
                if left > 1 {
                    self.phase = Phase::Soaking {
                        since,
                        left: left - 1,
                        last: now,
                        pos,
                    };
                    return Step::Continue;
                }
                // The warm-up frames are not timed; each pass of `soak_frames` frames has its own average, and the average
                // reported is the median of them (one slow pass from outside load does not move it).
                let frames = self.request.soak_frames.unwrap_or(1) as usize;
                let warm = (self.request.soak_warmup as usize).min(self.ms.len());
                self.ms.drain(..warm);
                let mut passes: Vec<f64> = self
                    .ms
                    .chunks(frames)
                    .map(|c| c.iter().sum::<f64>() / c.len() as f64)
                    .collect();
                passes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let median = passes[passes.len() / 2];
                let spread = (passes[passes.len() - 1] - passes[0]) / median.max(1e-9) * 100.0;
                let st = frame_stats(&mut self.ms);
                let n = self.ms.len();
                // The 1% low: the frame rate of the slowest 1% of frames (their mean), as the bench reports it.
                let worst = (n / 100).max(1);
                let low1 =
                    1000.0 / (self.ms[n - worst..].iter().sum::<f64>() / worst as f64).max(1e-9);
                let worst01 = (n / 1000).max(1);
                let low01 = 1000.0
                    / (self.ms[n - worst01..].iter().sum::<f64>() / worst01 as f64).max(1e-9);
                let p50 = self.ms[n / 2];
                fork_log::info!(target: "scene", "scene: soak {} frames {} avg_ms {:.3} p99_ms {:.2} max_ms {:.2} low1_fps {:.1} passes {} spread_pct {:.2} p50_ms {:.3} low01_fps {:.1}", place.name, n, median, st.1, st.2, low1, passes.len(), spread, p50, low01);
                self.after_ready(w, place.name.clone(), since, pos)
            }
            Phase::Holding { since, until, pos } => {
                if now - since > f64::from(self.request.timeout_s) {
                    return Step::Failed(format!(
                        "scene: place {}: the fixed capture frame was not reached within {} s",
                        place.name, self.request.timeout_s
                    ));
                }
                if self.place_frames < until {
                    return Step::Continue;
                }
                self.after_hold(w, place.name.clone(), since, pos)
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

    /// The player stands (and the free camera, when there is one, is in place): a soak starts, or the picture is
    /// asked for.
    fn ready(
        &mut self,
        w: &mut dyn World,
        name: String,
        since: f64,
        pos: [f64; 3],
        now: f64,
    ) -> Step {
        if let Some(frames) = self.request.soak_frames {
            let total = self.request.soak_warmup + frames * self.request.soak_passes;
            self.ms = Vec::with_capacity(total as usize);
            self.phase = Phase::Soaking {
                since,
                left: total,
                last: now,
                pos,
            };
            return Step::Continue;
        }
        self.after_ready(w, name, since, pos)
    }

    /// The place is ready (and soaked): ask for the picture, or finish when there is none to take.
    fn after_ready(&mut self, w: &mut dyn World, name: String, since: f64, pos: [f64; 3]) -> Step {
        if let (Some(n), Some(_)) = (self.request.capture_frames, &self.request.out) {
            let until = u64::from(n);
            if self.place_frames > until {
                return Step::Failed(format!("scene: place {name} was ready after {} frames, later than the fixed capture frame {n}", self.place_frames));
            }
            self.phase = Phase::Holding { since, until, pos };
            return Step::Continue;
        }
        match &self.request.out {
            None => self.finish(name, pos, None),
            Some(dir) => {
                if let Err(e) = std::fs::create_dir_all(dir) {
                    return Step::Failed(format!("scene: {}: {e}", dir.display()));
                }
                let bmp = dir.join(format!("{name}.bmp"));
                let _ = std::fs::remove_file(&bmp);
                w.ask_picture(&bmp);
                self.phase = Phase::Waiting { since, bmp, pos };
                Step::Continue
            }
        }
    }

    /// The fixed frame has come: ask for the picture.
    fn after_hold(&mut self, w: &mut dyn World, name: String, since: f64, pos: [f64; 3]) -> Step {
        let Some(dir) = &self.request.out else {
            return self.finish(name, pos, None);
        };
        if let Err(e) = std::fs::create_dir_all(dir) {
            return Step::Failed(format!("scene: {}: {e}", dir.display()));
        }
        let bmp = dir.join(format!("{name}.bmp"));
        let _ = std::fs::remove_file(&bmp);
        w.ask_picture(&bmp);
        self.phase = Phase::Waiting { since, bmp, pos };
        Step::Continue
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
        settings: Vec<(String, f32)>,
        /// What the "drawing layer" writes when asked for a picture.
        draw: Option<Vec<u8>>,
        /// The free camera calls: where the player stood each time.
        cameras: Vec<[f64; 3]>,
        /// The positions the player was moved to along a path.
        moves: Vec<[f64; 3]>,
    }
    impl World for Fake {
        fn screen(&self) -> Screen {
            self.screen
        }
        fn go_to(&mut self, p: &Place) -> Result<Target, String> {
            if self.refuse {
                return Err("no world".into());
            }
            self.sent.push(p.name.clone());
            Ok(Target {
                pos: p.pos,
                heading: p.heading,
                pitch: p.pitch,
            })
        }
        fn player(&self) -> Option<Standing> {
            self.standing
        }
        fn camera(&mut self, p: &Place, standing: [f64; 3]) -> Result<Target, String> {
            self.cameras.push(standing);
            Ok(Target {
                pos: [standing[0], standing[1], standing[2] + 100.0],
                heading: p.heading,
                pitch: p.pitch,
            })
        }
        fn move_player(&mut self, pos: [f64; 3]) -> Result<(), String> {
            self.moves.push(pos);
            Ok(())
        }
        fn set_float_setting(&mut self, name: &str, value: f32) -> Result<bool, String> {
            self.settings.push((name.to_string(), value));
            Ok(true)
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
            capture_frames: None,
            hide_actors: false,
            soak_frames: None,
            soak_warmup: 0,
            soak_passes: 1,
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
            settings: vec![],
            draw: None,
            cameras: Vec::new(),
            moves: Vec::new(),
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
    fn a_place_with_a_camera_holds_it_for_frames_before_it_is_ready() {
        let f = PlaceFile::parse(
            "version = 1\n[[place]]\nname = \"a\"\nworld = \"W\"\nx = 100\ny = 100\nz = 5\ncamera_dz = 500\n",
        )
        .unwrap();
        let mut req = request(60);
        req.places = f.places;
        let mut r = Runner::new(req);
        let mut w = fake(Screen::Menu);
        send(&mut r, &mut w, 0.0);
        w.screen = Screen::Play;
        w.standing = Some(Standing {
            pos: [100.0, 100.0, 5.0],
        });
        let mut steps = 0;
        let reached = loop {
            steps += 1;
            assert!(steps < 200, "never ready");
            if let Step::Reached { .. } = r.step(&mut w, 1.0 + f64::from(steps) * 0.01) {
                break steps;
            }
        };
        // The camera was set when the player stood and held for CAMERA_FRAMES frames more.
        assert_eq!(w.cameras.len() as u32, CAMERA_FRAMES + 1);
        assert!(w.cameras.iter().all(|c| c == &[100.0, 100.0, 5.0]));
        assert!(reached as u32 > CAMERA_FRAMES);
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
    fn a_fixed_capture_frame_asks_for_the_picture_at_that_frame_whenever_the_place_was_ready() {
        let dir = std::env::temp_dir().join(format!("scene-run-fixed-{}", std::process::id()));
        let mut req = request(60);
        req.out = Some(dir.clone());
        req.capture_frames = Some(20);
        let mut r = Runner::new(req);
        let mut w = fake(Screen::Menu);
        send(&mut r, &mut w, 0.0);
        w.screen = Screen::Play;
        w.standing = Some(Standing {
            pos: [100.0, 100.0, 5.0],
        });
        let mut asked_at = None;
        for i in 1..=40u32 {
            r.step(&mut w, f64::from(i) * 0.01);
            if asked_at.is_none() && !w.asked.is_empty() {
                asked_at = Some(i);
            }
        }
        // Ready after 3 frames, but asked only at the fixed frame (the send itself is frame 0).
        assert_eq!(asked_at, Some(20));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_place_ready_after_the_fixed_capture_frame_fails_naming_both() {
        let dir = std::env::temp_dir().join(format!("scene-run-late-{}", std::process::id()));
        let mut req = request(600);
        req.out = Some(dir.clone());
        req.capture_frames = Some(1);
        let mut r = Runner::new(req);
        let mut w = fake(Screen::Menu);
        send(&mut r, &mut w, 0.0);
        w.screen = Screen::Play;
        w.standing = Some(Standing {
            pos: [100.0, 100.0, 5.0],
        });
        r.step(&mut w, 1.0);
        r.step(&mut w, 1.1);
        let Step::Failed(line) = r.step(&mut w, 1.2) else {
            panic!("no failure")
        };
        assert!(
            line.contains("later than the fixed capture frame 1"),
            "{line}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_idle_camera_is_set_out_of_reach_once_when_the_world_is_frozen() {
        let mut req = request(60);
        req.freeze = Some((12.0, 1));
        let mut r = Runner::new(req);
        let mut w = fake(Screen::Menu);
        for _ in 0..5 {
            r.step(&mut w, 0.0);
        }
        assert_eq!(
            w.settings,
            [(VANITY_SETTING.to_string(), VANITY_OFF_SECONDS)]
        );
    }

    #[test]
    fn frame_times_are_summarised() {
        let mut ms = [10.0, 20.0, 30.0, 40.0];
        assert_eq!(frame_stats(&mut ms), (25.0, 40.0, 40.0));
        assert_eq!(frame_stats(&mut []), (0.0, 0.0, 0.0));
    }

    #[test]
    fn a_soak_runs_its_frames_at_the_place_before_it_ends() {
        let mut req = request(60);
        req.soak_frames = Some(3);
        let mut r = Runner::new(req);
        let mut w = fake(Screen::Menu);
        send(&mut r, &mut w, 0.0);
        w.screen = Screen::Play;
        w.standing = Some(Standing {
            pos: [100.0, 100.0, 5.0],
        });
        r.step(&mut w, 1.0);
        r.step(&mut w, 1.1);
        // Three soak frames, then the place is done.
        assert_eq!(r.step(&mut w, 1.2), Step::Continue);
        assert_eq!(r.step(&mut w, 1.3), Step::Continue);
        assert!(matches!(r.step(&mut w, 1.4), Step::Reached { .. }));
    }

    #[test]
    fn a_soak_runs_its_warm_up_and_every_pass_before_it_ends() {
        let mut req = request(60);
        req.soak_frames = Some(3);
        req.soak_warmup = 2;
        req.soak_passes = 2;
        let mut r = Runner::new(req);
        let mut w = fake(Screen::Menu);
        send(&mut r, &mut w, 0.0);
        w.screen = Screen::Play;
        w.standing = Some(Standing {
            pos: [100.0, 100.0, 5.0],
        });
        r.step(&mut w, 1.0);
        r.step(&mut w, 1.1);
        // 2 + 2 * 3 = 8 frames; the eighth ends the soak.
        for i in 0..7 {
            assert_eq!(r.step(&mut w, 1.2 + f64::from(i) * 0.01), Step::Continue);
        }
        assert!(matches!(r.step(&mut w, 2.0), Step::Reached { .. }));
    }

    #[test]
    fn a_soak_of_a_place_with_a_path_walks_it_a_step_a_frame() {
        let f = PlaceFile::parse(
            "version = 1\n[[place]]\nname = \"a\"\nworld = \"W\"\nx = 100\ny = 100\nz = 5\npath = [[0, 0, 5], [100, 0, 5]]\npath_speed = 2\n",
        )
        .unwrap();
        let mut req = request(60);
        req.places = f.places;
        req.soak_frames = Some(4);
        let mut r = Runner::new(req);
        let mut w = fake(Screen::Menu);
        send(&mut r, &mut w, 0.0);
        w.screen = Screen::Play;
        w.standing = Some(Standing {
            pos: [100.0, 100.0, 5.0],
        });
        let mut t = 1.0;
        while !matches!(r.step(&mut w, t), Step::Reached { .. }) {
            t += 0.01;
            assert!(t < 5.0);
        }
        let xs: Vec<f64> = w.moves.iter().map(|p| p[0]).collect();
        assert_eq!(xs, [0.0, 2.0, 4.0, 6.0]);
    }

    #[test]
    fn leaving_the_play_screen_during_a_soak_fails() {
        let mut req = request(60);
        req.soak_frames = Some(5);
        let mut r = Runner::new(req);
        let mut w = fake(Screen::Menu);
        send(&mut r, &mut w, 0.0);
        w.screen = Screen::Play;
        w.standing = Some(Standing {
            pos: [100.0, 100.0, 5.0],
        });
        r.step(&mut w, 1.0);
        r.step(&mut w, 1.1);
        w.screen = Screen::Menu;
        for t in 0..AWAY_FRAMES - 1 {
            assert_eq!(r.step(&mut w, 1.2 + f64::from(t) * 0.001), Step::Continue);
        }
        let Step::Failed(line) = r.step(&mut w, 9.0) else {
            panic!("no failure")
        };
        assert!(line.contains("left the play screen"), "{line}");
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
