//! The hitch flight recorder (docs/fork/HITCHES.md): cheap enough to leave on in play, it keeps
//! the last seconds of frames in a ring, notices a frame that is far over the usual one, says
//! what it was spent on and what led up to it, and writes that as one structured line of the
//! JSON log.
//!
//! Std only apart from `fork-log` (where the line goes). The hot path is relaxed atomic adds
//! ([`add`], [`span`]) and, once a frame, [`frame_end`]; nothing allocates there. A hitch (a cold
//! path) builds its record then.
//!
//! `NV_HITCH`: `off`, `on` (the default: low detail) or `detail` (a sampler on every long
//! frame, full frames before the hitch). `NV_HITCH_MS` the threshold (25), `NV_HITCH_FACTOR`
//! the multiple of the rolling median (3), `NV_HITCH_SUMMARY_SECS` the summary interval (60,
//! 0 none), `NV_HITCH_CSV` a path for the hitch markers (default: beside the JSON log).

mod counters;
mod model;
mod render;
mod run;
mod sampler;
mod signals;
mod sys;

pub use counters::{Counter, Unit};
pub use model::{Cause, Context, Rec, Screen, Stats, Usage, Verdict};
pub use run::Thresholds;
pub use sampler::{file_class, set_stack_source, StackSource};

use model::{classify, limit_ns, Median, Worst};
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

/// Frames the ring keeps (about ten seconds at 144 frames a second).
pub const RING: usize = 2048;
/// How far back a hitch's trigger context reaches.
const CONTEXT_MS: u64 = 500;
/// Frames before a hitch that its record carries.
const BEFORE: usize = 5;
/// A platform call this long is an event of its own.
const SLOW_CALL_NS: u64 = 2_000_000;
/// More hitch records than this in [`BURST_MS`] are written in brief (a loading screen is one
/// long hitch, not a hundred).
const BURST_RECORDS: u32 = 12;
const BURST_MS: u64 = 10_000;
/// The rolling window of the slow-window check.
const WINDOW_MS: u64 = 10_000;

/// How much the recorder does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Mode {
    Off = 0,
    On = 1,
    Detail = 2,
}

static MODE: AtomicU8 = AtomicU8::new(Mode::On as u8);

/// The mode now.
#[inline]
pub fn mode() -> Mode {
    match MODE.load(Ordering::Relaxed) {
        0 => Mode::Off,
        2 => Mode::Detail,
        _ => Mode::On,
    }
}

/// Whether the recorder is on (the hot paths' one load).
#[inline]
pub fn enabled() -> bool {
    MODE.load(Ordering::Relaxed) != 0
}

/// The settings the environment gives.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub mode: Mode,
    pub threshold_ms: f64,
    pub factor: u64,
    pub summary_secs: u64,
    pub csv: Option<String>,
    pub thresholds: Thresholds,
}

impl Config {
    /// Parses the variables, given by name (so a test needs no environment).
    pub fn parse(var: &dyn Fn(&str) -> Option<String>) -> Config {
        let mode = match var("NV_HITCH")
            .map(|v| v.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("off" | "0" | "false" | "none") => Mode::Off,
            Some("detail" | "full" | "2") => Mode::Detail,
            _ => Mode::On,
        };
        let num = |n: &str| var(n).and_then(|v| v.trim().parse::<f64>().ok());
        Config {
            mode,
            threshold_ms: num("NV_HITCH_MS").filter(|v| *v > 0.0).unwrap_or(25.0),
            factor: num("NV_HITCH_FACTOR")
                .filter(|v| *v >= 1.0)
                .map_or(3, |v| v as u64),
            summary_secs: num("NV_HITCH_SUMMARY_SECS").map_or(60, |v| v.max(0.0) as u64),
            csv: var("NV_HITCH_CSV").filter(|v| !v.trim().is_empty()),
            thresholds: Thresholds::parse(var),
        }
    }
}

/// Which kind of thread this is, for the counters that only the main thread's time belongs to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Role {
    Other = 0,
    Main = 1,
    Game = 2,
}

thread_local! {
    static ROLE: Cell<Role> = const { Cell::new(Role::Other) };
}

/// Marks the calling thread (the engine does it where it starts the game's threads).
pub fn set_role(role: Role) {
    ROLE.with(|r| r.set(role));
}

/// The calling thread's role.
#[inline]
pub fn role() -> Role {
    ROLE.with(Cell::get)
}

fn start() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

/// Nanoseconds since the recorder's first use.
#[inline]
pub fn now_ns() -> u64 {
    start().elapsed().as_nanos() as u64
}

// ----- the hot path: counters -----

/// Adds `v` (nanoseconds, a count or bytes) to a counter.
#[inline]
pub fn add(c: Counter, v: u64) {
    if enabled() {
        counters::add(c, v);
    }
}

/// A timed span: its elapsed time goes to `ns` (and one to `n`, when given) when it drops.
#[must_use]
pub struct Span {
    start: Option<Instant>,
    ns: Counter,
    n: Option<Counter>,
}

/// Times a span into `ns`, counting it in `n`.
#[inline]
pub fn span(ns: Counter, n: Option<Counter>) -> Span {
    Span {
        start: enabled().then(Instant::now),
        ns,
        n,
    }
}

impl Drop for Span {
    #[inline]
    fn drop(&mut self) {
        if let Some(t) = self.start {
            counters::add(self.ns, t.elapsed().as_nanos() as u64);
            if let Some(n) = self.n {
                counters::add(n, 1);
            }
        }
    }
}

/// Times a span of the main thread's own time into `main`, or into `other` on any other thread.
#[inline]
pub fn span_split(main: Counter, other: Counter) -> Span {
    span(if role() == Role::Main { main } else { other }, None)
}

// ----- platform calls (the main thread) -----

/// The main thread inside platform function `slot` (its import slot address), made from the
/// guest call `stack`, the import itself last. The sampler sees it while it runs; its time is
/// the main thread's platform time; a long one is an event.
#[must_use]
pub struct CallGuard {
    start: Option<(Instant, u32)>,
}

#[inline]
pub fn enter_call(slot: u32, stack: &[u32]) -> CallGuard {
    if !enabled() || role() != Role::Main {
        return CallGuard { start: None };
    }
    let now = Instant::now();
    sampler::publish_enter(slot, stack, now_ns());
    CallGuard {
        start: Some((now, slot)),
    }
}

impl Drop for CallGuard {
    #[inline]
    fn drop(&mut self) {
        if let Some((t, slot)) = self.start {
            let ns = t.elapsed().as_nanos() as u64;
            sampler::publish_leave();
            counters::add(Counter::PlatformNs, ns);
            counters::add(Counter::PlatformCalls, 1);
            if ns >= SLOW_CALL_NS {
                counters::add(Counter::SlowCalls, 1);
                event(&format!("slow:{}", import_name(slot)), ns / 1000);
            }
        }
    }
}

/// The running platform call's argument class: a file name (its extension and a hash only).
#[inline]
pub fn note_file(path: &[u8]) {
    if enabled() && role() == Role::Main {
        let (ext, hash) = file_class(path);
        sampler::publish_arg(ext, hash);
    }
}

fn names() -> MutexGuard<'static, HashMap<u32, &'static str>> {
    static NAMES: OnceLock<Mutex<HashMap<u32, &'static str>>> = OnceLock::new();
    NAMES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

/// Names the import slot `slot` (`ReadFile`): at registration, once.
pub fn name_import(slot: u32, name: &'static str) {
    names().insert(slot, name);
}

/// The name of import slot `slot`, or its address.
pub fn import_name(slot: u32) -> String {
    names()
        .get(&slot)
        .map_or_else(|| format!("{slot:08x}"), |n| (*n).to_string())
}

// ----- events -----

const EVENTS: usize = 256;
const KIND_LEN: usize = 31;

#[derive(Clone, Copy)]
struct Ev {
    ns: u64,
    len: u8,
    kind: [u8; KIND_LEN],
    arg: u64,
}

struct EventRing {
    ev: [Ev; EVENTS],
    next: usize,
    filled: usize,
}

fn events() -> MutexGuard<'static, EventRing> {
    static RING: OnceLock<Mutex<EventRing>> = OnceLock::new();
    RING.get_or_init(|| {
        Mutex::new(EventRing {
            ev: [Ev {
                ns: 0,
                len: 0,
                kind: [0; KIND_LEN],
                arg: 0,
            }; EVENTS],
            next: 0,
            filled: 0,
        })
    })
    .lock()
    .unwrap_or_else(|p| p.into_inner())
}

/// Records that something happened that may explain a hitch: a cell load, a loading screen, a
/// save, a slow platform call. `kind` is a short dotted name, `arg` a number (an id, a size, a
/// time): never a name from the game's data.
pub fn event(kind: &str, arg: u64) {
    if !enabled() {
        return;
    }
    let ns = now_ns();
    let mut r = events();
    let i = r.next;
    let mut buf = [0u8; KIND_LEN];
    let mut len = kind.len().min(KIND_LEN);
    while !kind.is_char_boundary(len) {
        len -= 1;
    }
    buf[..len].copy_from_slice(&kind.as_bytes()[..len]);
    r.ev[i] = Ev {
        ns,
        len: len as u8,
        kind: buf,
        arg,
    };
    r.next = (i + 1) % EVENTS;
    r.filled = (r.filled + 1).min(EVENTS);
}

/// Whether a game event kind is one the recorder keeps (the throttled and per-frame kinds of
/// the game trace are not).
fn notable(kind: &str) -> bool {
    [
        "menu.",
        "load.",
        "loading.",
        "movie.open",
        "movie.close",
        "save.",
        "cell.",
        "world.",
        "screen.",
        "door.",
        "combat.",
        "quest.",
        "present.source",
        "gpu.render_target",
        "gpu.surface",
        "audio.stream.play",
    ]
    .iter()
    .any(|p| kind.starts_with(p))
}

fn observe(kind: &str) {
    if notable(kind) {
        event(kind, 0);
    }
}

/// Events of the last `ms` before `now_ns`, oldest first.
fn recent_events(now: u64, ms: u64) -> Vec<render::Seen> {
    let r = events();
    let from = now.saturating_sub(ms * 1_000_000);
    let mut out: Vec<(u64, render::Seen)> = (0..r.filled)
        .map(|k| r.ev[(r.next + EVENTS - 1 - k) % EVENTS])
        .filter(|e| e.ns >= from && e.ns <= now)
        .map(|e| {
            (
                e.ns,
                render::Seen {
                    age_ms: (now - e.ns) / 1_000_000,
                    kind: String::from_utf8_lossy(&e.kind[..e.len as usize]).into_owned(),
                    arg: e.arg,
                },
            )
        })
        .collect();
    out.sort_by_key(|e| e.0);
    out.into_iter().map(|e| e.1).collect()
}

// ----- frames -----

struct Recorder {
    cfg: Config,
    ring: Vec<Rec>,
    head: usize,
    len: usize,
    median: Median,
    last_counters: [u64; counters::N],
    last_usage: Usage,
    last_end_ns: Option<u64>,
    idx: u64,
    stats: Stats,
    burst_from_ms: u64,
    burst_records: u32,
    next_summary: Instant,
    csv: Option<std::fs::File>,
    csv_tried: bool,
    next_window_ms: u64,
}

static RECORDER: Mutex<Option<Recorder>> = Mutex::new(None);
static FRAME_START_NS: AtomicU64 = AtomicU64::new(0);
static FRAME_EPOCH: AtomicU64 = AtomicU64::new(0);

fn recorder() -> MutexGuard<'static, Option<Recorder>> {
    RECORDER.lock().unwrap_or_else(|p| p.into_inner())
}

impl Recorder {
    fn new(cfg: Config) -> Recorder {
        let secs = cfg.summary_secs;
        Recorder {
            cfg,
            ring: vec![Rec::default(); RING],
            head: 0,
            len: 0,
            median: Median::default(),
            last_counters: counters::read(),
            last_usage: sys::usage(),
            last_end_ns: None,
            idx: 0,
            stats: Stats::default(),
            burst_from_ms: 0,
            burst_records: 0,
            next_summary: Instant::now() + Duration::from_secs(secs.max(1)),
            csv: None,
            csv_tried: false,
            next_window_ms: WINDOW_MS,
        }
    }

    /// The frames before the newest, newest last, at most `n`.
    fn before(&self, n: usize) -> Vec<Rec> {
        let n = n.min(self.len);
        (0..n)
            .map(|k| self.ring[(self.head + RING - n + k) % RING])
            .collect()
    }

    fn push(&mut self, r: Rec) {
        self.ring[self.head] = r;
        self.head = (self.head + 1) % RING;
        self.len = (self.len + 1).min(RING);
    }
}

/// Starts the recorder from the environment: reads the settings, registers the game trace's
/// events and the summary at exit, starts the sampler thread. Call once, early; harmless twice.
pub fn init() {
    init_with(Config::parse(&|n| std::env::var(n).ok()));
}

/// [`init`] with settings given.
pub fn init_with(cfg: Config) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    let mut first = false;
    ONCE.call_once(|| first = true);
    if !first {
        return;
    }
    MODE.store(cfg.mode as u8, Ordering::Relaxed);
    if cfg.mode == Mode::Off {
        return;
    }
    let _ = start();
    fork_log::game::set_observer(observe);
    fork_log::on_finish(finish_hook);
    let _ = run_state();
    signals::install();
    scan_viewers();
    let detail = cfg.mode == Mode::Detail;
    let after_ms = (cfg.threshold_ms * 2.0).max(1.0);
    *recorder() = Some(Recorder::new(cfg));
    fork_log::info!(
        target: "hitch",
        "hitch recorder {} (NV_HITCH=off stops it, NV_HITCH=detail adds the sampler on every long frame)",
        if detail { "on, detail" } else { "on" }
    );
    let _ = std::thread::Builder::new()
        .name("nv-hitch".into())
        .spawn(move || sampler_loop(detail, after_ms));
}

/// The sampler thread: idle while the frame in progress is young, sampling every 2 ms once it
/// is older than the limit.
fn sampler_loop(detail: bool, after_ms: f64) {
    loop {
        let began = FRAME_START_NS.load(Ordering::Relaxed);
        let mut wait = Duration::from_millis(4);
        let sig = signals::pending();
        if sig != 0 {
            write_summary(&format!("signal:{}", signals::name(sig)));
            fork_log::flush();
            signals::die(sig);
        }
        periodic();
        if began != 0 && enabled() {
            let epoch = FRAME_EPOCH.load(Ordering::Acquire);
            let now = now_ns();
            let age_ms = now.saturating_sub(began) as f64 / 1e6;
            // Detail mode samples from a frame older than three typical ones; the default only
            // when the frame is already a clear hitch (twice the threshold).
            let after = if detail {
                (median_ms() * 3.0).clamp(8.0, after_ms)
            } else {
                after_ms
            };
            if age_ms >= after {
                sampler::sample(epoch, now);
                wait = Duration::from_millis(2);
            }
        }
        std::thread::sleep(wait);
    }
}

static MEDIAN_US: AtomicU64 = AtomicU64::new(0);

fn median_ms() -> f64 {
    MEDIAN_US.load(Ordering::Relaxed) as f64 / 1000.0
}

/// A main frame ended (the viewer calls it once per iteration of its loop, with what the game
/// shows and where the player is). The frame's numbers are what the counters gained since the
/// last call.
pub fn frame_end(ctx: &Context) {
    if !enabled() {
        return;
    }
    let now = now_ns();
    let mut guard = recorder();
    let Some(rec) = guard.as_mut() else {
        return;
    };
    let finished_epoch = FRAME_EPOCH.fetch_add(1, Ordering::AcqRel);
    FRAME_START_NS.store(now, Ordering::Relaxed);
    let Some(last) = rec.last_end_ns.replace(now) else {
        rec.last_counters = counters::read();
        rec.last_usage = sys::usage();
        return;
    };
    let wall = now - last;
    let cur = counters::read();
    let usage = sys::usage();
    let mut r = Rec {
        idx: rec.idx,
        end_ms: now / 1_000_000,
        wall_ns: wall,
        ctx: *ctx,
        minflt: usage.minflt.saturating_sub(rec.last_usage.minflt),
        majflt: usage.majflt.saturating_sub(rec.last_usage.majflt),
        rss_growth_kb: usage.maxrss_kb.saturating_sub(rec.last_usage.maxrss_kb),
        cpu_us: usage.cpu_us.saturating_sub(rec.last_usage.cpu_us),
        ..Rec::default()
    };
    for (d, (c, p)) in r.d.iter_mut().zip(cur.iter().zip(&rec.last_counters)) {
        *d = c.saturating_sub(*p);
    }
    rec.last_counters = cur;
    rec.last_usage = usage;
    rec.idx += 1;

    let median = rec.median.get();
    let limit = limit_ns(median, rec.cfg.threshold_ms, rec.cfg.factor);
    let hitch = rec.median.is_warm() && wall > limit;
    rec.stats.frame(wall, ctx.screen);
    if hitch {
        report_hitch(rec, &r, median, finished_epoch);
    }
    rec.push(r);
    rec.median.push(wall);
    MEDIAN_US.store(rec.median.get() / 1000, Ordering::Relaxed);
    if r.end_ms >= rec.next_window_ms {
        rec.next_window_ms = r.end_ms + WINDOW_MS;
        slow_window(rec, r.end_ms);
    }
    if rec.idx % 64 == 0 && rec.cfg.summary_secs > 0 && Instant::now() >= rec.next_summary {
        rec.next_summary = Instant::now() + Duration::from_secs(rec.cfg.summary_secs);
        let lines = rec.stats.lines();
        drop(guard);
        for l in lines {
            fork_log::info!(target: "hitch", "{l}");
        }
    }
}

/// The sampler's findings as short names: the busiest platform call, then hot guest addresses.
fn hot_names(s: Option<&sampler::Samples>) -> Vec<String> {
    let Some(s) = s else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut calls: Vec<_> = s.calls.iter().collect();
    calls.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    if let Some(((slot, ..), _)) = calls.first() {
        out.push(import_name(*slot));
    }
    let mut hot: Vec<_> = s.hot.iter().collect();
    hot.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    out.extend(hot.into_iter().take(3).map(|(a, _)| format!("{a:08x}")));
    out
}

/// A frame over the limit: classify it, collect its context, write the record.
#[cold]
fn report_hitch(rec: &mut Recorder, r: &Rec, median: u64, epoch: u64) {
    let now = r.end_ms * 1_000_000;
    let events = recent_events(now_ns(), CONTEXT_MS);
    let loading = r.ctx.screen == Screen::Loading
        || events.iter().any(|e| {
            e.kind.starts_with("loading.")
                || e.kind.starts_with("load.")
                || e.kind == "menu.loading"
        });
    let verdict = classify(r, loading);
    // The trigger: the newest event of the context, with its age; a slow call names itself.
    let trigger = events.last().map_or_else(
        || "none".to_string(),
        |e| format!("{} {} ms before", e.kind, e.age_ms),
    );
    let samples = sampler::take(epoch);
    let detail = rec.cfg.mode == Mode::Detail;
    // Records in a burst are brief: the first dozen say everything, the rest are counted.
    let ms = r.end_ms;
    if ms.saturating_sub(rec.burst_from_ms) > BURST_MS {
        rec.burst_from_ms = ms;
        rec.burst_records = 0;
    }
    rec.burst_records += 1;
    let brief = rec.burst_records > BURST_RECORDS;
    let before = if brief {
        Vec::new()
    } else {
        rec.before(BEFORE)
    };
    let _ = now;
    let top = hot_names(samples.as_ref());
    rec.stats.hitch(Worst {
        top,
        frame: r.idx,
        end_ms: r.end_ms,
        wall_ms: r.wall_ms(),
        cause: verdict.cause,
        trigger: trigger.clone(),
    });
    let name = |slot: u32| import_name(slot);
    let h = render::Hitch {
        rec: r,
        before: &before,
        median_ms: median as f64 / 1e6,
        threshold_ms: rec.cfg.threshold_ms,
        verdict: &verdict,
        trigger: &trigger,
        events: &events,
        samples: samples.as_ref(),
        detail,
        brief,
        name_of: &name,
    };
    fork_log::json_record(
        fork_log::Level::Warn,
        "hitch",
        "hitch",
        &render::hitch_body(&h),
    );
    if !brief {
        let line = render::hitch_line(&h);
        if r.wall_ns as f64 >= rec.cfg.threshold_ms * 2e6 {
            fork_log::info!(target: "hitch", "{line}");
        } else {
            fork_log::debug!(target: "hitch", "{line}");
        }
    }
    write_marker(rec, r, verdict.cause);
}

/// One CSV line per hitch with the wall clock and the run's clock, for lining the hitches up
/// with an outside log (MangoHud's CSV names its start time in its file name).
fn write_marker(rec: &mut Recorder, r: &Rec, cause: Cause) {
    use std::io::Write;
    if rec.csv.is_none() && !rec.csv_tried {
        rec.csv_tried = true;
        let path = rec
            .cfg
            .csv
            .clone()
            .map(std::path::PathBuf::from)
            .or_else(|| fork_log::json_path().map(|p| p.with_extension("hitches.csv")));
        if let Some(p) = path {
            if let Ok(mut f) = std::fs::File::create(&p) {
                let _ = writeln!(f, "unix_ms,run_ms,frame,wall_ms,cause");
                rec.csv = Some(f);
            }
        }
    }
    if let Some(f) = rec.csv.as_mut() {
        let unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let _ = writeln!(
            f,
            "{unix_ms},{},{},{:.3},{}",
            r.end_ms,
            r.idx,
            r.wall_ms(),
            cause.name()
        );
    }
}

/// The run's hitch numbers so far.
pub fn stats() -> Stats {
    recorder()
        .as_ref()
        .map(|r| r.stats.clone())
        .unwrap_or_default()
}

/// The last `n` frames the ring holds, oldest first.
pub fn recent(n: usize) -> Vec<Rec> {
    recorder().as_ref().map(|r| r.before(n)).unwrap_or_default()
}

/// The run ends (`how`: `exit`, `stop`, `panic`): the run summary file; on a clean exit also the
/// terminal lines and the `hitch_summary` record.
fn finish_hook(how: &str) {
    if !enabled() {
        return;
    }
    // The end of the run is shown once, on the terminal too: a clean exit, or a panic that ends it.
    write_summary_with(how, how == "exit" || how == "panic");
    if how == "exit" {
        finish();
    }
}

/// The hitch summary at the end of the run: the terminal lines and one `hitch_summary` record.
pub fn finish() {
    let stats = {
        let g = recorder();
        match g.as_ref() {
            Some(r) if r.stats.frames > 0 => r.stats.clone(),
            _ => return,
        }
    };
    for l in stats.lines() {
        fork_log::info!(target: "hitch", "{l}");
    }
    let mut o = String::new();
    summary_json(&mut o, &stats);
    fork_log::json_record(fork_log::Level::Info, "hitch", "hitch_summary", &o);
}

// ----- the run summary -----

struct RunState {
    started_unix: u64,
    started: Instant,
    ended: Mutex<Option<String>>,
    meta: Mutex<std::collections::BTreeMap<String, String>>,
    peaks: Mutex<std::collections::BTreeMap<String, u64>>,
    other_viewers: AtomicU64,
    last_periodic: Mutex<Instant>,
}

fn run_state() -> &'static RunState {
    static RUN: OnceLock<RunState> = OnceLock::new();
    RUN.get_or_init(|| RunState {
        started_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
        started: Instant::now(),
        ended: Mutex::new(None),
        meta: Mutex::default(),
        peaks: Mutex::default(),
        other_viewers: AtomicU64::new(0),
        last_periodic: Mutex::new(Instant::now()),
    })
}

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Describes the run in the summary: `build`, `profile`, `gpu`, `driver`, `session`,
/// `resolution`, `threads`. Short strings, no game data and no paths of the player's.
pub fn set_meta(key: &str, value: &str) {
    // The start header says the same things (fork-log's facts).
    fork_log::fact(key, value);
    locked(&run_state().meta).insert(key.to_string(), value.to_string());
}

/// Keeps the highest value seen of a number the summary reports (`rss_mb`, `vram_mb`,
/// `gtt_mb`, `heap_mb`).
pub fn peak(key: &str, value: u64) {
    let mut p = locked(&run_state().peaks);
    let e = p.entry(key.to_string()).or_insert(0);
    *e = (*e).max(value);
}

/// Counts other `nv-viewer` processes (Linux `/proc`): a run that shared the machine is noisy.
fn scan_viewers() {
    #[cfg(target_os = "linux")]
    {
        let me = std::process::id().to_string();
        let mut n = 0u64;
        if let Ok(dir) = std::fs::read_dir("/proc") {
            for e in dir.flatten() {
                let name = e.file_name();
                let Some(pid) = name
                    .to_str()
                    .filter(|p| p.bytes().all(|b| b.is_ascii_digit()))
                else {
                    continue;
                };
                if pid == me {
                    continue;
                }
                if std::fs::read_to_string(e.path().join("comm"))
                    .is_ok_and(|c| c.trim() == "nv-viewer")
                {
                    n += 1;
                }
            }
        }
        run_state().other_viewers.fetch_max(n, Ordering::Relaxed);
    }
}

/// Every 30 seconds: the other-viewers scan and a summary marked `running`.
fn periodic() {
    let st = run_state();
    {
        let mut last = locked(&st.last_periodic);
        if last.elapsed() < Duration::from_secs(30) {
            return;
        }
        *last = Instant::now();
    }
    scan_viewers();
    write_summary("running");
}

/// Writes `run-summary-<unix>-<pid>.json` next to the JSON log. The first real end of the run
/// (anything but `running`) is the reason kept when it is written again.
pub fn write_summary(how: &str) {
    write_summary_with(how, false);
}

/// [`write_summary`], and with `show` the final summary lines on the terminal (fork-log's
/// headline, once).
fn write_summary_with(how: &str, show: bool) {
    let st = run_state();
    let ended = {
        let mut e = locked(&st.ended);
        if how != "running" && e.is_none() {
            *e = Some(how.to_string());
        }
        e.clone().unwrap_or_else(|| "running".to_string())
    };
    let (stats, cfg) = {
        let g = recorder();
        match g.as_ref() {
            Some(r) => (r.stats.clone(), r.cfg.clone()),
            None => return,
        }
    };
    if stats.frames == 0 && stats.loading_frames == 0 && ended == "running" {
        return;
    }
    let Some(dir) = fork_log::log_dir() else {
        return;
    };
    let id = format!("{}-{}", st.started_unix, std::process::id());
    let log_name = fork_log::json_path()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (errors, warnings) = fork_log::counts();
    let meta = locked(&st.meta).clone();
    let peaks = locked(&st.peaks).clone();
    let input = run::Input {
        id: &id,
        started_unix: st.started_unix,
        ended_unix: now_unix,
        ended: &ended,
        duration_s: st.started.elapsed().as_secs_f64(),
        meta: &meta,
        peaks: &peaks,
        stats: &stats,
        soft_stops: fork_log::soft_stops(),
        errors,
        warnings,
        other_viewers: st.other_viewers.load(Ordering::Relaxed),
        log_name: &log_name,
        thresholds: &cfg.thresholds,
    };
    let text = run::build(&input);
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("run-summary-{id}.json"));
    if show {
        fork_log::final_summary(&run::final_lines(
            &input,
            &fork_log::json_path().map_or_else(|| "-".to_string(), |p| p.display().to_string()),
            &path.display().to_string(),
        ));
    }
    let tmp = dir.join(format!("run-summary-{id}.json.tmp"));
    if std::fs::write(&tmp, text + "\n").is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// Every ten seconds: when the window's 99th percentile frame time is over the limit, one
/// `perf.slow_window` record with the window's cause mix (sustained slowness, not a spike).
#[cold]
fn slow_window(rec: &mut Recorder, end_ms: u64) {
    let from = end_ms.saturating_sub(WINDOW_MS);
    let frames: Vec<Rec> = (0..rec.len)
        .map(|k| rec.ring[(rec.head + RING - 1 - k) % RING])
        .filter(|r| r.end_ms >= from)
        .collect();
    let measured: Vec<&Rec> = frames
        .iter()
        .filter(|r| r.ctx.screen != Screen::Loading)
        .collect();
    if measured.len() < 50 || measured.len() * 2 < frames.len() {
        return;
    }
    let mut walls: Vec<u64> = measured.iter().map(|r| r.wall_ns).collect();
    walls.sort_unstable();
    let p99 = walls[(walls.len() * 99 / 100).min(walls.len() - 1)];
    let limit_ms = rec.cfg.thresholds.p99_ms;
    if (p99 as f64) / 1e6 <= limit_ms {
        return;
    }
    let median = walls[walls.len() / 2];
    let mut mix: std::collections::BTreeMap<&'static str, u64> = Default::default();
    for r in measured
        .iter()
        .filter(|r| r.wall_ns > median.saturating_mul(2))
    {
        *mix.entry(classify(r, false).cause.name()).or_default() += 1;
    }
    rec.stats.slow_windows += 1;
    use std::fmt::Write;
    let mut o = String::new();
    let _ = write!(
        o,
        "\"slow_window\":{{\"from_ms\":{from},\"to_ms\":{end_ms},\"frames\":{},\"p50_ms\":{:.2},\"p99_ms\":{:.2},\"max_ms\":{:.2},\"fps\":{:.1},\"limit_ms\":{limit_ms},\"cause_mix\":{{",
        measured.len(),
        median as f64 / 1e6,
        p99 as f64 / 1e6,
        *walls.last().unwrap_or(&0) as f64 / 1e6,
        walls.len() as f64 * 1e9 / walls.iter().sum::<u64>().max(1) as f64
    );
    for (i, (k, n)) in mix.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        let _ = write!(o, "\"{k}\":{n}");
    }
    o.push_str("}}");
    fork_log::json_record(fork_log::Level::Warn, "hitch", "perf.slow_window", &o);
    fork_log::info!(
        target: "hitch",
        "slow window: p99 {:.1} ms over {} frames ({:.1} s), cause mix {}",
        p99 as f64 / 1e6,
        measured.len(),
        WINDOW_MS as f64 / 1000.0,
        mix.iter().map(|(k, n)| format!("{k} x{n}")).collect::<Vec<_>>().join(", ")
    );
}

fn summary_json(o: &mut String, s: &Stats) {
    use std::fmt::Write;
    let _ = write!(
        o,
        "\"summary\":{{\"frames\":{},\"hitches\":{},\"per_minute\":{:.2},\"over_ms\":{{",
        s.frames,
        s.hitches,
        s.per_minute()
    );
    for (i, (l, n)) in model::LEVELS_MS.iter().zip(s.over).enumerate() {
        if i > 0 {
            o.push(',');
        }
        let _ = write!(o, "\"{l}\":{n}");
    }
    o.push_str("},\"by_cause\":{");
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
    o.push_str("},\"worst\":[");
    for (i, w) in s.worst.iter().enumerate() {
        if i > 0 {
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
        fork_log::json_string(o, &w.trigger);
        o.push('}');
    }
    o.push_str("]}");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(vars: &[(&str, &str)]) -> Config {
        Config::parse(&|n| {
            vars.iter()
                .find(|(k, _)| *k == n)
                .map(|(_, v)| (*v).to_string())
        })
    }

    #[test]
    fn the_defaults_are_on_at_25_ms_and_three_medians() {
        let c = cfg(&[]);
        assert_eq!(c.mode, Mode::On);
        assert_eq!((c.threshold_ms, c.factor, c.summary_secs), (25.0, 3, 60));
        assert_eq!(c.csv, None);
    }

    #[test]
    fn the_variables_choose_the_mode_and_the_numbers() {
        assert_eq!(cfg(&[("NV_HITCH", "off")]).mode, Mode::Off);
        assert_eq!(cfg(&[("NV_HITCH", "0")]).mode, Mode::Off);
        assert_eq!(cfg(&[("NV_HITCH", "DETAIL")]).mode, Mode::Detail);
        assert_eq!(cfg(&[("NV_HITCH", "1")]).mode, Mode::On);
        let c = cfg(&[
            ("NV_HITCH_MS", "40"),
            ("NV_HITCH_FACTOR", "2"),
            ("NV_HITCH_SUMMARY_SECS", "0"),
            ("NV_HITCH_CSV", "/tmp/h.csv"),
        ]);
        assert_eq!((c.threshold_ms, c.factor, c.summary_secs), (40.0, 2, 0));
        assert_eq!(c.csv.as_deref(), Some("/tmp/h.csv"));
        assert_eq!(cfg(&[("NV_HITCH_MS", "nonsense")]).threshold_ms, 25.0);
    }

    #[test]
    fn the_events_of_the_context_come_oldest_first_with_their_age() {
        event("test.first", 1);
        event("test.second", 2);
        let now = now_ns();
        let seen = recent_events(now, 500);
        let at = seen.iter().position(|e| e.kind == "test.first").unwrap();
        assert!(seen[at + 1..].iter().any(|e| e.kind == "test.second"));
        assert!(seen[at].age_ms <= 500);
        assert!(recent_events(now + 10_000_000_000, 500)
            .iter()
            .all(|e| e.kind != "test.first"));
    }

    #[test]
    fn only_the_game_trace_kinds_that_explain_a_hitch_are_kept() {
        assert!(notable("loading.begin"));
        assert!(notable("menu.loading"));
        assert!(notable("load.thread"));
        assert!(!notable("movie.frame"));
        assert!(!notable("input.key"));
        assert!(!notable("audio.play"));
    }

    #[test]
    fn a_kind_is_cut_at_a_character_boundary() {
        event(&"é".repeat(40), 0);
        let seen = recent_events(now_ns(), 500);
        assert!(seen.iter().any(|e| e.kind.starts_with('é')));
    }

    /// The whole path on a synthetic run: steady frames, one slow one with a file stall in it.
    #[test]
    fn a_slow_frame_makes_one_classified_record_in_the_ring_and_stats() {
        let mut rec = Recorder::new(cfg(&[("NV_HITCH_SUMMARY_SECS", "0")]));
        // Drive report_hitch and the stats directly: the global recorder belongs to the process.
        let mut r = Rec {
            idx: 200,
            end_ms: 5000,
            wall_ns: 90_000_000,
            ..Rec::default()
        };
        r.d[Counter::FileNs as usize] = 70_000_000;
        r.d[Counter::PlatformNs as usize] = 71_000_000;
        for i in 0..150 {
            rec.push(Rec {
                idx: i,
                wall_ns: 7_000_000,
                ..Rec::default()
            });
        }
        rec.stats.frame(r.wall_ns, Screen::Play);
        event("cell.change", 9);
        report_hitch(&mut rec, &r, 7_000_000, 0);
        assert_eq!(rec.stats.hitches, 1);
        assert_eq!(rec.stats.worst[0].cause, Cause::FileIo);
        assert!(rec.stats.worst[0].trigger.starts_with("cell.change"));
        assert_eq!(rec.before(5).len(), 5);
        assert_eq!(rec.before(5)[4].idx, 149);
        // The 13th record in a burst is brief.
        for _ in 0..13 {
            report_hitch(&mut rec, &r, 7_000_000, 0);
        }
        assert!(rec.burst_records > BURST_RECORDS);
    }

    #[test]
    fn the_ring_wraps_and_keeps_the_newest() {
        let mut rec = Recorder::new(cfg(&[]));
        for i in 0..(RING as u64 + 10) {
            rec.push(Rec {
                idx: i,
                ..Rec::default()
            });
        }
        assert_eq!(rec.len, RING);
        let last = rec.before(3);
        assert_eq!(last[2].idx, RING as u64 + 9);
        assert_eq!(last[0].idx, RING as u64 + 7);
    }

    #[test]
    fn spans_and_adds_land_in_the_counters() {
        let before = counters::read()[Counter::Draws as usize];
        add(Counter::Draws, 5);
        {
            let _s = span(Counter::PipelineNs, Some(Counter::PipelineN));
            std::thread::sleep(Duration::from_millis(2));
        }
        let after = counters::read();
        assert!(after[Counter::Draws as usize] >= before + 5);
        assert!(after[Counter::PipelineNs as usize] >= 2_000_000);
        assert!(after[Counter::PipelineN as usize] >= 1);
    }
}
