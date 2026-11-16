//! The per-run summary: one compact JSON file, `run-summary-<unix>-<pid>.json`, next to the JSON
//! log, written when the run ends (a clean exit, a stop, a panic, a SIGTERM/SIGINT/SIGHUP) and
//! every 30 seconds while it runs (so a SIGKILL or a power cut leaves the last one, marked
//! `"ended":"running"`). The pure builder is here; the writing and the signal handling are in
//! `lib.rs` and `signals`.

use crate::model::{Cause, Stats, LEVELS_MS};
use std::collections::BTreeMap;
use std::fmt::Write;

pub const SCHEMA: &str = "nv-rs-run-summary/1";

/// The levels a run is flagged at.
#[derive(Clone, Debug, PartialEq)]
pub struct Thresholds {
    pub hitches_per_min: f64,
    pub p99_ms: f64,
    pub target_fps: f64,
    pub loading_secs: f64,
    /// Seconds of measured frames a run needs before rates are judged.
    pub min_secs: f64,
}

impl Thresholds {
    pub fn parse(var: &dyn Fn(&str) -> Option<String>) -> Thresholds {
        let num = |n: &str, d: f64| {
            var(n)
                .and_then(|v| v.trim().parse::<f64>().ok())
                .filter(|v| *v >= 0.0)
                .unwrap_or(d)
        };
        Thresholds {
            hitches_per_min: num("NV_HITCH_FLAG_PER_MIN", 3.0),
            p99_ms: num("NV_HITCH_FLAG_P99_MS", 20.0),
            target_fps: num("NV_HITCH_TARGET_FPS", 60.0),
            loading_secs: num("NV_HITCH_FLAG_LOADING_S", 120.0),
            min_secs: num("NV_HITCH_FLAG_MIN_SECS", 20.0),
        }
    }
}

/// Everything the summary is made from.
pub struct Input<'a> {
    pub id: &'a str,
    pub started_unix: u64,
    pub ended_unix: u64,
    pub ended: &'a str,
    pub duration_s: f64,
    pub meta: &'a BTreeMap<String, String>,
    pub peaks: &'a BTreeMap<String, u64>,
    pub stats: &'a Stats,
    pub soft_stops: u64,
    pub errors: u64,
    pub warnings: u64,
    pub other_viewers: u64,
    pub log_name: &'a str,
    pub thresholds: &'a Thresholds,
}

/// The flags of a run, in a stable order.
pub fn flags(i: &Input<'_>) -> Vec<&'static str> {
    let t = i.thresholds;
    let s = i.stats;
    let mut f = Vec::new();
    let measured_s = s.total_wall_ns as f64 / 1e9;
    let enough = measured_s >= t.min_secs;
    if enough && s.per_minute() > t.hitches_per_min {
        f.push("hitches_per_min_high");
    }
    if enough && s.percentile_ms(0.99) > t.p99_ms {
        f.push("p99_over_20ms");
    }
    if enough && s.play_ns as f64 / 1e9 >= t.min_secs && s.fps() < t.target_fps {
        f.push("fps_below_target");
    }
    if s.loading_ns as f64 / 1e9 > t.loading_secs {
        f.push("loading_over_limit");
    }
    if s.slow_windows > 0 {
        f.push("slow_windows");
    }
    if i.soft_stops > 0 {
        f.push("soft_stops");
    }
    if i.errors > 0 {
        f.push("errors");
    }
    match i.ended {
        "exit" => {}
        "running" => f.push("unclean_exit"),
        e if e.starts_with("signal") => f.push("killed"),
        _ => f.push("stopped"),
    }
    if i.other_viewers > 0 {
        f.push("noisy_machine");
    }
    f
}

/// The one-line verdict.
pub fn verdict(i: &Input<'_>, flags: &[&str]) -> String {
    let s = i.stats;
    let base = format!(
        "{:.0} fps, p99 {:.1} ms, {} hitches ({:.1}/min)",
        s.fps(),
        s.percentile_ms(0.99),
        s.hitches,
        s.per_minute()
    );
    let hard = flags
        .iter()
        .any(|f| matches!(*f, "stopped" | "killed" | "unclean_exit"));
    if flags.is_empty() {
        return format!("OK: {base}");
    }
    let mut v = format!(
        "{}: {base}; flags {}",
        if hard { "FAILED" } else { "FLAGGED" },
        flags.join(", ")
    );
    if let Some(w) = s.worst.first() {
        let _ = write!(
            v,
            "; worst {:.0} ms {} (trigger {})",
            w.wall_ms,
            w.cause.name(),
            w.trigger
        );
    }
    v
}

/// A duration as `14m 32s` or `45s`.
fn span(secs: f64) -> String {
    let s = secs.round() as u64;
    if s >= 3600 {
        format!("{}h {:02}m", s / 3600, s % 3600 / 60)
    } else if s >= 60 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        format!("{s}s")
    }
}

/// The lines the terminal shows when the run ends (quiet mode's final summary): the verdict,
/// the frame numbers, the hitches with the worst three, the peaks and the counts with the
/// flags, and where the log and the summary file are.
pub fn final_lines(i: &Input<'_>, log: &str, summary: &str) -> Vec<String> {
    let s = i.stats;
    let fl = flags(i);
    let p99 = s.percentile_ms(0.99);
    let p999 = s.percentile_ms(0.999);
    let low = |p: f64| if p > 0.0 { 1000.0 / p } else { 0.0 };
    let mut out = vec![format!("run {}: {}", i.ended, verdict(i, &fl))];
    out.push(format!(
        "  {:.1} fps, p99 {p99:.1} ms, 1% low {:.0} fps, 0.1% low {:.0} fps; {} frames, play {}, loading {}",
        s.fps(),
        low(p99),
        low(p999),
        s.frames,
        span(s.play_ns as f64 / 1e9),
        span(s.loading_ns as f64 / 1e9)
    ));
    let mut h = format!("  hitches {} ({:.1}/min)", s.hitches, s.per_minute());
    let worst: Vec<String> = s
        .worst
        .iter()
        .take(3)
        .map(|w| {
            format!(
                "{:.0} ms f{} {} ({})",
                w.wall_ms,
                w.frame,
                w.cause.name(),
                w.trigger
            )
        })
        .collect();
    if !worst.is_empty() {
        h.push_str(&format!("; worst {}", worst.join(", ")));
    }
    out.push(h);
    let peaks: Vec<String> = i
        .peaks
        .iter()
        .map(|(k, v)| match k.strip_suffix("_mb") {
            Some(name) => format!("{name} {v} MB"),
            None => format!("{k} {v}"),
        })
        .collect();
    out.push(format!(
        "  peaks {}; soft stops {}, errors {}, warnings {}; flags {}",
        if peaks.is_empty() {
            "-".to_string()
        } else {
            peaks.join(", ")
        },
        i.soft_stops,
        i.errors,
        i.warnings,
        if fl.is_empty() {
            "none".to_string()
        } else {
            fl.join(", ")
        }
    ));
    out.push(format!("  log {log}; summary {summary} (nv-rs-dev logs)"));
    out
}

fn q(o: &mut String, s: &str) {
    fork_log::json_string(o, s);
}

/// The summary as JSON text (one object, one line).
pub fn build(i: &Input<'_>) -> String {
    let s = i.stats;
    let fl = flags(i);
    let mut o = String::with_capacity(2048);
    let _ = write!(o, "{{\"schema\":\"{SCHEMA}\",\"id\":");
    q(&mut o, i.id);
    let _ = write!(
        o,
        ",\"started_unix\":{},\"ended_unix\":{},\"ended\":",
        i.started_unix, i.ended_unix
    );
    q(&mut o, i.ended);
    o.push_str(",\"log\":");
    q(&mut o, i.log_name);
    o.push_str(",\"meta\":{");
    for (n, (k, v)) in i.meta.iter().enumerate() {
        if n > 0 {
            o.push(',');
        }
        q(&mut o, k);
        o.push(':');
        q(&mut o, v);
    }
    let _ = write!(
        o,
        "}},\"duration_s\":{:.1},\"play_s\":{:.1},\"loading_s\":{:.1},\"frames\":{},",
        i.duration_s,
        s.play_ns as f64 / 1e9,
        s.loading_ns as f64 / 1e9,
        s.frames
    );
    let avg = if s.frames == 0 {
        0.0
    } else {
        s.total_wall_ns as f64 / s.frames as f64 / 1e6
    };
    let p99 = s.percentile_ms(0.99);
    let p999 = s.percentile_ms(0.999);
    let _ = write!(
        o,
        "\"frame_ms\":{{\"avg\":{avg:.2},\"p50\":{:.1},\"p95\":{:.1},\"p99\":{p99:.1},\"max\":{:.1},\"fps\":{:.1},\"low1_fps\":{:.1},\"low01_fps\":{:.1}}},",
        s.percentile_ms(0.5),
        s.percentile_ms(0.95),
        s.max_ns as f64 / 1e6,
        s.fps(),
        if p99 > 0.0 { 1000.0 / p99 } else { 0.0 },
        if p999 > 0.0 { 1000.0 / p999 } else { 0.0 }
    );
    o.push_str("\"over_pct\":{");
    for (n, (l, c)) in LEVELS_MS.iter().zip(s.over).enumerate() {
        if n > 0 {
            o.push(',');
        }
        let pct = if s.frames == 0 {
            0.0
        } else {
            c as f64 * 100.0 / s.frames as f64
        };
        let _ = write!(o, "\"{l}\":{pct:.3}");
    }
    let _ = write!(
        o,
        "}},\"hitches\":{{\"count\":{},\"per_min\":{:.2},\"slow_windows\":{},\"by_cause\":{{",
        s.hitches,
        s.per_minute(),
        s.slow_windows
    );
    let mut first = true;
    for (c, (n, us)) in Cause::ALL.iter().zip(s.by_cause) {
        if n == 0 {
            continue;
        }
        if !first {
            o.push(',');
        }
        first = false;
        let _ = write!(o, "\"{}\":{{\"n\":{n},\"ms\":{}}}", c.name(), us / 1000);
    }
    o.push_str("}},\"worst\":[");
    for (n, w) in s.worst.iter().enumerate() {
        if n > 0 {
            o.push(',');
        }
        let _ = write!(
            o,
            "{{\"frame\":{},\"run_ms\":{},\"wall_ms\":{:.1},\"cause\":\"{}\",\"trigger\":",
            w.frame,
            w.end_ms,
            w.wall_ms,
            w.cause.name()
        );
        q(&mut o, &w.trigger);
        o.push_str(",\"top\":[");
        for (k, t) in w.top.iter().enumerate() {
            if k > 0 {
                o.push(',');
            }
            q(&mut o, t);
        }
        o.push_str("]}");
    }
    o.push_str("],\"peaks\":{");
    for (n, (k, v)) in i.peaks.iter().enumerate() {
        if n > 0 {
            o.push(',');
        }
        q(&mut o, k);
        let _ = write!(o, ":{v}");
    }
    let _ = write!(
        o,
        "}},\"soft_stops\":{},\"errors\":{},\"warnings\":{},\"other_viewers\":{},\"flags\":[",
        i.soft_stops, i.errors, i.warnings, i.other_viewers
    );
    for (n, f) in fl.iter().enumerate() {
        if n > 0 {
            o.push(',');
        }
        q(&mut o, f);
    }
    o.push_str("],\"verdict\":");
    q(&mut o, &verdict(i, &fl));
    o.push('}');
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Screen, Worst};

    fn steady(secs: u64, ms: u64) -> Stats {
        let mut s = Stats::default();
        for _ in 0..(secs * 1000 / ms) {
            s.frame(ms * 1_000_000, Screen::Play);
        }
        s
    }

    fn input<'a>(
        s: &'a Stats,
        meta: &'a BTreeMap<String, String>,
        peaks: &'a BTreeMap<String, u64>,
        t: &'a Thresholds,
        ended: &'a str,
    ) -> Input<'a> {
        Input {
            id: "1700000000-42",
            started_unix: 1_700_000_000,
            ended_unix: 1_700_000_100,
            ended,
            duration_s: 100.0,
            meta,
            peaks,
            stats: s,
            soft_stops: 0,
            errors: 0,
            warnings: 0,
            other_viewers: 0,
            log_name: "nv-rs-1700000000-42.jsonl",
            thresholds: t,
        }
    }

    fn defaults() -> Thresholds {
        Thresholds::parse(&|_| None)
    }

    #[test]
    fn a_clean_steady_run_is_ok() {
        let s = steady(60, 7);
        let (m, p, t) = (BTreeMap::new(), BTreeMap::new(), defaults());
        let i = input(&s, &m, &p, &t, "exit");
        assert!(flags(&i).is_empty());
        let v = verdict(&i, &[]);
        assert!(v.starts_with("OK: 143 fps"), "{v}");
    }

    #[test]
    fn hitches_slowness_and_stops_are_flagged() {
        let mut s = steady(60, 25);
        for k in 0..10 {
            s.hitch(Worst {
                top: vec!["ReadFile".into(), "0040abcd".into()],
                frame: k,
                end_ms: k * 1000,
                wall_ms: 80.0 + k as f64,
                cause: Cause::FileIo,
                trigger: "cell.change 20 ms before".into(),
            });
        }
        let (m, p, t) = (BTreeMap::new(), BTreeMap::new(), defaults());
        let mut i = input(&s, &m, &p, &t, "stop");
        i.soft_stops = 2;
        i.other_viewers = 1;
        let f = flags(&i);
        for want in [
            "hitches_per_min_high",
            "p99_over_20ms",
            "fps_below_target",
            "soft_stops",
            "stopped",
            "noisy_machine",
        ] {
            assert!(f.contains(&want), "{want} in {f:?}");
        }
        let v = verdict(&i, &f);
        assert!(v.starts_with("FAILED:"), "{v}");
        assert!(v.contains("worst 89 ms file_io"), "{v}");
    }

    #[test]
    fn a_short_run_is_not_judged_on_rates_and_loading_is_counted_apart() {
        let mut s = steady(5, 40);
        for _ in 0..50 {
            s.frame(300_000_000, Screen::Loading);
        }
        let (m, p) = (BTreeMap::new(), BTreeMap::new());
        let t = Thresholds {
            loading_secs: 10.0,
            ..defaults()
        };
        let i = input(&s, &m, &p, &t, "exit");
        assert_eq!(flags(&i), vec!["loading_over_limit"]);
        assert_eq!(s.frames, 125, "loading frames are not measured frames");
    }

    #[test]
    fn a_kill_and_an_unclean_end_are_told_apart() {
        let s = steady(30, 7);
        let (m, p, t) = (BTreeMap::new(), BTreeMap::new(), defaults());
        assert_eq!(flags(&input(&s, &m, &p, &t, "signal:15")), vec!["killed"]);
        assert_eq!(
            flags(&input(&s, &m, &p, &t, "running")),
            vec!["unclean_exit"]
        );
    }

    #[test]
    fn the_summary_is_one_line_of_json_with_the_documented_keys() {
        let mut s = steady(60, 7);
        s.hitch(Worst {
            top: vec!["0040abcd".into()],
            frame: 9,
            end_ms: 5000,
            wall_ms: 90.0,
            cause: Cause::Loading,
            trigger: "none".into(),
        });
        let mut m = BTreeMap::new();
        m.insert("gpu".to_string(), "test \"gpu\"".to_string());
        let mut p = BTreeMap::new();
        p.insert("rss_mb".to_string(), 4096);
        let t = defaults();
        let text = build(&input(&s, &m, &p, &t, "exit"));
        assert!(!text.contains('\n'));
        for key in [
            "\"schema\":\"nv-rs-run-summary/1\"",
            "\"id\":\"1700000000-42\"",
            "\"ended\":\"exit\"",
            "\"frame_ms\":{\"avg\"",
            "\"low1_fps\"",
            "\"over_pct\":{\"16.7\"",
            "\"by_cause\":{\"loading\":{\"n\":1",
            "\"top\":[\"0040abcd\"]",
            "\"peaks\":{\"rss_mb\":4096}",
            "\"gpu\":\"test \\\"gpu\\\"\"",
            "\"flags\":[]",
            "\"verdict\":\"OK:",
        ] {
            assert!(text.contains(key), "{key} in {text}");
        }
    }

    #[test]
    fn the_final_lines_say_the_verdict_the_numbers_and_the_paths() {
        let mut s = steady(60, 7);
        for (n, ms) in [(30, 41.0), (9, 90.0), (50, 33.0), (70, 20.0)] {
            s.hitch(Worst {
                top: vec![],
                frame: n,
                end_ms: 5000 + n,
                wall_ms: ms,
                cause: Cause::Loading,
                trigger: "cell.change 12 ms before".into(),
            });
        }
        let mut p = BTreeMap::new();
        p.insert("rss_mb".to_string(), 4096);
        let m = BTreeMap::new();
        let t = defaults();
        let lines = final_lines(
            &input(&s, &m, &p, &t, "exit"),
            "/l/a.jsonl",
            "/l/run-summary.json",
        );
        assert_eq!(lines.len(), 5, "{lines:#?}");
        assert!(lines[0].starts_with("run exit: "), "{}", lines[0]);
        assert!(lines[1].contains("1% low") && lines[1].contains("0.1% low"));
        assert!(lines[2].contains("worst 90 ms f9 loading (cell.change 12 ms before), 41 ms f30"));
        assert_eq!(lines[2].matches(" ms f").count(), 3);
        assert!(lines[3].contains("peaks rss 4096 MB") && lines[3].contains("flags"));
        assert!(lines[4].contains("/l/a.jsonl") && lines[4].contains("/l/run-summary.json"));
        assert_eq!(span(45.0), "45s");
        assert_eq!(span(872.0), "14m 32s");
        assert_eq!(span(7260.0), "2h 01m");
    }

    #[test]
    fn the_percentiles_come_from_the_buckets() {
        let mut s = Stats::default();
        for _ in 0..99 {
            s.frame(7_000_000, Screen::Play);
        }
        s.frame(120_000_000, Screen::Play);
        assert!((s.percentile_ms(0.5) - 7.0).abs() < 0.2);
        assert!((s.percentile_ms(0.99) - 7.0).abs() < 0.2);
        assert!((s.percentile_ms(1.0) - 120.0).abs() < 0.2);
    }
}
