//! `NV_PERF=1`: a per-frame CPU timeline of the wgpu backend and a report of the frames that
//! took much longer than the usual one, each with the cause the counters point at.
//!
//! Std only, and one relaxed load where it is off. A [`span`] adds its own time (its children's
//! taken out, so the categories add up) to a counter; [`frame_end`] (the start of each `Present`,
//! on the device's thread) turns the counters into one frame record; [`report`] prints, every
//! report interval, the per-category average and the frames over twice the median with their
//! dominant category. What no span covers (the game's own code, waiting for the recorded
//! commands) is `other`. Categories marked off-thread run on the queue thread and are shown
//! apart (they overlap the device thread).
//!
//! `NV_PERF_FILE=path` also appends one tab-separated line per frame (the frame's wall time and
//! the nanoseconds of each category) for plotting. Numbers only.

#![allow(clippy::needless_range_loop)]

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// What a span's time is counted as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(usize)]
pub(crate) enum Cat {
    /// Recording a draw (state hashing, pipeline lookup, bind groups, the draw call itself).
    Draw,
    /// A pipeline (and its shader modules) made on a draw.
    Pipeline,
    /// Ending and beginning render passes.
    Pass,
    /// A bind group or uniform buffer made on a draw.
    Group,
    /// Handing a batch to the queue thread (or submitting it here).
    Flush,
    /// A buffer or texture upload recorded or written.
    Upload,
    /// A resource made (buffer, texture, view, sampler).
    Create,
    /// A read-back: waiting for the GPU and mapping.
    ReadBack,
    /// Waiting for the queue thread.
    Sync,
    /// `Present`'s own work (the swap chain, the wait for the display).
    Present,
    /// Off-thread: the queue thread's write-and-submit of a batch.
    QueueSubmit,
}

const N: usize = 11;
const NAMES: [&str; N] = [
    "draw",
    "pipeline",
    "pass",
    "group",
    "flush",
    "upload",
    "create",
    "readback",
    "sync",
    "present",
    "queue_submit",
];
/// The categories from here on run on other threads and are not part of the device thread's sum.
const OFF_THREAD: usize = Cat::QueueSubmit as usize;

static ON: OnceLock<bool> = OnceLock::new();
static NS: [AtomicU64; N] = [const { AtomicU64::new(0) }; N];
static COUNT: [AtomicU64; N] = [const { AtomicU64::new(0) }; N];

thread_local! {
    /// The time of the spans that ended inside the one running (taken out of its own).
    static CHILD: Cell<u64> = const { Cell::new(0) };
}

/// Whether `NV_PERF` is on (read once).
#[inline]
pub(crate) fn enabled() -> bool {
    *ON.get_or_init(|| std::env::var("NV_PERF").is_ok_and(|v| !v.is_empty() && v != "0"))
}

/// A running span; its time is added when it drops.
pub(crate) struct Span {
    cat: Cat,
    start: Option<(Instant, u64)>,
}

/// Starts a span of `cat` (nothing when `NV_PERF` is off).
#[inline]
pub(crate) fn span(cat: Cat) -> Span {
    if !enabled() {
        return Span { cat, start: None };
    }
    let saved = CHILD.with(|c| c.replace(0));
    Span {
        cat,
        start: Some((Instant::now(), saved)),
    }
}

impl Drop for Span {
    #[inline]
    fn drop(&mut self) {
        if let Some((t, saved)) = self.start {
            let whole = t.elapsed().as_nanos() as u64;
            let child = CHILD.with(Cell::get);
            let own = whole.saturating_sub(child);
            CHILD.with(|c| c.set(saved.saturating_add(whole)));
            NS[self.cat as usize].fetch_add(own, Ordering::Relaxed);
            COUNT[self.cat as usize].fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// One frame: the time since the last `Present` began and what the counters held.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Frame {
    pub wall_ns: u64,
    pub ns: [u64; N],
    pub count: [u64; N],
}

impl Frame {
    /// The device thread's time no span covers: the game's code and waits for its commands.
    pub fn other_ns(&self) -> u64 {
        self.wall_ns
            .saturating_sub(self.ns[..OFF_THREAD].iter().sum::<u64>())
    }

    /// The category that held most of the frame (`other` included), and its nanoseconds.
    pub fn dominant(&self) -> (&'static str, u64) {
        let mut best = ("other", self.other_ns());
        for i in 0..OFF_THREAD {
            if self.ns[i] > best.1 {
                best = (NAMES[i], self.ns[i]);
            }
        }
        best
    }
}

/// The recorder's state on the device's thread.
pub(crate) struct Recorder {
    last: Option<Instant>,
    frames: Vec<Frame>,
    file: Option<Mutex<std::io::BufWriter<std::fs::File>>>,
}

impl Recorder {
    pub(crate) fn new() -> Self {
        let file = if enabled() {
            std::env::var("NV_PERF_FILE").ok().and_then(|p| {
                let mut f = std::io::BufWriter::new(std::fs::File::create(p).ok()?);
                use std::io::Write;
                let _ = writeln!(f, "wall_ns\t{}", NAMES.join("\t"));
                Some(Mutex::new(f))
            })
        } else {
            None
        };
        Recorder {
            last: None,
            frames: Vec::new(),
            file,
        }
    }

    /// A frame ends (a `Present` begins): the counters become its record.
    pub(crate) fn frame_end(&mut self) {
        if !enabled() {
            return;
        }
        let now = Instant::now();
        let mut f = Frame::default();
        for i in 0..N {
            f.ns[i] = NS[i].swap(0, Ordering::Relaxed);
            f.count[i] = COUNT[i].swap(0, Ordering::Relaxed);
        }
        if let Some(t) = self.last.replace(now) {
            f.wall_ns = (now - t).as_nanos() as u64;
            if let Some(file) = &self.file {
                use std::io::Write;
                if let Ok(mut w) = file.lock() {
                    let _ = write!(w, "{}", f.wall_ns);
                    for n in f.ns {
                        let _ = write!(w, "\t{n}");
                    }
                    let _ = writeln!(w);
                }
            }
            if self.frames.len() < 100_000 {
                self.frames.push(f);
            }
        }
    }

    /// The report lines since the last one (the frames are taken).
    pub(crate) fn report(&mut self) -> Vec<String> {
        let frames = std::mem::take(&mut self.frames);
        report_lines(&frames)
    }
}

fn ms(ns: u64) -> f64 {
    ns as f64 / 1e6
}

/// The report of `frames` (pure, tested): the average split, and the spikes with their causes.
pub(crate) fn report_lines(frames: &[Frame]) -> Vec<String> {
    if frames.is_empty() {
        return Vec::new();
    }
    let mut walls: Vec<u64> = frames.iter().map(|f| f.wall_ns).collect();
    walls.sort_unstable();
    let median = walls[walls.len() / 2];
    let p99 = walls[(walls.len() * 99 / 100).min(walls.len() - 1)];
    let n = frames.len() as f64;
    let mut sum = [0u64; N];
    let mut other = 0u64;
    for f in frames {
        for i in 0..N {
            sum[i] += f.ns[i];
        }
        other += f.other_ns();
    }
    let mut out = Vec::new();
    let mut split = format!(
        "NV_PERF: {} frames, wall median {:.2} ms p99 {:.2} ms max {:.2} ms; mean ms per frame: other {:.2}",
        frames.len(),
        ms(median),
        ms(p99),
        ms(*walls.last().unwrap_or(&0)),
        ms(other) / n
    );
    for i in 0..N {
        let tag = if i >= OFF_THREAD { " (off-thread)" } else { "" };
        split.push_str(&format!(", {}{} {:.2}", NAMES[i], tag, ms(sum[i]) / n));
    }
    let draws: u64 = frames.iter().map(|f| f.count[Cat::Draw as usize]).sum();
    split.push_str(&format!(", draws/frame {:.0}", draws as f64 / n));
    out.push(split);
    // Spikes: frames over twice the median, the worst first, with what held them.
    let mut spikes: Vec<&Frame> = frames
        .iter()
        .filter(|f| f.wall_ns > median.saturating_mul(2) && f.wall_ns > 4_000_000)
        .collect();
    spikes.sort_by_key(|f| std::cmp::Reverse(f.wall_ns));
    let mut by_cause: std::collections::BTreeMap<&str, (u32, u64)> = Default::default();
    for f in &spikes {
        let e = by_cause.entry(f.dominant().0).or_default();
        e.0 += 1;
        e.1 += f.wall_ns;
    }
    out.push(format!(
        "NV_PERF: {} spike frames (over 2x median): by cause {}",
        spikes.len(),
        by_cause
            .iter()
            .map(|(k, (c, t))| format!("{k} x{c} ({:.0} ms)", ms(*t)))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    for f in spikes.iter().take(8) {
        let (cause, ns) = f.dominant();
        let mut parts = format!("other {:.1}", ms(f.other_ns()));
        for i in 0..OFF_THREAD {
            if f.ns[i] > 500_000 {
                parts.push_str(&format!(", {} {:.1}", NAMES[i], ms(f.ns[i])));
            }
        }
        out.push(format!(
            "NV_PERF: spike {:.1} ms, cause {cause} ({:.1} ms), {} draws: {parts}",
            ms(f.wall_ns),
            ms(ns),
            f.count[Cat::Draw as usize]
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(wall_ms: u64, cat: Cat, ms_in: u64) -> Frame {
        let mut f = Frame {
            wall_ns: wall_ms * 1_000_000,
            ..Frame::default()
        };
        f.ns[cat as usize] = ms_in * 1_000_000;
        f
    }

    #[test]
    fn spikes_are_attributed_to_their_dominant_category() {
        let mut frames: Vec<Frame> = (0..100).map(|_| frame(7, Cat::Draw, 3)).collect();
        frames.push(frame(120, Cat::Pipeline, 100));
        frames.push(frame(60, Cat::ReadBack, 10));
        let lines = report_lines(&frames);
        assert!(lines[0].contains("102 frames"), "{}", lines[0]);
        assert!(lines[1].contains("2 spike frames"), "{}", lines[1]);
        assert!(lines[1].contains("pipeline x1"), "{}", lines[1]);
        // The 60 ms frame has 50 ms no span covers: the game's code or a wait.
        assert!(lines[1].contains("other x1"), "{}", lines[1]);
        assert!(
            lines[2].contains("spike 120.0 ms, cause pipeline"),
            "{}",
            lines[2]
        );
    }

    #[test]
    fn a_steady_run_has_no_spikes() {
        let frames: Vec<Frame> = (0..50).map(|_| frame(7, Cat::Draw, 3)).collect();
        let lines = report_lines(&frames);
        assert!(lines[1].contains("0 spike frames"));
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn nested_spans_count_only_their_own_time() {
        // Off: spans do nothing, so test the arithmetic on a frame directly.
        let mut f = frame(10, Cat::Draw, 4);
        f.ns[Cat::Pipeline as usize] = 3_000_000;
        assert_eq!(f.other_ns(), 3_000_000);
        assert_eq!(f.dominant().0, "draw");
    }
}
