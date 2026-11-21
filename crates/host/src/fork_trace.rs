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

/// The interface manager's pointer (`011d8a80`, what `004b7210` reads), its menu mode (`+0x0c`,
/// `cMenuMode`) and its menu stack (ten words from `+0x114`, the menu class ids in the order they
/// were put on it, 0 after the last; `interfacemanager.rs`, `AddToEnterStack`).
const INTERFACE_MANAGER: u32 = 0x011d_8a80;
const MENU_MODE: u32 = 0x0c;
const MENU_STACK: u32 = 0x114;
const MENU_STACK_WORDS: u32 = 10;

#[derive(Default)]
pub struct Trace {
    seen: Option<[u32; 5]>,
    stack: Option<(u32, [u32; MENU_STACK_WORDS as usize])>,
}

/// The menu stack as a line: its depth, the class ids from the bottom up in hex, and the mode.
pub fn describe_stack(mode: u32, words: &[u32]) -> String {
    let used: Vec<String> = words
        .iter()
        .take_while(|w| **w != 0)
        .map(|w| format!("{w:#x}"))
        .collect();
    if used.is_empty() {
        format!("no menu is open (menu mode {mode})")
    } else {
        format!(
            "{} open, bottom to top {} (menu mode {mode})",
            used.len(),
            used.join(" ")
        )
    }
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
        self.sample_menu_stack(e);
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

    /// Logs the menu stack when it or the menu mode changes (`menu.stack`).
    fn sample_menu_stack(&mut self, e: &Engine) {
        let manager = e.mem.u32(INTERFACE_MANAGER);
        if manager == 0 {
            return;
        }
        let mode = e.mem.u32(manager + MENU_MODE);
        let mut words = [0u32; MENU_STACK_WORDS as usize];
        for (i, w) in words.iter_mut().enumerate() {
            *w = e.mem.u32(manager + MENU_STACK + 4 * i as u32);
        }
        let now = (mode, words);
        if self.stack == Some(now) {
            return;
        }
        let first = self.stack.is_none();
        self.stack = Some(now);
        if first && words[0] == 0 {
            return;
        }
        fork_log::game_info!("menu.stack", "{}", describe_stack(mode, &words));
    }
}

#[cfg(test)]
mod tests {
    use super::describe_stack;

    #[test]
    fn the_stack_line_names_depth_order_and_mode() {
        assert_eq!(describe_stack(4, &[0; 10]), "no menu is open (menu mode 4)");
        assert_eq!(
            describe_stack(3, &[0x40c, 0x3e9, 0, 0x99, 0, 0, 0, 0, 0, 0]),
            "2 open, bottom to top 0x40c 0x3e9 (menu mode 3)"
        );
    }
}
