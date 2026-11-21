//! nv-viewer: runs Fallout: New Vegas through the engine port.
//!
//! The player's own FalloutNV.exe (1.4.0.525) is mapped and the game's translated code
//! (`crates/engine`, through `crates/host`) runs its `WinMain` as the main thread; this program
//! only finds the installation, starts the host, gives the game's frames their turn and ends
//! when the game's `main` ends. The game draws in its own window through the real Direct3D 9
//! (`NV_D3D9=real`, docs/adr/0008) and plays its sound through DirectSound (`NV_SOUND=out`);
//! the host reads the keyboard and the mouse from that window.
//!
//! Until 2026-11-03 this was also a hand-written reimplementation of the game drawn with Bevy;
//! that layer was removed (docs/adr/0011-old-layer-removed.md).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const USAGE: &str = "\
nv-viewer - run Fallout: New Vegas through the engine port

USAGE:
    nv-viewer <DATA FOLDER> [OPTIONS]

    DATA FOLDER  the game's Data folder, or the installation folder containing it
                 (FalloutNV.exe 1.4.0.525 is read from the installation)

OPTIONS:
    --movement-missing FILE  write the untranslated functions the game's code reached,
                             with counts, to FILE, its stops to FILE.stops.tsv and the
                             static initializers skipped to FILE.static.tsv (every 5 s
                             and when the game's main ends)
    --movement-probe         for testing: the game's code goes on past functions not
                             translated yet (with zero results) to list more of them
    --check                  only check the installation (FalloutNV.exe is 1.4.0.525) and
                             what the run will find, print it and end: status 0 when the
                             game can start, 1 when not
    --precompile-shaders     convert and compile every shader of the load order into the shader
                             caches (docs/fork/SHADER-CACHE.md) and end: the Data folder's shader
                             packages, then each --shader-root in order (later roots replace a
                             package of the same name)
    --shader-root PATH       another Data folder, or shader package file, of the load order
                             (with --precompile-shaders; repeatable)
    -h, --help               print this

Other arguments (the removed viewer's place name, --walk, --physics, --wait, --background,
--new-game, --official, ...) are accepted and ignored, so older launchers keep working.

ENVIRONMENT:
    NV_D3D9=real    the game's own window through the real Direct3D 9 (the play build)
    NV_SOUND=out    the game's sound through DirectSound
    docs/PLAYTESTING.md lists the rest (read by crates/host and crates/engine-core).
";

/// Linux and macOS: what `play.sh` asks the program before a start.
#[cfg(not(windows))]
const FIND_USAGE: &str = "
LINUX AND MACOS:
    nv-viewer --find   print nv-rs's own folder, the game folders found (Steam, Heroic, Lutris)
                       and the game's save folders found in Proton's and Wine's prefixes, one a
                       line, fields split by tabs: own FOLDER / warning TEXT / game STORE FOLDER /
                       saves FROM FOLDER; in each field every byte but ASCII letters, digits and
                       ` /._-+,:@=~()` is written %XX (only read; nothing is written)
    nv-viewer --build-mods GAME OWN MODS
                       make OWN/modgame, the game folder to play with mods: GAME's files linked,
                       the mods in OWN/mod installs and then those in MODS copied over them
                       (play.sh --mods; the game's own folder is never written)
";

/// `--find` (Linux and macOS): [`FIND_USAGE`]'s lines (`game_folders::report`), from `HOME`.
#[cfg(not(windows))]
fn find() {
    match game_folders::Home::from_process() {
        Some(home) => print!("{}", game_folders::report(&home)),
        None => {
            eprintln!("error: HOME is not set to a full path, so the player's folders are unknown");
            host::end_process(1);
        }
    }
}

/// `--build-mods GAME OWN LOOSE` (Linux and macOS, play.sh's `--mods`): makes `OWN/modgame`, the
/// whole game folder to play with mods, as Play.exe makes `userdata\modgame` on Windows
/// (`mod_install::overlay::build`): GAME's files linked, the installed mods (`OWN/mod installs`)
/// and then the loose folder LOOSE copied over them; the plugin list goes to the mods profile's
/// `AppData/Local/FalloutNV/plugins.txt`. Says what it did in one line; on a failure, what to do,
/// and ends with status 1.
#[cfg(not(windows))]
fn build_mods(raw: &[OsString]) {
    let [game, own, loose] = [1, 2, 3].map(|i| raw.get(i).map(PathBuf::from));
    let (Some(game), Some(own), Some(loose)) = (game, own, loose) else {
        eprintln!(
            "error: --build-mods needs the game's folder, nv-rs's own folder and the mods folder"
        );
        host::end_process(2);
    };
    match build_mods_in(&game, &own, &loose) {
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
        }
        Err(text) => {
            eprintln!("{text}");
            host::end_process(1);
        }
    }
}

/// `--install-mods GAME INSTALLS MODS`: the mod installer's text front end (`mod_chooser`): the
/// mods waiting in MODS installed into INSTALLS, or removed from it; the game folder GAME only
/// read. Started by Play.exe's `--install-mods` (Install mods.cmd, in a console window of its own)
/// and play.sh's. Every way it ends, on every system, says "Press Enter to close." and waits for
/// it (`mod_chooser::run_in_window`), so that window never closes before it is read.
fn install_mods(raw: &[OsString]) {
    let [game, installs, loose] = [1, 2, 3].map(|i| raw.get(i).map(PathBuf::from));
    let places = match (game, installs, loose) {
        (Some(game), Some(installs), Some(loose)) => {
            let (major, minor, beta) = nvse::VERSION;
            Ok(mod_chooser::Places {
                game,
                installs,
                loose,
                extenders: vec![("nvse".to_owned(), format!("{major}.{minor}.{beta}"))],
                play_with_mods: if cfg!(windows) {
                    "choose \"Play with mods\"".to_owned()
                } else {
                    "start nv-rs with ./play.sh --mods".to_owned()
                },
                room: mod_chooser::room_on_drive,
            })
        }
        _ => Err("The mod installer was started without the folders it needs (the game's folder, the mod installs folder and the mods folder). Start it with \"Install mods\" (Windows) or ./play.sh --install-mods (Linux and macOS).".to_owned()),
    };
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut out = std::io::stdout();
    let code = mod_chooser::run_in_window(&mut input, &mut out, places);
    if code != 0 {
        host::end_process(code);
    }
}

/// [`build_mods`]'s work: `OWN/modgame` made from GAME, the installed mods and LOOSE, the plugin
/// list at the mods profile's place where the game reads it (`machine_table::plugins_txt`); the
/// lines to say, or what went wrong in words.
#[cfg(not(windows))]
fn build_mods_in(game: &Path, own: &Path, loose: &Path) -> Result<Vec<String>, String> {
    let layers = mod_install::installs::layers(&own.join("mod installs"), loose)
        .map_err(|e| format!("error: the installed mods could not be read: {e}"))?;
    let plugins = engine_platform::machine_table::plugins_txt(own, "mods");
    let built = mod_install::overlay::build(game, &layers, &own.join("modgame"), &plugins)
        .map_err(|e| e.player_text())?;
    let mut lines = vec![format!(
        "mods: {} mod files from {} folders; {} names made and {} removed since the last start",
        built.mod_files,
        layers.len(),
        built.made,
        built.removed
    )];
    for name in &built.added_plugins {
        lines.push(format!(
            "mods: {name} switched on after the mods folder's own plugins.txt"
        ));
    }
    if built.extender_plugins {
        lines.push(
            "mods: some mods include plugins for script extenders (.dll files): those are not used"
                .to_owned(),
        );
    }
    Ok(lines)
}

/// The command line.
#[derive(Debug, Default, PartialEq)]
struct Args {
    data: PathBuf,
    movement_missing: Option<PathBuf>,
    movement_probe: bool,
    /// `--check`: check the installation and end.
    check: bool,
    /// `--precompile-shaders`: fill the shader caches and end.
    precompile_shaders: bool,
    /// `--shader-root`: later roots of the load order.
    shader_roots: Vec<PathBuf>,
    /// The removed viewer's arguments, ignored.
    ignored: Vec<String>,
}

/// `Ok(None)`: `--help`. Each argument as the system gave it (on Linux any bytes: a game folder
/// whose name is not UTF-8 is carried as it is, never turned into text).
fn parse(raw: &[OsString]) -> Result<Option<Args>, String> {
    let mut args = Args::default();
    let mut data = None;
    let mut i = 0;
    while i < raw.len() {
        let arg = raw[i].as_os_str();
        match arg.to_str() {
            Some("-h" | "--help") => return Ok(None),
            Some("--movement-probe") => args.movement_probe = true,
            Some("--check") => args.check = true,
            Some("--precompile-shaders") => args.precompile_shaders = true,
            Some("--shader-root") => {
                i += 1;
                let dir = raw.get(i).ok_or("--shader-root needs a PATH")?;
                args.shader_roots.push(PathBuf::from(dir));
            }
            Some("--movement-missing") => {
                i += 1;
                let file = raw.get(i).ok_or("--movement-missing needs a FILE")?;
                args.movement_missing = Some(PathBuf::from(file));
            }
            _ if data.is_none() && !arg.to_string_lossy().starts_with('-') => {
                data = Some(PathBuf::from(arg));
            }
            _ => args.ignored.push(arg.to_string_lossy().into_owned()),
        }
        i += 1;
    }
    args.data = data.ok_or("the game's Data folder is missing")?;
    Ok(Some(args))
}

/// The command line after the program's name. On Windows as before (`std::env::args`); on Linux
/// and macOS as the system's own strings (`args_os`), so a folder name that is not UTF-8 reaches
/// [`parse`] as it is instead of stopping the program with a panic.
fn command_line() -> Vec<OsString> {
    #[cfg(windows)]
    {
        std::env::args().skip(1).map(OsString::from).collect()
    }
    #[cfg(not(windows))]
    {
        std::env::args_os().skip(1).collect()
    }
}

/// The game's master file, which a Data folder holds.
const MAIN_MASTER: &str = "FalloutNV.esm";

/// The Data folder: `path` itself or its `Data`, whichever holds FalloutNV.esm (any case). Off
/// Windows by the finder's own rule (`game_folders::data_folder`: a `Data` folder in any case,
/// every one tried), which `--find` and play.sh use too, so a game they report is one this takes.
fn find_data_folder(path: &Path) -> Result<PathBuf, String> {
    #[cfg(not(windows))]
    if let Some(data) = game_folders::data_folder(path) {
        return Ok(data);
    }
    #[cfg(windows)]
    for folder in [path.to_path_buf(), path.join("Data")] {
        let has_master = std::fs::read_dir(&folder).is_ok_and(|entries| {
            entries.flatten().any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(MAIN_MASTER)
            })
        });
        if has_master {
            return Ok(folder);
        }
    }
    Err(format!(
        "neither {} nor a Data folder inside it contains {MAIN_MASTER}",
        path.display()
    ))
}

/// `--movement-missing`'s lists, each rewritten whole: the untranslated addresses reached
/// (`path`), the stops (`.stops.tsv`) and the static initializers skipped (`.static.tsv`).
fn write_lists(host: &host::Host, path: &Path) {
    let _ = std::fs::write(path, host.movement.missing_tsv(&|_| None));
    let _ = std::fs::write(path.with_extension("stops.tsv"), host.movement.stops_tsv());
    let _ = std::fs::write(path.with_extension("static.tsv"), host.static_skips_tsv());
}

#[cfg(windows)]
#[link(name = "user32")]
extern "system" {
    fn SetProcessDpiAwarenessContext(context: isize) -> i32;
}

fn main() {
    // macOS: winit's loop, and with it the game's window, must own the program's main thread, so
    // the program runs on a thread of its own and the main thread is the window layer's
    // (`host::run_on_main_thread`). Everywhere else the program runs here, as before.
    #[cfg(target_vendor = "apple")]
    host::run_on_main_thread(Box::new(run));
    #[cfg(not(target_vendor = "apple"))]
    run()
}

/// Whether this run is `--check` and the player has not chosen a log target: the report goes to
/// the terminal and no log files are made.
fn check_leaves_no_logs(args: impl IntoIterator<Item = OsString>, log_chosen: bool) -> bool {
    !log_chosen && args.into_iter().any(|a| a == "--check")
}

/// The program: its arguments, the check or the start, main's frames.
pub fn run() {
    // With the game's own window (`NV_D3D9=real`, `wgpu` or `real,wgpu`, or a launcher's play
    // run, which draws by its `Fallout.ini`'s `sRenderer` without the variable) the process is
    // DPI-unaware, as FalloutNV.exe is (no `dpiAware` in its manifest): Windows scales the game's
    // window on a display above 100% from the start, as it does the original's (playtest,
    // 2026-10-30). Any backend named, as the window's test (`d3d9real::wanted`; a value that one
    // refuses draws nothing, and the awareness doesn't matter then).
    #[cfg(windows)]
    if std::env::var("NV_D3D9").is_ok_and(|v| !v.trim().is_empty())
        || host::fixes::is_play_run(|name| std::env::var(name).ok())
    {
        // DPI_AWARENESS_CONTEXT_UNAWARE.
        // SAFETY: no pointers; a failure leaves the awareness as it was.
        unsafe {
            SetProcessDpiAwarenessContext(-1);
        }
    }
    // `--check` prints its report and writes no log files (#401): without this the run left an
    // empty `.log` and `.jsonl` pair each time. A player's own `NV_LOG_JSON` still wins.
    if check_leaves_no_logs(
        std::env::args_os().skip(1),
        std::env::var_os("NV_LOG_JSON").is_some(),
    ) {
        // The process is still single-threaded here.
        std::env::set_var("NV_LOG_JSON", "off");
    }
    // Logging (docs/fork/LOGGING.md): the JSON file and the panic hook, before anything logs.
    fork_log::init();
    // The hitch flight recorder and the run summary (docs/fork/HITCHES.md).
    fork_hitch::init();
    set_run_meta();
    let raw = command_line();
    #[cfg(not(windows))]
    if raw.first().map(OsString::as_os_str) == Some(std::ffi::OsStr::new("--find")) {
        find();
        return;
    }
    if raw.first().map(OsString::as_os_str) == Some(std::ffi::OsStr::new("--install-mods")) {
        install_mods(&raw);
        return;
    }
    #[cfg(not(windows))]
    if raw.first().map(OsString::as_os_str) == Some(std::ffi::OsStr::new("--build-mods")) {
        build_mods(&raw);
        return;
    }
    let args = match parse(&raw) {
        Ok(Some(args)) => args,
        Ok(None) => {
            print!("{USAGE}");
            #[cfg(not(windows))]
            print!("{FIND_USAGE}");
            return;
        }
        Err(message) => {
            eprintln!("error: {message}\n\n{USAGE}");
            host::end_process(2);
        }
    };
    if !args.ignored.is_empty() {
        fork_log::warn!(
            target: "host",
            "Ignored (the removed viewer's arguments): {}",
            args.ignored.join(" ")
        );
    }
    let data = match find_data_folder(&args.data) {
        Ok(data) => data,
        Err(e) => {
            fork_log::fatal!(target: "host", "{e}");
            host::end_process(1);
        }
    };
    if args.check {
        match host::check_installation(&data) {
            Ok(lines) => {
                for line in lines {
                    println!("{line}");
                }
                return;
            }
            Err(e) => {
                fork_log::fatal!(target: "host", "{e}");
                host::end_process(1);
            }
        }
    }
    if args.precompile_shaders {
        let mut roots = vec![data.clone()];
        roots.extend(args.shader_roots.iter().cloned());
        let (shader_cache, pipelines) =
            d3d9_wgpu::shader_cache::plan(host::wgpu_cache_path(), true);
        let options = d3d9_wgpu::precompile::Options {
            roots,
            shader_cache,
            pipelines,
        };
        match d3d9_wgpu::precompile::run(&options) {
            Ok(r) => {
                println!(
                    "precompiled: {} packages ({} replaced by a later root), {} shaders, {} distinct, {} unreadable, {} waited for, {:.2} s",
                    r.packages, r.overridden, r.shaders, r.unique, r.unreadable, r.waited_for, r.elapsed.as_secs_f64()
                );
                return;
            }
            Err(e) => {
                eprintln!("error: {e}");
                // The expected stop off Windows: its own status (play.sh: no report needed).
                host::end_process(host::error_status(&e));
            }
        }
    }
    // The game's own code runs on the player's FalloutNV.exe, read from the same installation
    // as Data: any build but 1.4.0.525 is refused here, with the reason (2026-10-13). Starting
    // runs main's first frame. What differs from the exe: the fixes preset unless the settings or
    // the environment say otherwise (docs/FIXES.md).
    // `--movement-missing` and `--movement-probe` make the run a test: its threads run stepped
    // (the recorder and the probe are the main thread's engine's) unless `NV_THREADS` says, and
    // the player's Fallout.ini options are not read.
    let command_line_test = args.movement_missing.is_some() || args.movement_probe;
    let choice = match host::fixes::Choice::read_with(command_line_test) {
        Ok(choice) => choice,
        Err(e) => {
            fork_log::fatal!(target: "host", "{e}");
            host::end_process(1);
        }
    };
    // `NV_STOP_AFTER_FRAMES=N` (nv-rs#463): a value that is no positive number stops the start here.
    let stop_after = match fork_log::stop_after::from_env() {
        Ok(limit) => limit,
        Err(e) => {
            fork_log::fatal!(target: "host", "{e}");
            host::end_process(1);
        }
    };
    // `NV_SCENE=<place file>` (nv-rs#479, docs/fork/SCENE.md): the run goes to the named places itself.
    let mut scene = match fork_scene_run::Runner::from_env() {
        Ok(runner) => runner,
        Err(e) => {
            fork_log::fatal!(target: "host", "{e}");
            host::end_process(1);
        }
    };
    let mut game = match host::Host::start_with(&data, &choice) {
        Ok(host) => host,
        Err(e) => {
            fork_log::fatal!(target: "host", "{e}");
            host::end_process(host::error_status(&e));
        }
    };
    fork_log::info!(target: "host", "{}", game.describe());
    if let Some(runner) = scene.take() {
        fork_scene_run::install(&mut game.engine, runner);
    }
    game.movement.probe = args.movement_probe;
    // The run ends where the game's `main` ends its frames (`host::Host::at_main_end`), as the
    // original's process does: with main's answer when it returned, with the game's own code
    // when it called `ExitProcess`, and with status 3 when it stopped (a fault, a function not
    // translated, a call the platform does not provide), so the window closes and the host's line
    // naming the end is the log's last. `--movement-missing`'s lists are written first.
    let lists = args.movement_missing.clone();
    game.at_main_end = Some(Box::new(
        move |game: &host::Host, end: host::player_move::MainEnd| {
            if let Some(path) = &lists {
                write_lists(game, path);
            }
            // What the platform layer runs before it ends a run itself (the window closed): the
            // game's own Quit ends here, and its last pipelines are written too.
            host::run_exit_hooks();
            host::end_process(end.status());
        },
    ));
    // An end in the start-up.
    game.end_if_main_ended();
    // Main's frames. The keyboard and the mouse come from the game's window
    // (`Host::set_keys_down`, `Host::mouse`); scripted runs add `NV_MOVE_KEYS` and
    // `NV_MOVE_MOUSE`. Every 5 s the status line (the heartbeat the launchers watch) and the
    // lists.
    let mut last: Option<Instant> = None;
    // Discord Rich Presence (docs/fork/DISCORD.md): off without an application ID or with
    // NV_DISCORD=0; its thread never waits on the game.
    let presence = fork_presence::start(env!("CARGO_PKG_VERSION"));
    let mut announced = false;
    let mut frame_log = host::frame_log::FrameLog::from_env();
    // `NV_MEMLOG=<seconds>`: one line of memory numbers per interval; off without it.
    let mut memlog = fork_memlog::MemLog::from_env();
    let mut frames: u64 = 0;
    // `NV_STATUS_SECS`: the status line's interval, 5 s by default, 0 for none (the stop's line
    // and the exit summary stay).
    let status_every = std::env::var("NV_STATUS_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .map_or(Duration::from_secs(5), |s| {
            Duration::from_secs_f64(s.max(0.0))
        });
    // The live status line's numbers: the frames and the time of the last one.
    let mut live = (Instant::now(), 0u64);
    let mut trace = host::fork_trace::Trace::default();
    let mut heap_guard = host::fork_heap::Guard::default();
    let mut last_ctx = fork_hitch::Context::default();
    // The hitch log (on unless `NV_HITCH_LOG=0`): a line for each frame a player could feel as a jump.
    let mut hitch_log = host::hitch_log::HitchLog::from_env();
    loop {
        if fork_log::stop_requested() {
            host::run_exit_hooks();
            host::end_process(fork_log::STOP_REQUESTED_STATUS);
        }
        fork_log::set_frame(game.movement.main_frames);
        game.set_keys_down(&[]);
        game.mouse([0; 3], [false; 8]);
        if !game.main_frame() {
            break;
        }
        if !announced {
            announced = true;
            if let Some(p) = &presence {
                p.set_state(fork_presence::State::Running);
            }
        }
        if let Some(status) = fork_scene_run::exit_status() {
            game.join();
            host::run_exit_hooks();
            host::end_process(status);
        }
        // `NV_REPEATABLE=FRAMES` (a test mode): the run ends after FRAMES of main's frames.
        if game.repeatable_end() {
            host::end_process(0);
        }
        // `NV_STOP_AFTER_FRAMES=N`: the run, real threads and all, ends with status 0 after N of main's frames.
        if fork_log::stop_after::reached(stop_after, game.movement.main_frames) {
            game.join();
            let frames = game.movement.main_frames;
            fork_log::info!(target: "host", "{}", fork_log::stop_after::end_line(stop_after.unwrap_or(0), frames));
            host::run_exit_hooks();
            host::end_process(0);
        }
        trace.sample(&game.engine);
        heap_guard.sample(&game.engine);
        let ctx = hitch_context(&game.engine);
        if ctx.cell != last_ctx.cell {
            fork_hitch::event("cell.change", u64::from(ctx.cell));
        }
        if ctx.world != last_ctx.world {
            fork_hitch::event("world.change", u64::from(ctx.world));
        }
        if ctx.screen != last_ctx.screen {
            fork_hitch::event(&format!("screen.{}", ctx.screen.name()), 0);
        }
        last_ctx = ctx;
        fork_hitch::frame_end(&ctx);
        // The frame's stage marks, taken once for both logs.
        let stages = host::hitch_log::take_stages();
        if let Some(log) = &mut hitch_log {
            log.record(&mut game.engine, stages.as_ref().map(|s| &s[..]));
        }
        if let Some(log) = &mut frame_log {
            log.record(&mut game.engine, stages.as_ref().map(|s| &s[..]));
        }
        frames += 1;
        if let Some(m) = &mut memlog {
            let now = Instant::now();
            if m.due(now) {
                println!(
                    "{}",
                    memlog_line(m, now, frames, game.engine.mem.heap_stats())
                );
            }
        }
        if frames % 300 == 0 {
            hitch_peaks(game.engine.mem.heap_stats());
        }
        // The start header, once the window, the device and the sound have said what they are.
        if frames == 1 {
            fork_log::print_header();
        }
        // An F12 report waiting for where it was taken (`place.txt`).
        host::report_place::note(&mut game.engine);
        if !status_every.is_zero() && last.map_or(true, |at| at.elapsed() >= status_every) {
            last = Some(Instant::now());
            if fork_log::live_status_on() {
                let now = Instant::now();
                let secs = now.duration_since(live.0).as_secs_f64().max(0.001);
                let fps = (frames - live.1) as f64 / secs;
                live = (now, frames);
                let s = fork_hitch::stats();
                let measured = s.frames > 0;
                fork_log::live_status(&fork_log::term::status_text(
                    game.movement.main_frames,
                    fps,
                    measured.then(|| s.percentile_ms(0.99)),
                    measured.then_some(s.hitches),
                ));
            }
            fork_log::log_stdout(
                fork_log::Level::Info,
                "status",
                format_args!("{}", game.movement.summary()),
            );
            if let Some(path) = &args.movement_missing {
                write_lists(&game, path);
            }
        }
    }
    // No frame and no end: the game's main never ran as the main thread.
    if let Some(path) = &args.movement_missing {
        write_lists(&game, path);
    }
    eprintln!(
        "error: the game's main is not running ({})",
        game.movement.summary()
    );
    host::end_process(1);
}

/// What the run summary says about the build and the session (no paths, no game data).
fn set_run_meta() {
    fork_hitch::set_meta("version", env!("CARGO_PKG_VERSION"));
    fork_hitch::set_meta(
        "profile",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "optimised"
        },
    );
    let built = std::env::current_exe()
        .and_then(|p| std::fs::metadata(p)?.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    fork_hitch::set_meta("built_unix", &built.to_string());
    fork_hitch::set_meta("os", std::env::consts::OS);
    let session = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        "wayland"
    } else if std::env::var_os("DISPLAY").is_some() {
        "x11"
    } else {
        "none"
    };
    fork_hitch::set_meta("session", session);
    fork_hitch::set_meta(
        "desktop",
        &std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
    );
}

/// What the recorder records of the game's state each frame: the screen, the player's cell and
/// world space (form ids) and position, read from guest memory (a few loads).
fn hitch_context(e: &engine_core::Engine) -> fork_hitch::Context {
    const PLAYER: u32 = 0x011d_ea3c;
    let player = e.mem.u32(PLAYER);
    let (mut cell, mut world, mut pos) = (0, 0, [0.0; 3]);
    if player != 0 {
        pos = [0x30, 0x34, 0x38].map(|o| e.mem.f32(player + o));
        let c = e.mem.u32(player + 0x40);
        if c != 0 {
            cell = e.mem.u32(c + 0x0c);
            let w = e.mem.u32(c + 0xc0);
            if w != 0 {
                world = e.mem.u32(w + 0x0c);
            }
        }
    }
    let screen = match host::uncap::screen(e) {
        host::uncap::Screen::Play => fork_hitch::Screen::Play,
        host::uncap::Screen::Menu => fork_hitch::Screen::Menu,
        host::uncap::Screen::Loading => fork_hitch::Screen::Loading,
    };
    fork_hitch::Context {
        cell,
        world,
        pos,
        screen,
    }
}

/// The run summary's memory peaks (MiB): the process, the guest heap, the card (Linux).
fn hitch_peaks(h: engine_core::HeapStats) {
    let host = fork_memlog::host_now();
    fork_hitch::peak("rss_mb", host.rss_peak.max(host.rss) >> 20);
    fork_hitch::peak("heap_mb", h.used >> 20);
    if let Some(c) = fork_memlog::card_now() {
        fork_hitch::peak("vram_mb", c.vram_used >> 20);
        fork_hitch::peak("gtt_mb", c.gtt_used >> 20);
    }
}

/// One `NV_MEMLOG` line: the heap's counters, the host process and the GPU layer's, read now.
fn memlog_line(
    m: &mut fork_memlog::MemLog,
    now: Instant,
    frames: u64,
    h: engine_core::HeapStats,
) -> String {
    let g = d3d9_wgpu::memstats::snapshot();
    m.line(
        now,
        frames,
        &fork_memlog::Heap {
            capacity: h.capacity,
            used: h.used,
            free: h.free_bytes,
            largest_free: u64::from(h.largest_free),
            top: u64::from(h.top),
            peak_addr: u64::from(h.peak),
        },
        &fork_memlog::host_now(),
        &fork_memlog::Gpu {
            kept_bytes: g.kept_bytes,
            kept_levels: g.kept_levels,
            bind_groups: g.bind_groups,
            pipelines: g.pipelines,
            presents: g.presents,
        },
        fork_memlog::card_now().as_ref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(a: &[&str]) -> Vec<OsString> {
        a.iter().map(OsString::from).collect()
    }

    /// The real `--build-mods` on a tiny game tree: the game's master linked, the mod's file in
    /// place over it, and the plugin list at the mods profile's place where the game reads it.
    #[cfg(unix)]
    #[test]
    fn build_mods_makes_the_mods_game_folder_and_its_plugin_list() {
        let dir = std::env::temp_dir().join(format!("nv-viewer-build-mods-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let write = |path: &Path, bytes: &[u8]| {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        };
        let game = dir.join("game");
        write(&game.join("FalloutNV.exe"), b"exe");
        write(&game.join("Data").join("FalloutNV.esm"), b"esm");
        write(&game.join("Data").join("DeadMoney.esm"), b"dm");
        write(&game.join("Data").join("Textures").join("a.dds"), b"game's");
        let own = dir.join("own");
        let loose = own.join("my mods");
        write(&loose.join("Textures").join("a.dds"), b"mod's");
        // A plugin header with FalloutNV.esm as its only master.
        let mut esp = b"TES4".to_vec();
        let fields: Vec<u8> = [
            &b"MAST\x0e\0FalloutNV.esm\0"[..],
            &b"DATA\x08\0"[..],
            &[0; 8],
        ]
        .concat();
        esp.extend_from_slice(&(fields.len() as u32).to_le_bytes());
        esp.extend_from_slice(&[0; 16]);
        esp.extend_from_slice(&fields);
        write(&loose.join("Mine.esp"), &esp);
        let lines = build_mods_in(&game, &own, &loose).unwrap();
        assert!(lines[0].starts_with("mods: 2 mod files"), "{lines:?}");
        let data = own.join("modgame").join("Data");
        assert_eq!(std::fs::read(data.join("FalloutNV.esm")).unwrap(), b"esm");
        assert_eq!(
            std::fs::read(data.join("Textures").join("a.dds")).unwrap(),
            b"mod's"
        );
        assert_eq!(
            std::fs::read(game.join("Data").join("Textures").join("a.dds")).unwrap(),
            b"game's"
        );
        let plugins = own
            .join("mods")
            .join("AppData")
            .join("Local")
            .join("FalloutNV")
            .join("plugins.txt");
        assert_eq!(
            plugins,
            engine_platform::machine_table::plugins_txt(&own, "mods")
        );
        assert_eq!(
            std::fs::read_to_string(&plugins).unwrap(),
            "DeadMoney.esm\r\nMine.esp\r\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Review of #630 (round 2): a game folder whose name is not UTF-8 (Linux allows any bytes)
    /// is carried as it is, byte for byte, and its check ends with a plain message, not a panic.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_folder_name_that_is_not_utf8_is_carried_as_it_is() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let folder = OsString::from_vec(b"/x/Fallout \xe9".to_vec());
        let raw = vec![folder.clone(), OsString::from("--check")];
        let args = parse(&raw).unwrap().unwrap();
        assert_eq!(args.data.as_os_str().as_bytes(), b"/x/Fallout \xe9");
        assert!(args.check);
        let message = find_data_folder(&args.data).unwrap_err();
        assert!(message.contains("FalloutNV.esm"), "{message}");
        // An argument that is not UTF-8 and is not the folder is kept among the ignored ones.
        let raw = vec![folder, OsString::from_vec(b"--odd\xff".to_vec())];
        assert_eq!(parse(&raw).unwrap().unwrap().ignored.len(), 1);
    }

    /// Review of #630 (round 4): on Linux a game whose `Data` folder, master and exe were
    /// lower-cased (copied by a tool that does that) is one `--find` reports, and the program takes
    /// it too: its Data folder and its exe are found, by the same rule as the finder's.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_game_the_finder_reports_in_another_case_is_taken() {
        let game = std::env::temp_dir().join(format!("nv-viewer-case-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&game);
        std::fs::create_dir_all(game.join("data")).unwrap();
        std::fs::write(game.join("data/falloutnv.esm"), b"not the game's file").unwrap();
        std::fs::write(game.join("falloutnv.exe"), b"not the game's file").unwrap();
        let reported = game_folders::game_folder(&game);
        let data = find_data_folder(&game);
        let exe = host::game_exe(&game.join("data"));
        let _ = std::fs::remove_dir_all(&game);
        assert_eq!(reported, Some(game.clone()));
        assert_eq!(data, Ok(game.join("data")));
        assert_eq!(exe, game.join("falloutnv.exe"));
    }

    /// play.sh's command line still parses: the Data folder, the old flags ignored.
    #[test]
    fn old_launch_lines_still_parse() {
        let raw = strings(&[
            "C:\\Games\\Data",
            "GSDocMitchellHouse",
            "--background",
            "--walk",
            "--physics",
            "engine",
            "--wait",
            "100000",
            "--movement-missing",
            "m.tsv",
        ]);
        let args = parse(&raw).unwrap().unwrap();
        assert_eq!(args.data, PathBuf::from("C:\\Games\\Data"));
        assert_eq!(args.movement_missing, Some(PathBuf::from("m.tsv")));
        assert!(!args.movement_probe);
        assert_eq!(args.ignored.len(), 7);
    }

    #[test]
    fn check_runs_write_no_log_files_unless_a_log_is_chosen() {
        let check = strings(&["C:\\Games\\Data", "--check"]);
        assert!(check_leaves_no_logs(check.clone(), false));
        assert!(!check_leaves_no_logs(check, true));
        assert!(!check_leaves_no_logs(strings(&["C:\\Games\\Data"]), false));
    }

    #[test]
    fn help_and_missing_data() {
        assert_eq!(parse(&strings(&["--help"])), Ok(None));
        assert!(parse(&strings(&["--movement-probe"])).is_err());
        assert!(parse(&strings(&["--movement-missing"])).is_err());
        assert!(
            parse(&strings(&["--check", r"C:\Games\Data"]))
                .unwrap()
                .unwrap()
                .check
        );
    }
}
