// SPDX-License-Identifier: GPL-3.0-only
//! The bench's numbers and files, in the shape of the Python harness's (`scripts/fork/bench/suite.py`, `nvbench.py`,
//! docs/fork/BENCH.md): a per-run `summary.json` (with `schema_version` 1), the headline row of each scenario (here a
//! place), the suite table with the change against the previous suite and a stored baseline, and baselines. Pure
//! functions: the tool runs the engine and hands the rows in.

/// A place's frame times as the runner logs them (`scene: soak NAME frames N avg_ms X ...`).
#[derive(Clone, Debug, PartialEq)]
pub struct PlaceRow {
    pub place: String,
    pub frames: u64,
    pub avg_ms: f64,
    pub p50_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
    pub low1_fps: f64,
    pub low01_fps: f64,
    pub passes: u64,
    pub spread_pct: f64,
}

impl PlaceRow {
    /// From the text after `scene: soak `: the place's name, then `key value` pairs.
    pub fn parse(text: &str) -> Option<PlaceRow> {
        let mut w = text.split_whitespace();
        let place = w.next()?.to_string();
        let mut r = PlaceRow {
            place,
            frames: 0,
            avg_ms: 0.0,
            p50_ms: 0.0,
            p99_ms: 0.0,
            max_ms: 0.0,
            low1_fps: 0.0,
            low01_fps: 0.0,
            passes: 1,
            spread_pct: 0.0,
        };
        let mut seen = 0;
        while let (Some(k), Some(v)) = (w.next(), w.next()) {
            let n: f64 = v.parse().ok()?;
            match k {
                "frames" => r.frames = n as u64,
                "avg_ms" => {
                    r.avg_ms = n;
                    seen += 1;
                }
                "p50_ms" => r.p50_ms = n,
                "p99_ms" => r.p99_ms = n,
                "max_ms" => r.max_ms = n,
                "low1_fps" => r.low1_fps = n,
                "low01_fps" => r.low01_fps = n,
                "passes" => r.passes = n as u64,
                "spread_pct" => r.spread_pct = n,
                _ => {}
            }
        }
        (seen == 1).then_some(r)
    }

    pub fn fps(&self) -> f64 {
        1000.0 / self.avg_ms.max(1e-9)
    }
}

/// Frame rates the displays and drivers cap at: a measured rate within 2% of one is not a headline (the old harness's list).
pub const CAP_VALUES: [f64; 13] = [
    24.0, 30.0, 48.0, 50.0, 60.0, 72.0, 75.0, 90.0, 100.0, 120.0, 144.0, 165.0, 240.0,
];

/// The frame limiters an environment asks for: variable and value for every limit that is not 0.
pub fn limiters_in(env: impl Fn(&str) -> Option<String>) -> Vec<(String, String)> {
    ["NV_FRAME_LIMIT", "NV_FPS_LIMIT", "NV_D3D9_FRAME_LIMIT"]
        .iter()
        .filter_map(|k| {
            let v = env(k)?.trim().to_string();
            (!v.is_empty() && !["0", "off", "none"].contains(&v.as_str()))
                .then(|| ((*k).to_string(), v))
        })
        .collect()
}

/// Why a run's fps is not a headline, or `None`: a limiter was set, or the measured fps sits within 2% of a usual cap value.
pub fn capped_reason(limiters: &[(String, String)], fps: f64) -> Option<String> {
    if !limiters.is_empty() {
        let l: Vec<String> = limiters.iter().map(|(k, v)| format!("{k}={v}")).collect();
        return Some(format!("limiter set: {}", l.join(", ")));
    }
    CAP_VALUES
        .iter()
        .find(|c| (fps - **c).abs() / **c <= 0.02)
        .map(|c| format!("measured {fps:.0} fps is within 2% of the {c:.0} fps cap value"))
}

/// What a run was and how it ended, for the summary and the headline.
#[derive(Clone, Debug)]
pub struct RunFacts {
    /// The run's name (a directory name).
    pub name: String,
    /// The suite or scenario ("quick").
    pub scenario: String,
    pub resolution: String,
    pub limiters: Vec<(String, String)>,
    pub wall_s: f64,
    /// Another `nv-viewer` was running while this one did.
    pub noisy: bool,
    pub exit_status: i32,
    pub stops: u32,
    pub errors: u32,
    pub warnings: u32,
    pub governor: String,
    pub loadavg: String,
    pub cpu: String,
    /// Frames before the timed ones, and passes of the timed ones.
    pub warmup: u32,
    pub passes: u32,
}

/// One headline row of a place: what a person wants first (the Python harness's `headline`).
#[derive(Clone, Debug, PartialEq)]
pub struct Headline {
    pub scenario: String,
    pub run: String,
    pub fps: f64,
    pub p50_ms: f64,
    pub p99_ms: f64,
    pub low1_fps: f64,
    pub low01_fps: f64,
    pub capped: Option<String>,
    pub wall_s: f64,
    pub verdict: &'static str,
    pub checks_ok: bool,
    pub noisy: bool,
}

/// The place's checks (all must pass): the engine ended with status 0, no stop, no error line.
pub fn checks(f: &RunFacts) -> Vec<(&'static str, bool, String)> {
    vec![
        (
            "the engine ended with status 0",
            f.exit_status == 0,
            format!("exit status {}", f.exit_status),
        ),
        (
            "no stop (Game's main stopped)",
            f.stops == 0,
            format!("{} stops", f.stops),
        ),
        (
            "no error line in the log",
            f.errors == 0,
            format!("{} error lines", f.errors),
        ),
    ]
}

pub fn headline(suite: &str, r: &PlaceRow, f: &RunFacts) -> Headline {
    let ok = checks(f).iter().all(|c| c.1);
    let capped = capped_reason(&f.limiters, r.fps());
    Headline {
        scenario: format!("{suite}:{}", r.place),
        run: f.name.clone(),
        fps: r.fps(),
        p50_ms: r.p50_ms,
        p99_ms: r.p99_ms,
        low1_fps: r.low1_fps,
        low01_fps: r.low01_fps,
        verdict: if capped.is_some() {
            "CAPPED"
        } else if ok {
            "PASS"
        } else {
            "FAIL"
        },
        capped,
        wall_s: f.wall_s,
        checks_ok: ok,
        noisy: f.noisy,
    }
}

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn num(v: f64) -> String {
    if v.is_finite() {
        format!("{v:.3}")
    } else {
        "null".into()
    }
}

fn opt(v: &Option<String>) -> String {
    v.as_deref().map_or("null".into(), esc)
}

impl Headline {
    pub fn json(&self) -> String {
        format!(
            "{{\"scenario\":{},\"run\":{},\"mode\":\"uncapped\",\"window\":\"offscreen\",\"fps\":{},\"p50_ms\":{},\"p99_ms\":{},\"low1_fps\":{},\"low01_fps\":{},\"busy_ms\":null,\"hitches_per_min\":null,\"vram_peak_mib\":null,\"capped\":{},\"wall_s\":{},\"too_long\":false,\"verdict\":{},\"checks_ok\":{},\"noisy\":{}}}",
            esc(&self.scenario), esc(&self.run), num(self.fps), num(self.p50_ms), num(self.p99_ms), num(self.low1_fps),
            num(self.low01_fps), opt(&self.capped), num(self.wall_s), esc(self.verdict), self.checks_ok, self.noisy
        )
    }
}

/// The `summary.json` of one run in the harness's shape (`name`, `scenario`, `settings`, `run`, `metrics`, `verdict`,
/// `headline`), plus `schema_version` 1. The metrics the engine path measures are filled in; the ones it has no source
/// for (main thread busy, VRAM, hitches) are `null`, the memory guard and the input driver are not part of this path.
pub fn summary_json(suite: &str, rows: &[PlaceRow], f: &RunFacts) -> String {
    let cs = checks(f);
    let ok = cs.iter().all(|c| c.1);
    let checks_json: Vec<String> = cs
        .iter()
        .map(|(n, ok, d)| format!("{{\"name\":{},\"ok\":{ok},\"detail\":{}}}", esc(n), esc(d)))
        .collect();
    let phases: Vec<String> = rows
        .iter()
        .map(|r| {
            format!(
                "{{\"name\":{},\"fps\":{},\"avg_ms\":{},\"p50_ms\":{},\"p99_ms\":{},\"low1_fps\":{},\"low01_fps\":{},\"frames\":{},\"passes\":{},\"spread_pct\":{}}}",
                esc(&format!("{}.measured", r.place)), num(r.fps()), num(r.avg_ms), num(r.p50_ms), num(r.p99_ms),
                num(r.low1_fps), num(r.low01_fps), r.frames, r.passes, num(r.spread_pct)
            )
        })
        .collect();
    // Across places: the frame-weighted mean of the places' averages is the headline of a whole run.
    let frames: f64 = rows.iter().map(|r| r.frames as f64).sum();
    let avg = rows.iter().map(|r| r.avg_ms * r.frames as f64).sum::<f64>() / frames.max(1.0);
    let worst = |g: fn(&PlaceRow) -> f64, hi: bool| {
        rows.iter()
            .map(g)
            .fold(if hi { 0.0 } else { f64::MAX }, |a, b| {
                if hi {
                    a.max(b)
                } else {
                    a.min(b)
                }
            })
    };
    let play = format!(
        "{{\"fps\":{},\"avg_ms\":{},\"p50_ms\":{},\"p99_ms\":{},\"low1_fps\":{},\"low01_fps\":{},\"max_ms\":{}}}",
        num(1000.0 / avg.max(1e-9)), num(avg), num(worst(|r| r.p50_ms, true)), num(worst(|r| r.p99_ms, true)),
        num(worst(|r| r.low1_fps, false)), num(worst(|r| r.low01_fps, false)), num(worst(|r| r.max_ms, true))
    );
    let head = rows
        .first()
        .map(|r| headline(suite, r, f).json())
        .unwrap_or_else(|| "null".into());
    let limiters: Vec<String> = f
        .limiters
        .iter()
        .map(|(k, v)| format!("{}:{}", esc(k), esc(v)))
        .collect();
    format!(
        "{{\"schema_version\":1,\"name\":{},\"scenario\":{},\"settings\":{{\"mode\":\"uncapped\",\"window\":\"offscreen\",\"resolution\":{},\"limiters\":{{{}}},\"clock\":\"real\",\"warmup_frames\":{},\"passes\":{}}},\"run\":{{\"wall_s\":{},\"noisy\":{},\"ended_by\":{},\"exit_status\":{},\"governor\":{},\"loadavg\":{},\"cpu\":{}}},\"metrics\":{{\"frame_time_play\":{play},\"phases\":[{}],\"main_busy_pct\":null,\"hitches\":{{\"per_min\":null}},\"memory\":{{\"vram_peak_mib\":null}},\"stops\":{},\"error_lines\":{},\"warnings\":{},\"exit_status\":{}}},\"verdict\":{{\"ok\":{ok},\"checks\":[{}]}},\"headline\":{head}}}\n",
        esc(&f.name), esc(suite), esc(&f.resolution), limiters.join(","), f.warmup, f.passes, num(f.wall_s), f.noisy,
        esc(if f.exit_status == 0 { "finished" } else { "engine_exit" }), f.exit_status, esc(&f.governor),
        esc(&f.loadavg), esc(&f.cpu), phases.join(","), f.stops, f.errors, f.warnings, f.exit_status, checks_json.join(",")
    )
}

/// `(new - old) / old` in percent; `None` without both or an old of 0.
pub fn delta_pct(new: Option<f64>, old: Option<f64>) -> Option<f64> {
    match (new, old) {
        (Some(n), Some(o)) if o != 0.0 => Some((n - o) / o * 100.0),
        _ => None,
    }
}

/// A scenario's fps in an earlier suite or a baseline (scenario, fps).
pub type Known = Vec<(String, f64)>;

/// The suite's Markdown table: the columns of the Python harness, the fps change against the previous suite and the
/// baseline when given, NOISY marks.
pub fn table(rows: &[Headline], previous: Option<&Known>, baseline: Option<&Known>) -> String {
    let mut head = vec![
        "Scenario",
        "avg fps",
        "p50 ms",
        "p99 ms",
        "1% low",
        "0.1% low",
        "main busy ms/frame",
        "hitches/min",
        "VRAM peak MiB",
        "verdict",
        "noisy",
    ];
    if previous.is_some() {
        head.push("fps vs previous");
    }
    if baseline.is_some() {
        head.push("fps vs baseline");
    }
    let mut out = vec![
        format!("| {} |", head.join(" | ")),
        format!(
            "| {} |",
            head.iter().map(|_| "---").collect::<Vec<_>>().join(" | ")
        ),
    ];
    for r in rows {
        let mut c = vec![
            r.scenario.clone(),
            format!("{:.0}", r.fps),
            format!("{:.2}", r.p50_ms),
            format!("{:.2}", r.p99_ms),
            format!("{:.0}", r.low1_fps),
            format!("{:.0}", r.low01_fps),
            "n/a".into(),
            "n/a".into(),
            "n/a".into(),
            r.verdict.to_string(),
            if r.noisy {
                "NOISY".into()
            } else {
                String::new()
            },
        ];
        for other in [previous, baseline].into_iter().flatten() {
            let old = other
                .iter()
                .find(|(s, _)| *s == r.scenario)
                .map(|(_, f)| *f);
            c.push(delta_pct(Some(r.fps), old).map_or("n/a".into(), |d| format!("{d:+.1}%")));
        }
        out.push(format!("| {} |", c.join(" | ")));
    }
    out.join("\n")
}

pub fn suite_markdown(
    name: &str,
    rows: &[Headline],
    previous: Option<&Known>,
    baseline: Option<&Known>,
    baseline_name: Option<&str>,
    resolution: &str,
) -> String {
    let mut out = vec![
        format!("# Bench suite {name}"),
        String::new(),
        format!("Mode uncapped (as real play), offscreen, {resolution}."),
        String::new(),
        table(rows, previous, baseline),
    ];
    for r in rows {
        if let Some(c) = &r.capped {
            out.push(format!(
                "\nCAPPED: {} ({c}): its fps is not a headline number.",
                r.scenario
            ));
        }
    }
    if rows.iter().any(|r| r.noisy) {
        out.push(
            "\nNOISY: other nv-viewer processes ran during the run; its numbers are not valid."
                .into(),
        );
    }
    if let Some(b) = baseline_name {
        out.push(format!(
            "\nBaseline: {b}{}",
            if baseline.is_some() {
                ""
            } else {
                " (not found)"
            }
        ));
    }
    out.join("\n") + "\n"
}

pub fn suite_json(name: &str, rows: &[Headline]) -> String {
    let rows: Vec<String> = rows.iter().map(Headline::json).collect();
    format!(
        "{{\"schema_version\":1,\"name\":{},\"rows\":[{}]}}\n",
        esc(name),
        rows.join(",")
    )
}

/// A baseline file: `{scenario: row}` as the Python harness wrote it, with `schema_version` among the keys.
pub fn baseline_json(rows: &[Headline]) -> String {
    let items: Vec<String> = rows
        .iter()
        .map(|r| format!("{}:{}", esc(&r.scenario), r.json()))
        .collect();
    format!("{{\"schema_version\":1,{}}}\n", items.join(","))
}

/// The `(scenario, fps)` pairs of a baseline file or a suite's `rows` (the harness's files and this one's alike).
pub fn known_from(text: &str) -> Known {
    let mut out = Vec::new();
    // Every row object holds `"scenario":"NAME"` and, later, `"fps":N`: read them in pairs, in order.
    let mut rest = text;
    while let Some(at) = rest.find("\"scenario\":") {
        rest = &rest[at + 11..];
        let rest_trim = rest.trim_start();
        let Some(body) = rest_trim.strip_prefix('"') else {
            continue;
        };
        let Some(end) = body.find('"') else { break };
        let scenario = body[..end].to_string();
        let row_end = rest.find("\"scenario\":").unwrap_or(rest.len());
        let row = &rest[..row_end];
        if let Some(f) = row.find("\"fps\":") {
            let v = &row[f + 6..];
            let e = v.find([',', '}']).unwrap_or(v.len());
            if let Ok(fps) = v[..e].trim().parse::<f64>() {
                out.push((scenario, fps));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> RunFacts {
        RunFacts {
            name: "r1".into(),
            scenario: "quick".into(),
            resolution: "1280x720".into(),
            limiters: vec![],
            wall_s: 42.0,
            noisy: false,
            exit_status: 0,
            stops: 0,
            errors: 0,
            warnings: 1,
            governor: "performance".into(),
            loadavg: "0.1 0.2 0.3".into(),
            cpu: "Some \"CPU\"".into(),
            warmup: 120,
            passes: 5,
        }
    }

    fn row(place: &str, avg: f64) -> PlaceRow {
        PlaceRow::parse(&format!("{place} frames 2500 avg_ms {avg} p50_ms 5.1 p99_ms 8.2 max_ms 12.0 low1_fps 90.5 low01_fps 70.1 passes 5 spread_pct 2.5")).unwrap()
    }

    #[test]
    fn a_row_is_read_by_key_and_the_old_form_still_parses() {
        let r = row("goodsprings", 6.25);
        assert_eq!((r.frames, r.passes), (2500, 5));
        assert!((r.fps() - 160.0).abs() < 1e-9 && r.low01_fps == 70.1);
        let old = PlaceRow::parse("a frames 500 avg_ms 7.0 p99_ms 9.6 max_ms 13.0 low1_fps 91.2")
            .unwrap();
        assert_eq!((old.passes, old.p50_ms), (1, 0.0));
        assert!(PlaceRow::parse("a frames 5").is_none());
    }

    #[test]
    fn capped_means_a_limiter_or_a_rate_within_two_percent_of_a_cap() {
        let env = |k: &str| match k {
            "NV_FRAME_LIMIT" => Some("60".to_string()),
            "NV_FPS_LIMIT" => Some("0".to_string()),
            _ => None,
        };
        let l = limiters_in(env);
        assert_eq!(l, vec![("NV_FRAME_LIMIT".to_string(), "60".to_string())]);
        assert!(capped_reason(&l, 200.0).unwrap().contains("limiter set"));
        assert!(capped_reason(&[], 59.5).unwrap().contains("60 fps cap"));
        assert!(capped_reason(&[], 160.0).is_none());
    }

    #[test]
    fn the_headline_and_the_summary_carry_the_contract_keys() {
        let f = facts();
        let h = headline("quick", &row("goodsprings", 6.25), &f);
        assert_eq!(
            (h.scenario.as_str(), h.verdict),
            ("quick:goodsprings", "PASS")
        );
        let s = summary_json("quick", &[row("goodsprings", 6.25), row("strip", 8.0)], &f);
        for key in [
            "\"schema_version\":1",
            "\"frame_time_play\"",
            "\"phases\"",
            "\"goodsprings.measured\"",
            "\"verdict\":{\"ok\":true",
            "\"headline\"",
            "\"noisy\":false",
            "\"governor\":\"performance\"",
            "Some \\\"CPU\\\"",
        ] {
            assert!(s.contains(key), "{key} in {s}");
        }
        let mut bad = facts();
        bad.exit_status = 1;
        assert_eq!(headline("quick", &row("a", 6.25), &bad).verdict, "FAIL");
        let mut capped = facts();
        capped.limiters = vec![("NV_FRAME_LIMIT".into(), "60".into())];
        assert_eq!(
            headline("quick", &row("a", 6.25), &capped).verdict,
            "CAPPED"
        );
    }

    #[test]
    fn the_table_shows_the_change_against_previous_and_baseline() {
        let f = facts();
        let rows = vec![
            headline("quick", &row("goodsprings", 5.0), &f),
            headline("quick", &row("strip", 10.0), &f),
        ];
        let prev: Known = vec![("quick:goodsprings".into(), 160.0)];
        let base: Known = vec![
            ("quick:goodsprings".into(), 250.0),
            ("quick:strip".into(), 100.0),
        ];
        let t = table(&rows, Some(&prev), Some(&base));
        assert!(t.contains("fps vs previous | fps vs baseline"));
        assert!(t.contains("| +25.0% | -20.0% |"), "{t}");
        assert!(
            t.contains("| n/a | +0.0% |") || t.contains("| n/a | 0.0% |"),
            "{t}"
        );
        let md = suite_markdown("suite-1", &rows, None, None, Some("old"), "1280x720");
        assert!(md.contains("Baseline: old (not found)"));
    }

    #[test]
    fn baselines_and_suites_read_back_and_the_old_harness_files_do_too() {
        let f = facts();
        let rows = vec![
            headline("quick", &row("goodsprings", 5.0), &f),
            headline("quick", &row("strip", 10.0), &f),
        ];
        let k = known_from(&baseline_json(&rows));
        assert_eq!(k.len(), 2);
        assert_eq!(k[0].0, "quick:goodsprings");
        assert!((k[0].1 - 200.0).abs() < 1e-3);
        assert_eq!(known_from(&suite_json("s", &rows)), k);
        // The Python harness's file: {scenario: row} with a space after the colons.
        let old =
            "{\"quick\": {\"scenario\": \"quick\", \"run\": \"x\", \"fps\": 97.5, \"p50_ms\": 1}}";
        assert_eq!(known_from(old), vec![("quick".to_string(), 97.5)]);
    }
}
