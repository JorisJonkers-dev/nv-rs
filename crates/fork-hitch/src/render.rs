//! The hitch record as one JSON object, and the small text line. Pure: it takes what the
//! recorder collected and returns strings.

use crate::counters::{Counter, Unit, ALL};
use crate::model::{Rec, Verdict};
use crate::sampler::{ext_text, Samples};
use std::fmt::Write;

/// An event kept in the preceding moments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    pub age_ms: u64,
    pub kind: String,
    pub arg: u64,
}

/// Everything one hitch record says.
pub struct Hitch<'a> {
    pub rec: &'a Rec,
    pub before: &'a [Rec],
    pub median_ms: f64,
    pub threshold_ms: f64,
    pub verdict: &'a Verdict,
    pub trigger: &'a str,
    pub events: &'a [Seen],
    pub samples: Option<&'a Samples>,
    pub detail: bool,
    pub brief: bool,
    pub name_of: &'a dyn Fn(u32) -> String,
}

fn ms(ns: u64) -> String {
    format!("{:.3}", ns as f64 / 1e6)
}

fn text(o: &mut String, s: &str) {
    fork_log::json_string(o, s);
}

/// One frame's full breakdown: the time of each counter in ms (counts and bytes as they are),
/// faults, CPU, and the player's context.
pub fn frame_json(o: &mut String, r: &Rec) {
    let _ = write!(
        o,
        "{{\"idx\":{},\"end_ms\":{},\"wall_ms\":{}",
        r.idx,
        r.end_ms,
        ms(r.wall_ns)
    );
    for c in ALL {
        let v = r.get(*c);
        if v == 0 {
            continue;
        }
        match c.unit() {
            Unit::Ns => {
                let _ = write!(o, ",\"{}_ms\":{}", c.name(), ms(v));
            }
            Unit::Count | Unit::Bytes => {
                let _ = write!(o, ",\"{}\":{v}", c.name());
            }
        }
    }
    let _ = write!(
        o,
        ",\"minflt\":{},\"majflt\":{},\"rss_growth_kb\":{},\"cpu_ms\":{:.3}",
        r.minflt,
        r.majflt,
        r.rss_growth_kb,
        r.cpu_us as f64 / 1e3
    );
    let c = &r.ctx;
    let _ = write!(
        o,
        ",\"cell\":\"{:08x}\",\"world\":\"{:08x}\",\"pos\":[{:.1},{:.1},{:.1}],\"screen\":\"{}\"}}",
        c.cell,
        c.world,
        c.pos[0],
        c.pos[1],
        c.pos[2],
        c.screen.name()
    );
}

/// A frame in brief: index, wall time, and the three largest counters' times.
fn brief_json(o: &mut String, r: &Rec) {
    let _ = write!(o, "{{\"idx\":{},\"wall_ms\":{}", r.idx, ms(r.wall_ns));
    let mut top: Vec<(Counter, u64)> = ALL
        .iter()
        .filter(|c| c.unit() == Unit::Ns)
        .map(|c| (*c, r.get(*c)))
        .filter(|(_, v)| *v > 0)
        .collect();
    top.sort_by_key(|t| std::cmp::Reverse(t.1));
    for (c, v) in top.into_iter().take(3) {
        let _ = write!(o, ",\"{}_ms\":{}", c.name(), ms(v));
    }
    let _ = write!(o, ",\"draws\":{}}}", r.get(Counter::Draws));
}

/// The body of the record, `"hitch":{...}`, for [`fork_log::json_record`].
pub fn hitch_body(h: &Hitch<'_>) -> String {
    let mut o = String::with_capacity(2048);
    let _ = write!(
        o,
        "\"hitch\":{{\"frame_idx\":{},\"wall_ms\":{},\"median_ms\":{:.3},\"threshold_ms\":{:.1},\"cause\":",
        h.rec.idx,
        ms(h.rec.wall_ns),
        h.median_ms,
        h.threshold_ms
    );
    text(&mut o, h.verdict.cause.name());
    let _ = write!(
        o,
        ",\"detail\":{},\"brief\":{},\"buckets\":{{",
        h.detail, h.brief
    );
    for (i, (c, ns)) in h.verdict.buckets.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        text(&mut o, c.name());
        let _ = write!(o, ":{}", ms(*ns));
    }
    let _ = write!(
        o,
        "}},\"platform_other_ms\":{},\"trigger\":",
        ms(h.verdict.platform_other_ns)
    );
    text(&mut o, h.trigger);
    o.push_str(",\"frame\":");
    frame_json(&mut o, h.rec);
    o.push_str(",\"events\":[");
    for (i, e) in h.events.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        let _ = write!(o, "{{\"age_ms\":{},\"kind\":", e.age_ms);
        text(&mut o, &e.kind);
        let _ = write!(o, ",\"arg\":{}}}", e.arg);
    }
    o.push(']');
    if !h.brief {
        o.push_str(",\"before\":[");
        for (i, r) in h.before.iter().enumerate() {
            if i > 0 {
                o.push(',');
            }
            if h.detail {
                frame_json(&mut o, r);
            } else {
                brief_json(&mut o, r);
            }
        }
        o.push(']');
        if let Some(s) = h.samples {
            o.push_str(",\"samples\":");
            samples_json(&mut o, s, h.name_of);
        }
    }
    o.push('}');
    o
}

fn samples_json(o: &mut String, s: &Samples, name_of: &dyn Fn(u32) -> String) {
    let _ = write!(
        o,
        "{{\"n\":{},\"in_guest\":{},\"in_platform\":{},\"longest_call_ms\":{},\"calls\":[",
        s.n,
        s.in_guest,
        s.n - s.in_guest.min(s.n),
        ms(s.longest_call_ns)
    );
    let mut calls: Vec<_> = s.calls.iter().collect();
    calls.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (i, ((slot, ext, hash, top), n)) in calls.into_iter().take(6).enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str("{\"call\":");
        text(o, &name_of(*slot));
        let _ = write!(o, ",\"n\":{n},\"caller\":\"{top:08x}\"");
        if *ext != 0 || *hash != 0 {
            o.push_str(",\"arg\":");
            let e = ext_text(*ext);
            text(o, &format!("file ext:{e} hash:{hash:08x}"));
        }
        o.push('}');
    }
    o.push_str("],\"last_call_chain\":[");
    for (i, a) in s.last_chain.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        let _ = write!(o, "\"{a:08x}\"");
    }
    o.push_str("],\"hot\":[");
    let mut hot: Vec<_> = s.hot.iter().collect();
    hot.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (i, (a, n)) in hot.into_iter().take(8).enumerate() {
        if i > 0 {
            o.push(',');
        }
        let _ = write!(o, "{{\"addr\":\"{a:08x}\",\"n\":{n}}}");
    }
    o.push_str("]}");
}

/// The text line a hitch gets on the terminal.
pub fn hitch_line(h: &Hitch<'_>) -> String {
    let mut line = format!(
        "hitch: {:.1} ms at frame {} (median {:.1} ms), cause {}",
        h.rec.wall_ms(),
        h.rec.idx,
        h.median_ms,
        h.verdict.cause.name()
    );
    if let Some((c, ns)) = h.verdict.buckets.first() {
        let _ = write!(line, " ({} {:.1} ms)", c.name(), *ns as f64 / 1e6);
    }
    let _ = write!(line, ", trigger {}", h.trigger);
    if let Some(s) = h.samples {
        let mut calls: Vec<_> = s.calls.iter().collect();
        calls.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        if let Some(((slot, ..), n)) = calls.first() {
            let _ = write!(line, ", in {} for {n}/{} samples", (h.name_of)(*slot), s.n);
        } else if s.in_guest > 0 {
            let _ = write!(
                line,
                ", in the game's code for {}/{} samples",
                s.in_guest, s.n
            );
            if let Some(a) = s.hot.keys().next().or(s.last_chain.last()) {
                let _ = write!(line, " (after {a:08x})");
            }
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{classify, Context, Screen};

    fn rec() -> Rec {
        let mut r = Rec {
            idx: 42,
            end_ms: 9000,
            wall_ns: 90_000_000,
            ctx: Context {
                cell: 0x1658b7,
                world: 0xda726,
                pos: [1.0, 2.5, -3.0],
                screen: Screen::Play,
            },
            ..Rec::default()
        };
        r.d[Counter::FileNs as usize] = 70_000_000;
        r.d[Counter::PlatformNs as usize] = 72_000_000;
        r.d[Counter::FileBytes as usize] = 4096;
        r
    }

    #[test]
    fn the_record_is_json_with_every_part() {
        let r = rec();
        let v = classify(&r, false);
        let mut s = Samples {
            epoch: 1,
            n: 10,
            in_guest: 2,
            ..Samples::default()
        };
        s.calls.insert((7, 0x0066_696e, 0xabcd, 0x00401000), 8);
        s.hot.insert(0x00401000, 8);
        let name = |slot: u32| format!("ReadFile#{slot}");
        let events = [Seen {
            age_ms: 120,
            kind: "cell.change".into(),
            arg: 5,
        }];
        let before = [Rec::default(), Rec::default()];
        let h = Hitch {
            rec: &r,
            before: &before,
            median_ms: 7.1,
            threshold_ms: 25.0,
            verdict: &v,
            trigger: "cell.change",
            events: &events,
            samples: Some(&s),
            detail: false,
            brief: false,
            name_of: &name,
        };
        let body = hitch_body(&h);
        let text = format!("{{{body}}}");
        // Balanced braces and brackets outside strings, and the parts there.
        let (mut depth, mut in_str, mut esc) = (0i32, false, false);
        for ch in text.chars() {
            if in_str {
                match (esc, ch) {
                    (true, _) => esc = false,
                    (false, '\\') => esc = true,
                    (false, '"') => in_str = false,
                    _ => {}
                }
            } else {
                match ch {
                    '"' => in_str = true,
                    '{' | '[' => depth += 1,
                    '}' | ']' => depth -= 1,
                    _ => {}
                }
            }
        }
        assert_eq!(depth, 0, "{text}");
        for part in [
            "\"cause\":\"file_io\"",
            "\"trigger\":\"cell.change\"",
            "\"file_ms\":70.000",
            "\"file_bytes\":4096",
            "\"cell\":\"001658b7\"",
            "\"before\":[",
            "\"call\":\"ReadFile#7\"",
            "file ext:nif hash:0000abcd",
            "\"addr\":\"00401000\"",
            "\"age_ms\":120",
        ] {
            assert!(text.contains(part), "{part} in {text}");
        }
        let line = hitch_line(&h);
        assert!(line.contains("90.0 ms at frame 42"), "{line}");
        assert!(line.contains("in ReadFile#7 for 8/10 samples"), "{line}");
        assert!(!text.contains('\n'));
    }

    #[test]
    fn a_brief_record_leaves_out_the_history() {
        let r = rec();
        let v = classify(&r, false);
        let name = |_: u32| String::new();
        let h = Hitch {
            rec: &r,
            before: &[Rec::default()],
            median_ms: 7.0,
            threshold_ms: 25.0,
            verdict: &v,
            trigger: "none",
            events: &[],
            samples: None,
            detail: false,
            brief: true,
            name_of: &name,
        };
        let body = hitch_body(&h);
        assert!(body.contains("\"brief\":true") && !body.contains("\"before\""));
    }
}
