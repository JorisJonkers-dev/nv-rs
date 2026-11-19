//! The play-mode entry point of the engine, for a program that links it (the Toolbox's raul).
//!
//! The program starts itself again as `<program> --play-mode <DATA> [viewer arguments]` and that
//! child calls [`play_main`]; the game is a process of its own, so a crash of the engine stays
//! out of the program that started it. The loop is the standalone viewer's (`viewer/src/main.rs`,
//! included here, so there is one loop): `--check`, `--precompile-shaders`, and
//! `--movement-missing FILE` work as they do there. The reserved word `--play-mode` is a dashed
//! argument the viewer skips with one warning in the log; the data folder is its first word
//! without a dash.
//!
//! Two things differ from the standalone program: `viewer/build.rs` (the Windows icon) is not run, so
//! the program that links this crate carries its own icon, and `env!("CARGO_PKG_VERSION")` in the
//! viewer's code is this crate's version.
//!
//! # Stop
//!
//! A signal or a kill skips the exit hooks and loses the pipeline cache and the save flush. The
//! stop is a file instead: when the environment variable [`STOP_FILE_ENV`] names a path, a small
//! thread (started by [`play_main`]) checks once a second whether the file exists and then calls
//! [`fork_log::request_stop`]. The game's own thread sees the request between two frames, runs
//! the exit hooks and ends the process with [`EXIT_STOP_REQUESTED`]. The program that asks for
//! the stop creates the file after the start (a file left from an earlier run stops the game at
//! once), waits for the exit and kills by process id only after a grace period longer than the
//! flush. A frame in which the game's code does not return (a loading screen) delays the stop, and the
//! `--movement-missing` lists are not written by a stop.
//!
//! # Exit codes
//!
//! | Code | Meaning |
//! |---|---|
//! | `0..` | the game's own code (its return or its `ExitProcess`); 0 also for `--help` and a successful `--check` |
//! | [`EXIT_START`] (1) | a start-up error: no data, the executable not accepted, a failed `--check`; also the window layer's close path that skips the exit hooks |
//! | [`EXIT_ARGUMENTS`] (2) | an argument error |
//! | [`EXIT_GAME_STOPPED`] (3) | the game's code stopped (a fault, a function not translated, a call the platform does not provide) |
//! | [`EXIT_SYSTEM_REFUSED`] (69) | the system refused the start (host start, shader precompile) |
//! | [`EXIT_STOP_REQUESTED`] (143) | ended by the stop file |
//! | a signal, or 101 or 134 | the process crashed (the system's) |
//!
//! The game's own code is any value, so it can equal one of the others: a caller trusts a code
//! only together with the log (`Game's main stopped in frame N` for 3) and with whether it asked
//! for the stop.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

#[cfg(test)]
mod drift;

/// The viewer's code, run as the play mode. The standalone `nv-viewer` calls the same `run`
/// from its own `main`, which is dead code here.
#[allow(dead_code)]
#[path = "../../../viewer/src/main.rs"]
mod viewer_main;

/// The first argument that selects the play mode in the program that links this crate.
pub const PLAY_MODE_ARG: &str = "--play-mode";

/// The environment variable naming the stop file.
pub const STOP_FILE_ENV: &str = "NV_STOP_FILE";

/// How often the stop file is looked for.
pub const STOP_POLL: Duration = Duration::from_secs(1);

/// A start-up error (no data, the executable not accepted, a failed `--check`).
pub const EXIT_START: i32 = 1;
/// An argument error.
pub const EXIT_ARGUMENTS: i32 = 2;
/// The game's code stopped (a fault, a function not translated, a call not provided).
pub const EXIT_GAME_STOPPED: i32 = 3;
/// The system refused the start (host start, shader precompile).
pub const EXIT_SYSTEM_REFUSED: i32 = 69;
/// Ended by the stop file.
pub const EXIT_STOP_REQUESTED: i32 = fork_log::STOP_REQUESTED_STATUS;

/// What an exit code of the child most likely means, for a caller that has not read the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExitClass {
    /// 0: the game ended, or the command was a query that succeeded.
    Finished,
    /// 1: a start-up error.
    StartFailed,
    /// 2: an argument error.
    BadArguments,
    /// 3: the game's code stopped.
    GameStopped,
    /// 69: the system refused the start.
    SystemRefused,
    /// 143: the stop file ended it.
    StopRequested,
    /// Any other code: the game's own, or a crash the system reports as a code.
    Other(i32),
}

impl ExitClass {
    /// The class of `code`. A signal has no code: the caller classifies that as a crash first.
    #[must_use]
    pub fn of(code: i32) -> Self {
        match code {
            0 => Self::Finished,
            EXIT_START => Self::StartFailed,
            EXIT_ARGUMENTS => Self::BadArguments,
            EXIT_GAME_STOPPED => Self::GameStopped,
            EXIT_SYSTEM_REFUSED => Self::SystemRefused,
            EXIT_STOP_REQUESTED => Self::StopRequested,
            other => Self::Other(other),
        }
    }
}

/// The path in the environment's stop-file variable, if set and not empty.
#[must_use]
pub fn stop_file(value: Option<OsString>) -> Option<PathBuf> {
    value.filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// Starts the thread that asks for the stop when `path` exists (checked every `every`).
///
/// # Errors
/// The thread could not be started.
pub fn watch_stop_file(
    path: PathBuf,
    every: Duration,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("stop-file".into())
        .spawn(move || loop {
            if path.exists() {
                fork_log::info!(
                    target: "host",
                    "Stop file {} seen: the game stops between two frames (status {EXIT_STOP_REQUESTED})",
                    path.display()
                );
                fork_log::request_stop();
                return;
            }
            std::thread::sleep(every);
        })
}

/// Runs the game as the play mode of this process and never returns.
///
/// Reads the process's real arguments. Does what the viewer's `main` does (macOS: winit's loop
/// owns the program's main thread, so the viewer's code runs on another); the log is started
/// once, inside the viewer's code, because `fork_log::init` is not guarded against a second
/// call, so a program must not call it before this. Most ends go through `host::end_process`;
/// when the viewer's code returns (`--help`, a successful `--check`)
/// the process ends here with status 0.
pub fn play_main() -> ! {
    if let Some(path) = stop_file(std::env::var_os(STOP_FILE_ENV)) {
        // A thread that cannot start leaves the stop to the window and to the kill.
        if let Err(e) = watch_stop_file(path, STOP_POLL) {
            fork_log::warn!(target: "host", "The stop file is not watched: {e}");
        }
    }
    #[cfg(target_vendor = "apple")]
    {
        host::run_on_main_thread(Box::new(viewer_main::run))
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        viewer_main::run();
        host::end_process(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_are_the_documented_table() {
        assert_eq!(
            [
                EXIT_START,
                EXIT_ARGUMENTS,
                EXIT_GAME_STOPPED,
                EXIT_SYSTEM_REFUSED,
                EXIT_STOP_REQUESTED
            ],
            [1, 2, 3, 69, 143]
        );
    }

    #[test]
    fn a_code_is_classed() {
        assert_eq!(ExitClass::of(0), ExitClass::Finished);
        assert_eq!(ExitClass::of(1), ExitClass::StartFailed);
        assert_eq!(ExitClass::of(2), ExitClass::BadArguments);
        assert_eq!(ExitClass::of(3), ExitClass::GameStopped);
        assert_eq!(ExitClass::of(69), ExitClass::SystemRefused);
        assert_eq!(ExitClass::of(143), ExitClass::StopRequested);
        assert_eq!(ExitClass::of(7), ExitClass::Other(7));
        assert_eq!(ExitClass::of(-1), ExitClass::Other(-1));
    }

    #[test]
    fn an_empty_stop_file_variable_is_none() {
        assert_eq!(stop_file(None), None);
        assert_eq!(stop_file(Some(OsString::new())), None);
        assert_eq!(
            stop_file(Some(OsString::from("/tmp/stop"))),
            Some(PathBuf::from("/tmp/stop"))
        );
    }

    #[test]
    fn the_stop_file_is_seen_and_asks_for_the_stop() {
        let dir = std::env::temp_dir().join(format!("fork-play-stop-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("stop");
        let _ = std::fs::remove_file(&file);
        assert!(!fork_log::stop_requested());
        let thread = watch_stop_file(file.clone(), Duration::from_millis(10)).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        assert!(!fork_log::stop_requested());
        std::fs::write(&file, b"").unwrap();
        thread.join().unwrap();
        assert!(fork_log::stop_requested());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
