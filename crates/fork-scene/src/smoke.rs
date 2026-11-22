// SPDX-License-Identifier: GPL-3.0-only
//! `nv-rs-scene smoke-report LOG RC SECONDS`: the checklist `nv-rs-dev smoke` prints for the log of one windowed run
//! (docs/fork/LOCAL.md). Numbers and verdicts only; it never echoes game data.

/// One line of the checklist.
#[derive(Clone, Debug, PartialEq)]
pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
}

fn find<'a>(lines: &[&'a str], keep: impl Fn(&str) -> bool) -> Vec<&'a str> {
    lines
        .iter()
        .copied()
        .filter(|l| keep(&l.to_ascii_lowercase()))
        .collect()
}

/// The digits (with a dot between digits) that end `s`, if any.
fn number_before(s: &str) -> Option<&str> {
    let t = s.trim_end();
    let start = t
        .rfind(|c: char| !(c.is_ascii_digit() || c == '.'))
        .map_or(0, |i| i + t[i..].chars().next().map_or(1, char::len_utf8));
    let n = &t[start..];
    (!n.is_empty() && n.chars().next().is_some_and(|c| c.is_ascii_digit())).then_some(n)
}

/// The digits that start `s` (after spaces), if any.
fn number_after(s: &str) -> Option<&str> {
    let t = s.trim_start();
    let end = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
    (end > 0).then(|| &t[..end])
}

/// `WIDTHxHEIGHT` (spaces allowed round the `x`) and the text after it.
fn size_in(line: &str) -> Option<((&str, &str), &str)> {
    for (i, _) in line.match_indices(['x', 'X']) {
        if let (Some(w), Some(h)) = (number_before(&line[..i]), number_after(&line[i + 1..])) {
            let rest_at = line[i + 1..].trim_start().len();
            let after = &line[line.len() - rest_at + h.len()..];
            return Some(((w, h), after));
        }
    }
    None
}

/// The number before the first `hz` (any case).
fn hz_in(s: &str) -> Option<String> {
    let low = s.to_ascii_lowercase();
    let i = low.find("hz")?;
    number_before(&s[..i]).map(String::from)
}

fn numbers_with_suffix(lines: &[&str], suffix: &str) -> Vec<u64> {
    let mut out = Vec::new();
    for l in lines {
        for (i, _) in l.match_indices(suffix) {
            if let Some(n) = number_before(&l[..i]).and_then(|n| n.parse().ok()) {
                out.push(n);
            }
        }
    }
    out
}

/// The checklist for a log's text.
pub fn checks(text: &str) -> Vec<Check> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut check =
        |name: &'static str, ok: bool, detail: String| out.push(Check { name, ok, detail });

    let bad_window = find(&lines, |l| {
        l.contains("could not be made") || l.contains("offscreen") || l.contains("off screen")
    });
    let real = !find(&lines, |l| l.contains("is real window")).is_empty();
    check(
        "real window created",
        bad_window.is_empty() && real,
        if bad_window.is_empty() {
            String::new()
        } else {
            format!("{} bad-window lines", bad_window.len())
        },
    );

    let display = find(&lines, |l| l.contains("display"))
        .into_iter()
        .find_map(|l| {
            size_in(l).and_then(|((w, h), rest)| hz_in(rest).map(|hz| format!("{w}x{h} @ {hz} Hz")))
        });
    check(
        "display line (size, Hz)",
        display.is_some(),
        display.unwrap_or_else(|| "none".into()),
    );

    let frame_lines = find(&lines, |l| {
        l.contains("movement: main ") && l.contains(" frames")
    });
    let frames: Vec<u64> = frame_lines
        .iter()
        .filter_map(|l| {
            l.split("main ")
                .nth(1)
                .and_then(number_after)
                .and_then(|n| n.parse().ok())
        })
        .collect();
    let (lo, hi) = (frames.iter().min().copied(), frames.iter().max().copied());
    check(
        "frames advanced",
        frames.len() >= 2 && hi > lo,
        match (lo, hi) {
            (Some(a), Some(b)) => format!("{a}..{b}"),
            _ => "no frame counters".into(),
        },
    );

    let reports = numbers_with_suffix(&lines, " wgpu errors");
    let other = find(&lines, |l| {
        l.contains("wgpu")
            && !l.contains(" wgpu errors")
            && (l.contains("validation")
                || l.contains("panic")
                || l.contains("error:")
                || l.contains("device lost"))
    });
    let nerr = reports.iter().max().copied().unwrap_or(0) as usize + other.len();
    check(
        "wgpu errors == 0",
        !reports.is_empty() && nerr == 0,
        if reports.is_empty() {
            "no wgpu report line".into()
        } else {
            format!("{nerr} errors over {} reports", reports.len())
        },
    );

    // A game stop is a line that starts with "stopped:" or "game stopped:"; the ledger's "static initializer skipped: ...
    // stopped: open function" notes are information, not a stop.
    let is_stop = |l: &str| {
        let t = l
            .trim_start_matches(|c: char| !c.is_alphanumeric())
            .to_ascii_lowercase();
        t.starts_with("stopped:") || t.starts_with("game stopped:")
    };
    let stops = lines.iter().filter(|l| is_stop(l)).count();
    let skipped = find(&lines, |l| l.contains("stopped:"))
        .len()
        .saturating_sub(stops);
    check(
        "no 'stopped:' line",
        stops == 0,
        format!("{stops} lines; {skipped} untranslated-function notes"),
    );

    let mem = find(&lines, |l| {
        l.contains("not enough memory")
            || l.contains("out of memory")
            || l.contains("device lost")
            || l.contains("devicelost")
            || l.contains("device-lost")
    });
    check(
        "no memory / device-lost lines",
        mem.is_empty(),
        format!("{} lines", mem.len()),
    );

    let snd = find(&lines, |l| {
        l.contains("sound") || l.contains("audio") || l.contains("pulse")
    });
    check(
        "sound line present",
        !snd.is_empty(),
        format!("{} lines", snd.len()),
    );

    let panics = find(&lines, |l| l.contains("panicked at"));
    check(
        "no panic",
        panics.is_empty(),
        format!("{} lines", panics.len()),
    );

    // The NV_MEMLOG timeline (docs/fork/MEMLOG.md): informational, present only when the line is.
    let memlog: Vec<Vec<(&str, u64)>> = lines
        .iter()
        .filter(|l| l.starts_with("NV_MEMLOG "))
        .map(|l| {
            l.split_whitespace()
                .skip(1)
                .filter_map(|kv| kv.split_once('='))
                .filter_map(|(k, v)| v.parse().ok().map(|v| (k, v)))
                .collect()
        })
        .collect();
    if let Some(last) = memlog.last() {
        let get =
            |row: &[(&str, u64)], k: &str| row.iter().find(|(n, _)| *n == k).map_or(0, |(_, v)| *v);
        let mb = |row: &[(&str, u64)], k: &str| get(row, k) as f64 / 1_048_576.0;
        let peaks: Vec<u64> = memlog.iter().map(|r| get(r, "rss_peak")).collect();
        check(
            "memlog: rss peak and guest heap",
            peaks.windows(2).all(|w| w[0] <= w[1]),
            format!(
                "{} lines; peak RSS {:.0} MB, guest heap {:.0} MB used, peak {:.0} MB of {:.0} MB",
                memlog.len(),
                mb(last, "rss_peak"),
                mb(last, "heap_used"),
                mb(last, "heap_peak"),
                mb(last, "heap_cap")
            ),
        );
    }
    out
}

/// The text the command prints, and whether every check passed.
pub fn report(text: &str, log: &str, rc: &str, secs: &str) -> (String, bool) {
    let results = checks(text);
    let mut s = String::new();
    for c in &results {
        s.push_str(&format!(
            "{}  {}{}\n",
            if c.ok { "PASS" } else { "FAIL" },
            c.name,
            if c.detail.is_empty() {
                String::new()
            } else {
                format!("  [{}]", c.detail)
            }
        ));
    }
    let all = results.iter().all(|c| c.ok);
    s.push_str(&format!(
        "{}: {}/{} checks; ran {secs} s, exit {rc}; log {log}\n",
        if all { "PASS" } else { "FAIL" },
        results.iter().filter(|c| c.ok).count(),
        results.len()
    ));
    (s, all)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "window: is real window 1920x1080\nDisplay 2560 x 1440 at 143.97 Hz\nmovement: main 100 frames\nmovement: main 460 frames\n\
        present: 0 wgpu errors\nsound: pulse backend open\nstatic initializer skipped: foo; stopped: open function\n";

    #[test]
    fn a_good_log_passes_every_check_and_the_stop_note_is_information() {
        let (text, ok) = report(GOOD, "run.log", "0", "60");
        assert!(ok, "{text}");
        assert!(
            text.contains("PASS  display line (size, Hz)  [2560x1440 @ 143.97 Hz]"),
            "{text}"
        );
        assert!(text.contains("PASS  frames advanced  [100..460]"), "{text}");
        assert!(
            text.contains("PASS  no 'stopped:' line  [0 lines; 1 untranslated-function notes]"),
            "{text}"
        );
        assert!(
            text.ends_with("PASS: 8/8 checks; ran 60 s, exit 0; log run.log\n"),
            "{text}"
        );
    }

    #[test]
    fn each_failure_is_named() {
        let bad = "window could not be made\nstopped: unimplemented\nwgpu validation error: x\npanicked at src/x.rs:1\nout of memory\n";
        let (text, ok) = report(bad, "l", "1", "5");
        assert!(!ok);
        for name in [
            "FAIL  real window created  [1 bad-window lines]",
            "FAIL  display line (size, Hz)  [none]",
            "FAIL  frames advanced  [no frame counters]",
            "FAIL  wgpu errors == 0  [no wgpu report line]",
            "FAIL  no 'stopped:' line  [1 lines; 0 untranslated-function notes]",
            "FAIL  no memory / device-lost lines  [1 lines]",
            "FAIL  sound line present",
            "FAIL  no panic  [1 lines]",
        ] {
            assert!(text.contains(name), "{name} in {text}");
        }
    }

    #[test]
    fn the_memory_log_is_read_when_present_and_must_not_shrink() {
        let ok = format!("{GOOD}NV_MEMLOG rss_peak=1048576000 heap_used=2097152 heap_peak=4194304 heap_cap=8388608\n");
        let c = checks(&ok);
        let last = c.last().unwrap();
        assert!(
            last.ok
                && last
                    .detail
                    .contains("peak RSS 1000 MB, guest heap 2 MB used, peak 4 MB of 8 MB"),
            "{last:?}"
        );
        let shrinking = format!("{GOOD}NV_MEMLOG rss_peak=9\nNV_MEMLOG rss_peak=3\n");
        assert!(!checks(&shrinking).last().unwrap().ok);
    }
}
