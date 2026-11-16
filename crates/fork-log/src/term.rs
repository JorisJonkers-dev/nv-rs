//! What the terminal shows (docs/fork/LOGGING.md): the policy `NV_LOG_TERM`, the repeat limit of
//! warnings and errors, the one-line live status and the start header. Everything here is pure
//! or holds its own small state, so it is tested without a terminal.

use crate::filter::{Filter, Level};
use std::collections::HashMap;

/// How much the terminal shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The start header, warnings and errors (repeats limited), stops, the final summary.
    Quiet,
    /// Quiet plus the one-time facts (`info`), without the periodic lines.
    Normal,
    /// Everything at `debug`: the file's own detail on the screen.
    Verbose,
    /// Nothing but the fatal failures.
    Off,
    /// A filter of the user's (`NV_LOG_TERM=d3d9=debug`, or `NV_LOG`/`RUST_LOG`).
    Custom,
}

/// The terminal's filter and whether repeats are limited.
#[derive(Clone, Debug)]
pub struct TermPolicy {
    pub mode: Mode,
    pub filter: Filter,
}

impl TermPolicy {
    /// `term` is `NV_LOG_TERM`, `legacy` is `NV_LOG` (else `RUST_LOG`), which was the terminal's
    /// filter before `NV_LOG_TERM` existed and still is when `NV_LOG_TERM` is not set.
    pub fn parse(term: Option<&str>, legacy: Option<&str>) -> TermPolicy {
        let named = |mode: Mode, spec: &str| TermPolicy {
            mode,
            filter: Filter::parse(spec, Level::Info),
        };
        match term.map(str::trim).filter(|v| !v.is_empty()) {
            Some(v) if v.eq_ignore_ascii_case("quiet") => {
                named(Mode::Quiet, "warn,headline=info,stop=error")
            }
            Some(v) if v.eq_ignore_ascii_case("normal") => {
                named(Mode::Normal, "info,game=off,status=off,hitch=warn")
            }
            Some(v) if v.eq_ignore_ascii_case("verbose") => named(Mode::Verbose, "debug,game=off"),
            Some(v) if v.eq_ignore_ascii_case("off") || v.eq_ignore_ascii_case("none") => {
                named(Mode::Off, "off")
            }
            Some(spec) => named(Mode::Custom, spec),
            None => match legacy.map(str::trim).filter(|v| !v.is_empty()) {
                Some(spec) => named(Mode::Custom, spec),
                None => named(Mode::Quiet, "warn,headline=info,stop=error"),
            },
        }
    }

    /// Whether warnings and errors on the terminal are limited when they repeat.
    pub fn limits_repeats(&self) -> bool {
        matches!(self.mode, Mode::Quiet | Mode::Normal)
    }

    /// Whether a line is printed: a fatal one always, the others as the filter says.
    pub fn prints(&self, level: Level, target: &str, fatal: bool) -> bool {
        fatal || self.shows(level, target)
    }

    /// Whether a line of `level` and `target` is shown.
    pub fn shows(&self, level: Level, target: &str) -> bool {
        self.filter.allows(level, target)
    }
}

/// How many times one warning or error is printed before the rest are counted.
pub const REPEAT_LIMIT: u64 = 3;

struct Seen {
    line: String,
    count: u64,
}

/// Counts the warnings and errors by their shape (digits and hex numbers do not tell two
/// apart) and says which to print: the first [`REPEAT_LIMIT`] of each.
#[derive(Default)]
pub struct Dedup {
    seen: HashMap<String, Seen>,
}

/// A message with its numbers blanked, the key two repeats share.
pub fn shape(target: &str, message: &str) -> String {
    let mut k = String::with_capacity(message.len().min(160) + target.len() + 1);
    k.push_str(target);
    k.push(':');
    let mut in_num = false;
    for c in message.chars().take(160) {
        if c.is_ascii_digit() {
            if !in_num {
                k.push('#');
            }
            in_num = true;
        } else {
            in_num = false;
            k.push(c);
        }
    }
    k
}

impl Dedup {
    /// Whether to print this one. `line` is the text as it would print; it is kept for the
    /// first of a shape, to say how often it came back.
    pub fn admit(&mut self, target: &str, message: &str, line: &dyn Fn() -> String) -> bool {
        if self.seen.len() > 4000 {
            self.seen.clear();
        }
        let e = self
            .seen
            .entry(shape(target, message))
            .or_insert_with(|| Seen {
                line: line(),
                count: 0,
            });
        e.count += 1;
        e.count <= REPEAT_LIMIT
    }

    /// `warning: ... (x repeated 40 times)` for each shape that was cut, most repeated first;
    /// forgets them.
    pub fn take_repeats(&mut self) -> Vec<String> {
        let mut cut: Vec<(u64, String)> = self
            .seen
            .drain()
            .filter(|(_, s)| s.count > REPEAT_LIMIT)
            .map(|(_, s)| (s.count - REPEAT_LIMIT, s.line))
            .collect();
        cut.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        cut.into_iter()
            .map(|(n, line)| format!("{line} (x repeated {n} times)"))
            .collect()
    }
}

/// The live status's text, cut to `width` columns (one terminal line, never wrapped).
pub fn fit_status(text: &str, width: usize) -> String {
    let width = width.max(20) - 1;
    text.chars().take(width).collect()
}

/// `f 123456  143 fps  p99 7 ms  hitches 0`.
pub fn status_text(frame: u64, fps: f64, p99_ms: Option<f64>, hitches: Option<u64>) -> String {
    let mut s = format!("f {frame}  {fps:.0} fps");
    if let Some(p) = p99_ms {
        s.push_str(&format!("  p99 {p:.0} ms"));
    }
    if let Some(h) = hitches {
        s.push_str(&format!("  hitches {h}"));
    }
    s
}

/// The bytes that put `text` on a single self-overwriting line: back to the line's start, the
/// text, spaces over what the last status was longer by, back to the start (so a line printed
/// next begins there). No escape codes: a Windows console without them behaves the same.
pub fn draw(text: &str, previous_len: usize) -> String {
    let pad = previous_len.saturating_sub(text.chars().count());
    format!("\r{text}{}\r", " ".repeat(pad))
}

/// The bytes that erase a status of `len` columns.
pub fn erase(len: usize) -> String {
    format!("\r{}\r", " ".repeat(len))
}

/// The start header: 3 to 6 lines from the run's facts (`version`, `profile`, `built_unix`,
/// `os`, `session`, `gpu`, `driver`, `display`, `resolution`, `audio`, `threads`), then the
/// log paths.
pub fn header_lines(
    fact: &dyn Fn(&str) -> Option<String>,
    json: Option<&str>,
    text: Option<&str>,
) -> Vec<String> {
    let mut out = Vec::new();
    let mut head = format!("nv-rs {}", fact("version").unwrap_or_else(|| "?".into()));
    let mut build = Vec::new();
    if let Some(p) = fact("profile") {
        build.push(p);
    }
    if let Some(secs) = fact("built_unix").and_then(|s| s.parse::<u64>().ok()) {
        if secs > 0 {
            let t = crate::sink::timestamp(
                std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs),
            );
            build.push(format!("built {} UTC", t[..16].replace('T', " ")));
        }
    }
    if !build.is_empty() {
        head.push_str(&format!(" ({})", build.join(", ")));
    }
    let mut on = vec![fact("os").unwrap_or_default()];
    on.extend(fact("session").filter(|s| s != "none"));
    on.extend(fact("desktop").filter(|s| !s.is_empty()));
    let on: Vec<String> = on.into_iter().filter(|s| !s.is_empty()).collect();
    if !on.is_empty() {
        head.push_str(&format!(" on {}", on.join(", ")));
    }
    out.push(head);
    if let Some(gpu) = fact("gpu") {
        out.push(match fact("driver").filter(|d| !d.trim().is_empty()) {
            Some(d) => format!("gpu: {gpu} ({})", d.trim()),
            None => format!("gpu: {gpu}"),
        });
    }
    let screen: Vec<String> = [
        fact("resolution"),
        fact("display").map(|d| format!("display {d}")),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !screen.is_empty() {
        out.push(format!("screen: {}", screen.join(", ")));
    }
    let mut audio: Vec<String> = fact("audio").into_iter().collect();
    audio.extend(fact("threads").map(|t| format!("threads {t}")));
    if !audio.is_empty() {
        out.push(format!("audio: {}", audio.join("; ")));
    }
    match (json, text) {
        (Some(j), Some(t)) => out.push(format!(
            "logs: {t} and {j} (read them with: nv-rs-dev logs)"
        )),
        (Some(j), None) => out.push(format!("log: {j} (read it with: nv-rs-dev logs)")),
        _ => out.push("no log file (NV_LOG_JSON=off)".to_string()),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(term: Option<&str>, legacy: Option<&str>) -> TermPolicy {
        TermPolicy::parse(term, legacy)
    }

    #[test]
    fn quiet_is_the_default_and_shows_warnings_and_headlines_only() {
        let p = policy(None, None);
        assert_eq!(p.mode, Mode::Quiet);
        assert!(p.shows(Level::Error, "d3d9"));
        assert!(p.shows(Level::Warn, "window"));
        assert!(!p.shows(Level::Info, "window"));
        assert!(!p.shows(Level::Debug, "d3d9"));
        assert!(p.shows(Level::Info, "headline"));
        assert!(p.shows(Level::Error, "stop"));
        assert!(p.limits_repeats());
    }

    #[test]
    fn normal_adds_facts_but_not_the_periodic_lines() {
        let p = policy(Some("Normal"), None);
        assert_eq!(p.mode, Mode::Normal);
        assert!(p.shows(Level::Info, "d3d9"));
        assert!(!p.shows(Level::Info, "status"));
        assert!(!p.shows(Level::Info, "game"));
        assert!(!p.shows(Level::Debug, "d3d9"));
    }

    #[test]
    fn a_fatal_failure_prints_at_every_level() {
        for spec in [
            "quiet",
            "normal",
            "verbose",
            "off",
            "error,stop=off",
            "off,headline=off",
        ] {
            let p = policy(Some(spec), None);
            assert!(p.prints(Level::Error, "stop", true), "{spec}");
        }
        assert!(!policy(Some("off"), None).prints(Level::Error, "stop", false));
    }

    #[test]
    fn verbose_shows_debug_and_does_not_limit_repeats() {
        let p = policy(Some("verbose"), Some("warn"));
        assert_eq!(p.mode, Mode::Verbose);
        assert!(p.shows(Level::Debug, "d3d9"));
        assert!(p.shows(Level::Info, "status"));
        assert!(!p.limits_repeats());
    }

    #[test]
    fn a_filter_in_nv_log_term_or_the_legacy_variables_is_used_as_given() {
        let p = policy(Some("warn,d3d9=debug"), None);
        assert_eq!(p.mode, Mode::Custom);
        assert!(p.shows(Level::Debug, "d3d9"));
        assert!(!p.shows(Level::Info, "files"));
        let legacy = policy(None, Some("info"));
        assert_eq!(legacy.mode, Mode::Custom);
        assert!(legacy.shows(Level::Info, "files"));
        // NV_LOG_TERM wins over NV_LOG.
        assert_eq!(policy(Some("quiet"), Some("debug")).mode, Mode::Quiet);
        assert!(!policy(Some("off"), None).shows(Level::Error, "stop"));
        assert_eq!(policy(Some("  "), None).mode, Mode::Quiet);
    }

    #[test]
    fn repeats_are_printed_three_times_then_counted() {
        let mut d = Dedup::default();
        let mut printed = 0;
        for i in 0..10 {
            let msg = format!("surface lost at frame {i}");
            if d.admit("window", &msg, &|| format!("warning: {msg}")) {
                printed += 1;
            }
        }
        assert_eq!(printed, 3);
        assert!(d.admit("window", "another one", &|| "warning: another one".into()));
        assert_eq!(
            d.take_repeats(),
            vec!["warning: surface lost at frame 0 (x repeated 7 times)".to_string()]
        );
        assert!(d.take_repeats().is_empty());
    }

    #[test]
    fn different_targets_and_texts_are_counted_apart() {
        let mut d = Dedup::default();
        for _ in 0..3 {
            assert!(d.admit("a", "same", &|| "x".into()));
            assert!(d.admit("b", "same", &|| "x".into()));
        }
        assert!(!d.admit("a", "same", &|| "x".into()));
        assert!(d.admit("a", "other", &|| "x".into()));
        assert_eq!(shape("t", "id 0x1f3 at 12"), "t:id #x#f# at #");
    }

    #[test]
    fn status_text_fits_one_line_and_overwrites_itself() {
        assert_eq!(
            status_text(123_456, 143.2, Some(7.4), Some(0)),
            "f 123456  143 fps  p99 7 ms  hitches 0"
        );
        assert_eq!(status_text(5, 60.0, None, None), "f 5  60 fps");
        assert_eq!(
            fit_status("abcdefghijklmnopqrstuvwxyz", 20),
            "abcdefghijklmnopqrs"
        );
        assert_eq!(draw("ab", 5), "\rab   \r");
        assert_eq!(draw("abcd", 2), "\rabcd\r");
        assert_eq!(erase(3), "\r   \r");
        assert!(!draw("x", 0).contains('\n'));
    }

    #[test]
    fn the_header_is_three_to_six_lines() {
        let facts: HashMap<&str, &str> = [
            ("version", "0.4.0"),
            ("profile", "optimised"),
            ("built_unix", "1794818000"),
            ("os", "linux"),
            ("session", "wayland"),
            ("gpu", "AMD Radeon RX 7900 XTX (RADV NAVI31)"),
            ("driver", "radv Mesa 26.0"),
            ("resolution", "2560x1440"),
            ("display", "1920x1080 144 Hz, Mailbox"),
            ("audio", "pulse 48000 Hz, 2 channels"),
            ("threads", "stepped"),
        ]
        .into_iter()
        .collect();
        let lines = header_lines(
            &|k| facts.get(k).map(|v| (*v).to_string()),
            Some("/l/nv-rs-1-2.jsonl"),
            Some("/l/nv-rs-1-2.log"),
        );
        assert_eq!(lines.len(), 5);
        assert_eq!(
            lines[0],
            "nv-rs 0.4.0 (optimised, built 2026-11-16 08:33 UTC) on linux, wayland"
        );
        assert_eq!(
            lines[1],
            "gpu: AMD Radeon RX 7900 XTX (RADV NAVI31) (radv Mesa 26.0)"
        );
        assert_eq!(
            lines[2],
            "screen: 2560x1440, display 1920x1080 144 Hz, Mailbox"
        );
        assert_eq!(
            lines[3],
            "audio: pulse 48000 Hz, 2 channels; threads stepped"
        );
        assert!(lines[4].starts_with("logs: /l/nv-rs-1-2.log and /l/nv-rs-1-2.jsonl"));
        let bare = header_lines(&|_| None, None, None);
        assert_eq!(
            bare,
            vec![
                "nv-rs ?".to_string(),
                "no log file (NV_LOG_JSON=off)".to_string()
            ]
        );
    }
}
