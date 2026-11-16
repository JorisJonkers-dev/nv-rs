//! The fork's logging (docs/fork/LOGGING.md): levels and targets, text on stderr, JSON lines in
//! a file, a panic hook, and the soft-stop policy.
//!
//! No dependency and no thread. Use the macros with a target naming the area (`window`, `d3d9`,
//! `sound`, `files`, `script`, `loader`, `stop`, `status`, `host`):
//!
//! ```
//! fork_log::warn!(target: "files", "{} could not be listed", "a folder");
//! ```
//!
//! The terminal is quiet by default (`NV_LOG_TERM`, [`term`]): the start header, warnings and
//! errors (repeats limited), stops, the final summary and, on a terminal, one live status
//! line. Everything else goes to two files beside each other, filtered by `NV_LOG_JSON_FILTER`
//! (default `debug`): the JSON lines (`NV_LOG_JSON=path|auto|off`, default `auto`) and the same
//! lines as plain text (`.log`). Neither holds game data: callers pass names of calls and
//! counts, never file contents.

mod filter;
pub mod game;
mod sink;
pub mod stop;
pub mod term;

pub use filter::{Filter, Level};
pub use stop::{classify, soft_stop, soft_stops, with_policy, Policy, StopClass};
pub use term::{Mode, TermPolicy};

use std::collections::VecDeque;
use std::fmt::{self, Display, Write as _};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

/// How many recent lines a stop or a panic record carries.
const RING_LINES: usize = 200;
const NO_FRAME: u64 = u64::MAX;

static FRAME: AtomicU64 = AtomicU64::new(NO_FRAME);
static ERRORS: AtomicU64 = AtomicU64::new(0);
static WARNINGS: AtomicU64 = AtomicU64::new(0);
static RING: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static FILE: Mutex<Option<sink::JsonFile>> = Mutex::new(None);
static TEXT_FILE: Mutex<Option<sink::JsonFile>> = Mutex::new(None);
static DEDUP: Mutex<Option<term::Dedup>> = Mutex::new(None);
static FACTS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
static HEADER_DONE: AtomicBool = AtomicBool::new(false);
static SUMMARY_DONE: AtomicBool = AtomicBool::new(false);

/// The live status line: what is drawn now and its width.
struct Status {
    text: String,
    shown: usize,
}
static STATUS: Mutex<Status> = Mutex::new(Status {
    text: String::new(),
    shown: 0,
});

struct Config {
    /// The terminal's policy and its filter.
    term: TermPolicy,
    text: Filter,
    /// The files' filter (the JSON lines and the plain text).
    json: Filter,
    full: bool,
    /// The most verbose level any sink shows.
    loosest: u8,
    /// The live status line: stderr is a terminal, `NV_STATUS_SECS` is not 0 and the terminal
    /// does not already show the status lines.
    live_status: bool,
    status_secs: f64,
}

fn config() -> &'static Config {
    static CONFIG: OnceLock<Config> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let var = |n: &str| std::env::var(n).ok().filter(|v| !v.trim().is_empty());
        // The terminal: `NV_LOG_TERM`, else `NV_LOG`/`RUST_LOG` as the filter it always was, else
        // quiet. `NV_LOG`/`RUST_LOG` also widen the files' filter (`game=trace` reaches them).
        let legacy = var("NV_LOG").or_else(|| var("RUST_LOG"));
        let term = TermPolicy::parse(var("NV_LOG_TERM").as_deref(), legacy.as_deref());
        let text = term.filter.clone();
        let json = match var("NV_LOG_JSON_FILTER") {
            Some(own) => Filter::parse(&own, Level::Debug),
            None => {
                let base = Filter::parse("", Level::Debug);
                match &legacy {
                    Some(l) => base.union(&Filter::parse(l, Level::Info)),
                    None => base,
                }
            }
        };
        let full = var("NV_LOG_FORMAT").is_some_and(|v| v.eq_ignore_ascii_case("full"));
        let status_secs = var("NV_STATUS_SECS")
            .and_then(|v| v.trim().parse::<f64>().ok())
            .map_or(5.0, |s| s.max(0.0));
        let live_status = status_secs > 0.0
            && term.mode != Mode::Off
            && !text.allows(Level::Info, "status")
            && std::io::stderr().is_terminal();
        let loosest = text.loosest().max(json.loosest());
        Config {
            term,
            text,
            json,
            full,
            loosest,
            live_status,
            status_secs,
        }
    })
}

/// The game's main frame the log lines are attributed to (the viewer sets it each frame).
pub fn set_frame(frame: u64) {
    FRAME.store(frame, Ordering::Relaxed);
}

pub(crate) fn frame() -> Option<u64> {
    Some(FRAME.load(Ordering::Relaxed)).filter(|f| *f != NO_FRAME)
}

/// Whether a message of `level` from `target` goes anywhere (a cheap test before building one).
pub fn enabled(level: Level, target: &str) -> bool {
    let c = config();
    (level as u8) <= c.loosest && (c.text.allows(level, target) || c.json.allows(level, target))
}

/// How a message is written.
#[derive(Clone, Copy, Default)]
struct How {
    /// Without the `warning:`/`error:` prefix.
    raw: bool,
    /// To stdout instead of stderr.
    stdout: bool,
    /// On the terminal whatever the policy says (a fatal failure).
    force: bool,
}

/// Logs a message.
pub fn log(level: Level, target: &str, args: fmt::Arguments<'_>) {
    emit(level, target, How::default(), args, &[]);
}

/// Logs a message whose text goes to stdout, as it is: the status heartbeat that launchers and
/// scripts read from there.
pub fn log_stdout(level: Level, target: &str, args: fmt::Arguments<'_>) {
    emit(
        level,
        target,
        How {
            raw: true,
            stdout: true,
            force: false,
        },
        args,
        &[],
    );
}

/// Logs a message whose text goes out as it is, without the `warning:`/`error:` prefix: the
/// lines scripts match (`Game's main stopped in frame N`).
pub fn log_raw(level: Level, target: &str, args: fmt::Arguments<'_>) {
    emit(
        level,
        target,
        How {
            raw: true,
            ..How::default()
        },
        args,
        &[],
    );
}

/// A fatal failure (a stop, device lost, out of memory, no window, no game data, a missing
/// library): an error that reaches the terminal whatever `NV_LOG_TERM` says, never limited as a
/// repeat.
pub fn fatal(target: &str, args: fmt::Arguments<'_>) {
    emit(
        Level::Error,
        target,
        How {
            force: true,
            ..How::default()
        },
        args,
        &[],
    );
}

/// [`fatal`] with the text as it is (the stop line scripts match).
pub fn fatal_raw(target: &str, args: fmt::Arguments<'_>) {
    emit(
        Level::Error,
        target,
        How {
            raw: true,
            force: true,
            ..How::default()
        },
        args,
        &[],
    );
}

/// A line the quiet terminal shows too (the start header, the final summary): `info` on the
/// target `headline`.
pub fn headline(args: fmt::Arguments<'_>) {
    emit(Level::Info, "headline", How::default(), args, &[]);
}

/// Logs a message with structured fields (they appear in the JSON record, not on the screen).
pub fn event(
    level: Level,
    target: &str,
    args: fmt::Arguments<'_>,
    fields: &[(&str, &dyn Display)],
) {
    emit(level, target, How::default(), args, fields);
}

fn thread_name() -> String {
    let t = std::thread::current();
    t.name()
        .map_or_else(|| format!("{:?}", t.id()), str::to_string)
}

fn emit(
    level: Level,
    target: &str,
    how: How,
    args: fmt::Arguments<'_>,
    fields: &[(&str, &dyn Display)],
) {
    let c = config();
    if !how.force && (level as u8) > c.loosest {
        return;
    }
    let mut to_text = c.term.prints(level, target, how.force);
    let to_json = c.json.allows(level, target);
    if !to_text && !to_json {
        return;
    }
    match level {
        Level::Error => ERRORS.fetch_add(1, Ordering::Relaxed),
        Level::Warn => WARNINGS.fetch_add(1, Ordering::Relaxed),
        _ => 0,
    };
    let message = args.to_string();
    let now = SystemTime::now();
    let ts = sink::timestamp(now);
    if to_text && !how.force && level <= Level::Warn && c.term.limits_repeats() {
        let mut d = DEDUP.lock().unwrap_or_else(|p| p.into_inner());
        let d = d.get_or_insert_with(term::Dedup::default);
        to_text = d.admit(target, &message, &|| {
            render_text(level, target, how.raw, c.full, &ts, &message)
        });
    }
    if to_text {
        let line = render_text(level, target, how.raw, c.full, &ts, &message);
        terminal_write(how.stdout, &line);
    }
    // The ring keeps what either sink shows, as one short line.
    {
        let mut line = format!("{ts} {} {target}: {message}", level.name());
        if line.len() > 400 {
            let mut cut = 400;
            while !line.is_char_boundary(cut) {
                cut -= 1;
            }
            line.truncate(cut);
        }
        let mut ring = RING.lock().unwrap_or_else(|p| p.into_inner());
        if ring.len() >= RING_LINES {
            ring.pop_front();
        }
        ring.push_back(line);
    }
    if to_json {
        let thread = thread_name();
        let record = render_json(level, target, &ts, &message, fields, frame(), &thread);
        write_json(&record, level <= Level::Warn);
        let plain = render_file_text(level, target, &ts, &message, frame());
        if let Some(f) = TEXT_FILE.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
            f.line(&plain, level <= Level::Warn);
        }
    }
}

/// One line of the plain-text log: `2026-11-16T09:40:19.586Z INFO  d3d9 f123: message`.
fn render_file_text(
    level: Level,
    target: &str,
    ts: &str,
    message: &str,
    frame: Option<u64>,
) -> String {
    let f = frame.map_or_else(|| "-".to_string(), |f| f.to_string());
    // One record is one line: a message with line breaks keeps them as `\n`.
    let message = message.replace('\r', "").replace('\n', "\\n");
    format!(
        "{ts} {:<5} {target} f{f}: {message}",
        level.name().to_ascii_uppercase()
    )
}

/// Writes a line to the terminal without breaking the live status line: the status is erased,
/// the line written, the status drawn again.
fn terminal_write(stdout: bool, line: &str) {
    use std::io::Write;
    let mut st = STATUS.lock().unwrap_or_else(|p| p.into_inner());
    let mut err = std::io::stderr().lock();
    if st.shown > 0 {
        let _ = err.write_all(term::erase(st.shown).as_bytes());
        st.shown = 0;
    }
    if stdout {
        let _ = writeln!(std::io::stdout().lock(), "{line}");
    } else {
        let _ = writeln!(err, "{line}");
    }
    if !st.text.is_empty() {
        let text = st.text.clone();
        let _ = err.write_all(term::draw(&text, 0).as_bytes());
        st.shown = text.chars().count();
    }
}

/// Draws the live status line (`f 123456  143 fps  p99 7 ms  hitches 0`) when stderr is a
/// terminal, `NV_STATUS_SECS` is not 0 and the terminal does not show the status lines anyway;
/// otherwise nothing. One line, overwriting itself.
pub fn live_status(text: &str) {
    use std::io::Write;
    let c = config();
    if !c.live_status {
        return;
    }
    let width = std::env::var("COLUMNS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(80);
    let text = term::fit_status(text, width);
    let mut st = STATUS.lock().unwrap_or_else(|p| p.into_inner());
    let _ = std::io::stderr()
        .lock()
        .write_all(term::draw(&text, st.shown).as_bytes());
    st.shown = text.chars().count();
    st.text = text;
}

/// Erases the live status line and stops it (before the final summary).
pub fn clear_status() {
    use std::io::Write;
    let mut st = STATUS.lock().unwrap_or_else(|p| p.into_inner());
    if st.shown > 0 {
        let _ = std::io::stderr()
            .lock()
            .write_all(term::erase(st.shown).as_bytes());
    }
    st.shown = 0;
    st.text.clear();
}

/// The interval of the status lines, `NV_STATUS_SECS` (5 s by default, 0 for none).
pub fn status_secs() -> f64 {
    config().status_secs
}

/// Whether the live status line is on (so a caller computes its numbers only then).
pub fn live_status_on() -> bool {
    config().live_status
}

/// The terminal's mode.
pub fn term_mode() -> Mode {
    config().term.mode
}

/// Records a fact about the run for the start header (`gpu`, `driver`, `display`, `resolution`,
/// `audio`, `threads`, `version`, ...). Short text, no paths of the player's, no game data.
pub fn fact(key: &str, value: &str) {
    let mut f = FACTS.lock().unwrap_or_else(|p| p.into_inner());
    match f.iter_mut().find(|(k, _)| k == key) {
        Some((_, v)) => *v = value.to_string(),
        None => f.push((key.to_string(), value.to_string())),
    }
}

/// A fact recorded with [`fact`].
pub fn fact_value(key: &str) -> Option<String> {
    FACTS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
}

/// Prints the start header once (3 to 6 lines; in the files always). The viewer calls it when the
/// first frame has gone, by when the window, the device and the sound have said what they are.
pub fn print_header() {
    if HEADER_DONE.swap(true, Ordering::Relaxed) {
        return;
    }
    let json = json_path().map(|p| p.display().to_string());
    let text = text_path().map(|p| p.display().to_string());
    for l in term::header_lines(&fact_value, json.as_deref(), text.as_deref()) {
        headline(format_args!("{l}"));
    }
}

/// The path of the plain-text log, when one is open.
pub fn text_path() -> Option<PathBuf> {
    TEXT_FILE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .map(|f| f.path().to_path_buf())
}

/// Prints the final summary lines of the run once (the verdict first), after erasing the live
/// status. The recorder that knows the numbers calls it; without one, [`finish`] says less.
pub fn final_summary(lines: &[String]) {
    if SUMMARY_DONE.swap(true, Ordering::Relaxed) {
        return;
    }
    clear_status();
    print_header();
    for l in lines {
        headline(format_args!("{l}"));
    }
}

/// Prints the warnings and errors that were cut as repeats, with their counts.
fn flush_repeats() {
    let lines = DEDUP
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_mut()
        .map(term::Dedup::take_repeats)
        .unwrap_or_default();
    for l in lines {
        terminal_write(false, &l);
    }
}

/// The text line: bare for `info` and for raw lines, prefixed for the rest; `full` adds the time,
/// the level and the target to every line.
fn render_text(
    level: Level,
    target: &str,
    raw: bool,
    full: bool,
    ts: &str,
    message: &str,
) -> String {
    if full {
        return format!(
            "{ts} {:<5} {target}: {message}",
            level.name().to_ascii_uppercase()
        );
    }
    match level {
        _ if raw => message.to_string(),
        Level::Error => format!("error: {message}"),
        Level::Warn => format!("warning: {message}"),
        Level::Info => message.to_string(),
        Level::Debug | Level::Trace => format!("[{} {target}] {message}", level.name()),
    }
}

fn render_json(
    level: Level,
    target: &str,
    ts: &str,
    message: &str,
    fields: &[(&str, &dyn Display)],
    frame: Option<u64>,
    thread: &str,
) -> String {
    let mut o = String::with_capacity(160 + message.len());
    o.push_str("{\"ts\":");
    sink::json_str(&mut o, ts);
    let _ = write!(o, ",\"level\":\"{}\",\"target\":", level.name());
    sink::json_str(&mut o, target);
    o.push_str(",\"message\":");
    sink::json_str(&mut o, message);
    o.push_str(",\"fields\":{");
    for (i, (k, v)) in fields.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        sink::json_str(&mut o, k);
        o.push(':');
        sink::json_str(&mut o, &v.to_string());
    }
    o.push_str("},\"frame\":");
    match frame {
        Some(f) => {
            let _ = write!(o, "{f}");
        }
        None => o.push_str("null"),
    }
    o.push_str(",\"thread\":");
    sink::json_str(&mut o, thread);
    o.push('}');
    o
}

fn write_json(record: &str, urgent: bool) {
    let mut file = FILE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(f) = file.as_mut() {
        f.line(record, urgent);
    }
}

/// Flushes the JSON file (before the process ends).
pub fn flush() {
    if let Some(f) = FILE.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
        f.flush();
    }
    if let Some(f) = TEXT_FILE.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
        f.flush();
    }
}

/// Writes one structured record of `kind` to the JSON file only: `body` is already-rendered JSON
/// members (`"hitch":{...}`) put after the `kind`, before the frame and thread. The recorders
/// that keep their own state (the hitch flight recorder, docs/fork/HITCHES.md) use it.
pub fn json_record(level: Level, target: &str, kind: &str, body: &str) {
    let mut o = String::with_capacity(96 + body.len());
    o.push_str("{\"ts\":");
    sink::json_str(&mut o, &sink::timestamp(SystemTime::now()));
    let _ = write!(o, ",\"level\":\"{}\",\"target\":", level.name());
    sink::json_str(&mut o, target);
    o.push_str(",\"kind\":");
    sink::json_str(&mut o, kind);
    if !body.is_empty() {
        o.push(',');
        o.push_str(body);
    }
    o.push_str(",\"frame\":");
    match frame() {
        Some(f) => {
            let _ = write!(o, "{f}");
        }
        None => o.push_str("null"),
    }
    o.push_str(",\"thread\":");
    sink::json_str(&mut o, &thread_name());
    o.push('}');
    write_json(&o, level <= Level::Warn);
}

/// Appends `s` to `out` as a JSON string (quoted, escaped).
pub fn json_string(out: &mut String, s: &str) {
    sink::json_str(out, s);
}

/// Registers a function run when the run ends, with how it ends (`exit`, `stop`, `panic`): a
/// recorder's last summary. It may run more than once (a stop, then the exit), so it must
/// tolerate that. At most eight.
pub fn on_finish(f: fn(&str)) {
    let mut hooks = FINISH_HOOKS.lock().unwrap_or_else(|p| p.into_inner());
    if hooks.len() < 8 {
        hooks.push(f);
    }
}

static FINISH_HOOKS: Mutex<Vec<fn(&str)>> = Mutex::new(Vec::new());

/// Runs the registered end-of-run functions with `how`.
pub fn run_finish_hooks(how: &str) {
    let hooks: Vec<fn(&str)> = FINISH_HOOKS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    for hook in hooks {
        hook(how);
    }
}

/// The folder the logs of this run go in: the JSON file's, else the automatic folder.
pub fn log_dir() -> Option<PathBuf> {
    if let Some(p) = json_path() {
        return p.parent().map(PathBuf::from);
    }
    sink::state_dir(&|n| std::env::var(n).ok())
}

/// How many errors and warnings were logged so far.
pub fn counts() -> (u64, u64) {
    (
        ERRORS.load(Ordering::Relaxed),
        WARNINGS.load(Ordering::Relaxed),
    )
}

/// The path of the JSON file, when one is open.
pub fn json_path() -> Option<PathBuf> {
    FILE.lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .map(|f| f.path().to_path_buf())
}

/// Opens the JSON file as `NV_LOG_JSON` says, installs the panic hook and says where the file
/// is. Call once, at the start of the process. Never fails: a file that cannot be opened is a
/// warning and the text log goes on.
pub fn init() -> Option<PathBuf> {
    let _ = config();
    let var = |n: &str| std::env::var(n).ok();
    let max_mb = var("NV_LOG_JSON_MAX_MB")
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|m| *m > 0)
        .unwrap_or(64);
    let (path, auto_dir) = match sink::Target::parse(var("NV_LOG_JSON").as_deref()) {
        sink::Target::Off => {
            install_panic_hook();
            return None;
        }
        sink::Target::Path(p) => (p, None),
        sink::Target::Auto => match sink::state_dir(&var) {
            Some(dir) => {
                let secs = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                (
                    dir.join(format!("nv-rs-{secs}-{}.jsonl", std::process::id())),
                    Some(dir),
                )
            }
            None => {
                install_panic_hook();
                return None;
            }
        },
    };
    if let Some(dir) = &auto_dir {
        // The newest few runs stay; the one about to start is the next.
        sink::prune(dir, 9);
    }
    install_panic_hook();
    match sink::JsonFile::open(&path, max_mb * 1024 * 1024) {
        Ok(f) => {
            *FILE.lock().unwrap_or_else(|p| p.into_inner()) = Some(f);
            // The same lines as plain text beside it, with the same bound (`NV_LOG_TEXT=off`
            // switches it off).
            let text_off = var("NV_LOG_TEXT").is_some_and(|v| {
                matches!(
                    v.trim().to_ascii_lowercase().as_str(),
                    "off" | "0" | "none" | "false"
                )
            });
            if !text_off {
                let text = path.with_extension("log");
                match sink::JsonFile::open(&text, max_mb * 1024 * 1024) {
                    Ok(f) => *TEXT_FILE.lock().unwrap_or_else(|p| p.into_inner()) = Some(f),
                    Err(why) => log(
                        Level::Warn,
                        "host",
                        format_args!(
                            "logging: the text log {} cannot be opened: {why}",
                            text.display()
                        ),
                    ),
                }
            }
            log(
                Level::Info,
                "host",
                format_args!(
                    "logging: JSON lines to {}, text to {} (NV_LOG_JSON=off stops them)",
                    path.display(),
                    text_path()
                        .unwrap_or_else(|| path.with_extension("log"))
                        .display()
                ),
            );
            Some(path)
        }
        Err(why) => {
            log(
                Level::Warn,
                "host",
                format_args!(
                    "logging: the JSON log {} cannot be opened: {why}",
                    path.display()
                ),
            );
            None
        }
    }
}

fn ring_lines() -> Vec<String> {
    RING.lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .cloned()
        .collect()
}

fn write_final(event: &str, level: Level, extra: &dyn Fn(&mut String)) {
    let ts = sink::timestamp(SystemTime::now());
    let mut o = String::new();
    let _ = write!(
        o,
        "{{\"ts\":\"{ts}\",\"level\":\"{}\",\"target\":\"stop\",\"event\":\"{event}\"",
        level.name()
    );
    extra(&mut o);
    o.push_str(",\"frame\":");
    match frame() {
        Some(f) => {
            let _ = write!(o, "{f}");
        }
        None => o.push_str("null"),
    }
    o.push_str(",\"thread\":");
    sink::json_str(&mut o, &thread_name());
    let _ = write!(
        o,
        ",\"soft_stops\":{},\"errors\":{},\"warnings\":{},\"last_lines\":[",
        soft_stops(),
        ERRORS.load(Ordering::Relaxed),
        WARNINGS.load(Ordering::Relaxed)
    );
    for (i, l) in ring_lines().iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        sink::json_str(&mut o, l);
    }
    o.push_str("]}");
    write_json(&o, true);
    flush();
}

/// The game's main stopped: one final structured record with the reason, its class, the exe
/// address chain, the frame and the last lines, flushed. The caller logs the text line.
pub fn report_stop(why: &str) {
    write_final("stop", Level::Error, &|o| {
        o.push_str(",\"reason\":");
        sink::json_str(o, why);
        let _ = write!(o, ",\"class\":\"{}\",\"addresses\":[", classify(why).name());
        for (i, a) in stop::addresses(why).iter().enumerate() {
            if i > 0 {
                o.push(',');
            }
            sink::json_str(o, a);
        }
        o.push(']');
    });
    run_finish_hooks("stop");
}

/// A panic hook that writes the same final record for a panic outside the game's own stops (the
/// host's own bugs), then runs the hook that was there.
pub fn install_panic_hook() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let before = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let message = info
                .payload()
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_default();
            let place = info.location().map(ToString::to_string).unwrap_or_default();
            ERRORS.fetch_add(1, Ordering::Relaxed);
            write_final("panic", Level::Error, &|o| {
                o.push_str(",\"reason\":");
                sink::json_str(o, &message);
                o.push_str(",\"location\":");
                sink::json_str(o, &place);
            });
            run_finish_hooks("panic");
            before(info);
        }));
    });
}

/// The run's summary line: counts of errors, warnings and soft stops, and where the JSON is.
pub fn summary() -> String {
    let mut s = format!(
        "Log summary: {} errors, {} warnings, {} soft stops",
        ERRORS.load(Ordering::Relaxed),
        WARNINGS.load(Ordering::Relaxed),
        soft_stops()
    );
    let seen = game::counts();
    if !seen.is_empty() {
        let list: Vec<String> = seen
            .iter()
            .take(8)
            .map(|(c, n)| format!("{c} x{n}"))
            .collect();
        let _ = write!(s, "; game events: {}", list.join(", "));
    }
    let calls = stop::soft_stop_calls();
    if !calls.is_empty() {
        let list: Vec<String> = calls
            .iter()
            .take(5)
            .map(|(c, n)| format!("{c} x{n}"))
            .collect();
        let _ = write!(s, " ({})", list.join(", "));
    }
    if let Some(p) = json_path() {
        let _ = write!(s, "; JSON log {}", p.display());
    }
    s
}

/// What to do as the process ends: the summary line (a warning when anything failed), a final
/// record in the JSON file, and the flush.
pub fn finish() {
    flush_repeats();
    run_finish_hooks("exit");
    if !SUMMARY_DONE.load(Ordering::Relaxed) {
        // No recorder said how the run went (NV_HITCH=off): the counts are the summary.
        let (errors, warnings) = counts();
        final_summary(&[format!(
            "run ended: {errors} errors, {warnings} warnings, {} soft stops{}",
            soft_stops(),
            json_path().map_or(String::new(), |p| format!("; log {}", p.display()))
        )]);
    }
    let level = if soft_stops() > 0 || ERRORS.load(Ordering::Relaxed) > 0 {
        Level::Warn
    } else {
        Level::Info
    };
    log(level, "host", format_args!("{}", summary()));
    flush();
    write_final("exit", Level::Info, &|o| {
        o.push_str(",\"summary\":");
        sink::json_str(o, &summary());
    });
}

/// `error!(target: "files", "message {}", x)`.
#[macro_export]
macro_rules! error { (target: $t:expr, $($a:tt)+) => { $crate::log($crate::Level::Error, $t, format_args!($($a)+)) }; }
/// A fatal failure: on the terminal whatever `NV_LOG_TERM` says.
#[macro_export]
macro_rules! fatal { (target: $t:expr, $($a:tt)+) => { $crate::fatal($t, format_args!($($a)+)) }; }
/// A line the quiet terminal shows too: `headline!("...")`.
#[macro_export]
macro_rules! headline { ($($a:tt)+) => { $crate::headline(format_args!($($a)+)) }; }
#[macro_export]
macro_rules! warn { (target: $t:expr, $($a:tt)+) => { $crate::log($crate::Level::Warn, $t, format_args!($($a)+)) }; }
#[macro_export]
macro_rules! info { (target: $t:expr, $($a:tt)+) => { $crate::log($crate::Level::Info, $t, format_args!($($a)+)) }; }
#[macro_export]
macro_rules! debug { (target: $t:expr, $($a:tt)+) => { $crate::log($crate::Level::Debug, $t, format_args!($($a)+)) }; }
#[macro_export]
macro_rules! trace { (target: $t:expr, $($a:tt)+) => { $crate::log($crate::Level::Trace, $t, format_args!($($a)+)) }; }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_record_shape() {
        let r = render_json(
            Level::Warn,
            "files",
            "T",
            "a \"b\"",
            &[("call", &"X"), ("n", &3)],
            Some(7),
            "main",
        );
        assert_eq!(
            r,
            r#"{"ts":"T","level":"warn","target":"files","message":"a \"b\"","fields":{"call":"X","n":"3"},"frame":7,"thread":"main"}"#
        );
        assert!(render_json(Level::Info, "x", "T", "m", &[], None, "t").contains("\"frame\":null"));
    }

    #[test]
    fn the_text_log_line_has_time_level_target_and_frame() {
        assert_eq!(
            render_file_text(Level::Warn, "window", "T", "a\nb", Some(12)),
            "T WARN  window f12: a\\nb"
        );
        assert_eq!(
            render_file_text(Level::Info, "d3d9", "T", "m", None),
            "T INFO  d3d9 f-: m"
        );
    }

    #[test]
    fn text_prefixes() {
        assert_eq!(
            render_text(Level::Warn, "a", false, false, "T", "m"),
            "warning: m"
        );
        assert_eq!(render_text(Level::Error, "a", true, false, "T", "m"), "m");
        assert_eq!(render_text(Level::Info, "a", false, false, "T", "m"), "m");
        assert_eq!(
            render_text(Level::Debug, "a", false, false, "T", "m"),
            "[debug a] m"
        );
        assert_eq!(
            render_text(Level::Warn, "a", false, true, "T", "m"),
            "T WARN  a: m"
        );
    }

    #[test]
    fn ring_and_final_record_reach_the_file() {
        let dir = std::env::temp_dir().join(format!("fork-log-lib-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("t.jsonl");
        *FILE.lock().unwrap() = Some(sink::JsonFile::open(&path, 1 << 20).unwrap());
        error!(target: "stop", "a line for the ring");
        report_stop("FindFirstFileA(x): the 8.3 short name is not provided (in 0044fb20 00401000)");
        let text = std::fs::read_to_string(&path).unwrap();
        *FILE.lock().unwrap() = None;
        let _ = std::fs::remove_dir_all(&dir);
        let last = text
            .lines()
            .find(|l| l.contains("\"event\":\"stop\""))
            .unwrap();
        assert!(last.contains("\"event\":\"stop\""), "{last}");
        assert!(last.contains("\"class\":\"platform\""));
        assert!(last.contains("\"addresses\":[\"0044fb20\",\"00401000\"]"));
        assert!(last.contains("a line for the ring"));
        assert!(text
            .lines()
            .any(|l| l.contains("\"message\":\"a line for the ring\"")));
    }
}
