//! The `game` trace's sampler of the game's own state (docs/fork/LOGGING.md): read-only, once a
//! frame, logging only changes. The menus are the game's translated code with no seam of the
//! port's, so what can be seen is the state words the translations name (`loadingmenu.rs`):
//! the loading menu and its thread and music, the first-load flag and the loading movie player.

use engine::Engine;

/// `LoadingMenu::pMe`, `pLoadingThread`, the music-started byte, `bInitialLoad` and the movie
/// player word (engine-fallout-interface, `loadingmenu.rs`).
const LOADING_MENU: u32 = 0x011d_a0c0;
const LOADING_THREAD: u32 = 0x011d_a0c4;
const MUSIC_STARTED: u32 = 0x011d_a0c9;
const INITIAL_LOAD: u32 = 0x011a_0294;
const MOVIE_PLAYER: u32 = 0x0126_fac4;

#[derive(Default)]
pub struct Trace {
    seen: Option<[u32; 5]>,
}

impl Trace {
    /// Reads the words and logs what changed since the last call.
    pub fn sample(&mut self, e: &Engine) {
        if !fork_log::enabled(fork_log::Level::Info, "game") {
            return;
        }
        let now = [
            e.mem.u32(LOADING_MENU),
            e.mem.u32(LOADING_THREAD),
            u32::from(e.mem.u8(MUSIC_STARTED)),
            u32::from(e.mem.u8(INITIAL_LOAD)),
            e.mem.u32(MOVIE_PLAYER),
        ];
        let before = self.seen.replace(now).unwrap_or([0; 5]);
        if now == before {
            return;
        }
        let first = now[3] != 0;
        if (now[0] != 0) != (before[0] != 0) {
            fork_log::game_info!(
                "menu.loading",
                "the loading menu {} ({} load)",
                if now[0] != 0 { "is up" } else { "is gone" },
                if first { "the game's first" } else { "a later" }
            );
        }
        if (now[1] != 0) != (before[1] != 0) {
            fork_log::game_info!(
                "load.thread",
                "the loading thread object {}",
                if now[1] != 0 {
                    "exists: a load runs"
                } else {
                    "is gone: the load ended"
                }
            );
        }
        if now[2] != before[2] {
            fork_log::game_info!("load.music", "the loading music started: {}", now[2] != 0);
        }
        if (now[4] != 0) != (before[4] != 0) {
            fork_log::game_info!(
                "load.movie",
                "the movie player {}",
                if now[4] != 0 { "is set" } else { "is cleared" }
            );
        }
    }
}
