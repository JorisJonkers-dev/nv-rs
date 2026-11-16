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
//! Text goes to stderr, filtered by `NV_LOG` (else `RUST_LOG`), default `info`. The JSON file
//! (`NV_LOG_JSON=path|auto|off`, default `auto`) is written once [`init`] ran, filtered by
//! `NV_LOG_JSON_FILTER` (default `debug`), so it holds more than the screen does. Neither holds
//! game data: callers pass names of calls and counts, never file contents.

mod filter;
pub mod game;
mod sink;
pub mod stop;

pub use filter::{Filter, Level};
pub use stop::{classify, soft_stop, soft_stops, with_policy, Policy, StopClass};

use std::collections::VecDeque;
use std::fmt::{self, Display, Write as _};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
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

struct Config {
    text: Filter,
    json: Filter,
    full: bool,
    /// The most verbose level either sink shows.
    loosest: u8,
}

fn config() -> &'static Config {
    static CONFIG: OnceLock<Config> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let var = |n: &str| std::env::var(n).ok().filter(|v| !v.trim().is_empty());
        // The `game` trace (game.rs) is for the file unless asked for: `NV_LOG=game=debug`.
        let text = Filter::parse(
            &var("NV_LOG")
                .or_else(|| var("RUST_LOG"))
                .unwrap_or_else(|| "info,game=off".to_string()),
            Level::Info,
        );
        let json = Filter::parse(&var("NV_LOG_JSON_FILTER").unwrap_or_default(), Level::Debug);
        let full = var("NV_LOG_FORMAT").is_some_and(|v| v.eq_ignore_ascii_case("full"));
        let loosest = text.loosest().max(json.loosest());
        Config {
            text,
            json,
            full,
            loosest,
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

/// Logs a message.
pub fn log(level: Level, target: &str, args: fmt::Arguments<'_>) {
    emit(level, target, false, false, args, &[]);
}

/// Logs a message whose text goes to stdout, as it is: the status heartbeat that launchers and
/// scripts read from there.
pub fn log_stdout(level: Level, target: &str, args: fmt::Arguments<'_>) {
    emit(level, target, true, true, args, &[]);
}

/// Logs a message whose text goes out as it is, without the `warning:`/`error:` prefix: the
/// lines scripts match (`Game's main stopped in frame N`).
pub fn log_raw(level: Level, target: &str, args: fmt::Arguments<'_>) {
    emit(level, target, true, false, args, &[]);
}

/// Logs a message with structured fields (they appear in the JSON record, not on the screen).
pub fn event(
    level: Level,
    target: &str,
    args: fmt::Arguments<'_>,
    fields: &[(&str, &dyn Display)],
) {
    emit(level, target, false, false, args, fields);
}

fn thread_name() -> String {
    let t = std::thread::current();
    t.name()
        .map_or_else(|| format!("{:?}", t.id()), str::to_string)
}

fn emit(
    level: Level,
    target: &str,
    raw: bool,
    stdout: bool,
    args: fmt::Arguments<'_>,
    fields: &[(&str, &dyn Display)],
) {
    let c = config();
    if (level as u8) > c.loosest {
        return;
    }
    let to_text = c.text.allows(level, target);
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
    if to_text {
        let line = render_text(level, target, raw, c.full, &ts, &message);
        use std::io::Write;
        if stdout {
            let _ = writeln!(std::io::stdout().lock(), "{line}");
        } else {
            let _ = writeln!(std::io::stderr().lock(), "{line}");
        }
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
        let record = render_json(
            level,
            target,
            &ts,
            &message,
            fields,
            frame(),
            &thread_name(),
        );
        write_json(&record, level <= Level::Warn);
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
            log(
                Level::Info,
                "host",
                format_args!(
                    "logging: JSON lines to {} (NV_LOG_JSON=off stops it)",
                    path.display()
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
    let level = if soft_stops() > 0 || ERRORS.load(Ordering::Relaxed) > 0 {
        Level::Warn
    } else {
        Level::Info
    };
    log(level, "host", format_args!("{}", summary()));
    write_final("exit", Level::Info, &|o| {
        o.push_str(",\"summary\":");
        sink::json_str(o, &summary());
    });
}

/// `error!(target: "files", "message {}", x)`.
#[macro_export]
macro_rules! error { (target: $t:expr, $($a:tt)+) => { $crate::log($crate::Level::Error, $t, format_args!($($a)+)) }; }
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
