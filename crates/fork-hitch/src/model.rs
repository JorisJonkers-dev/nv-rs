//! The pure parts: a frame's record, the rolling median, the cause of a hitch, the run's
//! statistics. No clock, no thread, no global: everything here is tested on numbers.

use crate::counters::{Counter, N};

/// What the screen shows (the viewer's reading of the game's state words).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(u8)]
pub enum Screen {
    #[default]
    Unknown = 0,
    Play = 1,
    Menu = 2,
    Loading = 3,
}

impl Screen {
    pub fn name(self) -> &'static str {
        match self {
            Screen::Unknown => "unknown",
            Screen::Play => "play",
            Screen::Menu => "menu",
            Screen::Loading => "loading",
        }
    }
}

/// What the player is doing, as ids and numbers (no names, no game data).
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Context {
    /// The player's cell form id (0 unknown) and its world space's (0 for an interior).
    pub cell: u32,
    pub world: u32,
    pub pos: [f32; 3],
    pub screen: Screen,
}

/// Process numbers the operating system keeps (Linux `getrusage`; zeros elsewhere), cumulative.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Usage {
    pub minflt: u64,
    pub majflt: u64,
    /// The peak resident size, KiB.
    pub maxrss_kb: u64,
    /// User plus system CPU of the whole process, microseconds.
    pub cpu_us: u64,
}

/// One frame of the ring.
#[derive(Clone, Copy, Debug)]
pub struct Rec {
    pub idx: u64,
    /// Milliseconds from the recorder's start to the frame's end.
    pub end_ms: u64,
    pub wall_ns: u64,
    /// The counters' change over the frame.
    pub d: [u64; N],
    pub minflt: u64,
    pub majflt: u64,
    /// Growth of the peak resident size during the frame, KiB.
    pub rss_growth_kb: u64,
    pub cpu_us: u64,
    pub ctx: Context,
}

impl Default for Rec {
    fn default() -> Self {
        Rec {
            idx: 0,
            end_ms: 0,
            wall_ns: 0,
            d: [0; N],
            minflt: 0,
            majflt: 0,
            rss_growth_kb: 0,
            cpu_us: 0,
            ctx: Context::default(),
        }
    }
}

impl Rec {
    pub fn get(&self, c: Counter) -> u64 {
        self.d[c as usize]
    }

    pub fn wall_ms(&self) -> f64 {
        self.wall_ns as f64 / 1e6
    }
}

/// Why a frame was slow.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum Cause {
    FileIo,
    PipelineCreate,
    ShaderCompile,
    TextureUpload,
    LockContention,
    RenderWait,
    DeviceWait,
    GameLogic,
    Loading,
    GcAlloc,
    Unknown,
}

impl Cause {
    pub const ALL: [Cause; 11] = [
        Cause::FileIo,
        Cause::PipelineCreate,
        Cause::ShaderCompile,
        Cause::TextureUpload,
        Cause::LockContention,
        Cause::RenderWait,
        Cause::DeviceWait,
        Cause::GameLogic,
        Cause::Loading,
        Cause::GcAlloc,
        Cause::Unknown,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Cause::FileIo => "file_io",
            Cause::PipelineCreate => "pipeline_create",
            Cause::ShaderCompile => "shader_compile",
            Cause::TextureUpload => "texture_upload",
            Cause::LockContention => "lock_contention",
            Cause::RenderWait => "render_wait",
            Cause::DeviceWait => "device_wait",
            Cause::GameLogic => "game_logic",
            Cause::Loading => "loading",
            Cause::GcAlloc => "gc/alloc",
            Cause::Unknown => "unknown",
        }
    }
}

/// The time in each bucket of a frame, nanoseconds, and the cause they point at.
#[derive(Clone, Debug, PartialEq)]
pub struct Verdict {
    pub cause: Cause,
    /// Every bucket over zero, largest first.
    pub buckets: Vec<(Cause, u64)>,
    /// The part of the frame the main thread spent inside platform calls no bucket names.
    pub platform_other_ns: u64,
}

/// The cost put on one minor page fault when estimating what memory growth cost a frame.
const FAULT_NS: u64 = 600;
/// A frame with this many page faults (about 40 MiB of fresh pages) or this much growth of the
/// peak resident size is a memory event.
const FAULTS_BIG: u64 = 10_000;
const GROWTH_BIG_KB: u64 = 32 * 1024;

/// Classifies `r`. `loading` is whether the screen was a loading screen or a load began in the
/// preceding moments.
pub fn classify(r: &Rec, loading: bool) -> Verdict {
    let g = |c| r.get(c);
    let wall = r.wall_ns;
    let file = g(Counter::FileNs);
    let sound = g(Counter::SoundNs);
    let wait = g(Counter::WaitNs);
    let kernel = g(Counter::KernelWaitNs);
    let device = g(Counter::DeviceWaitNs);
    let join = g(Counter::RenderWaitNs) + g(Counter::SyncWaitNs);
    let render = join + wait;
    let gpu = g(Counter::GpuWaitNs);
    let platform = g(Counter::PlatformNs);
    let named_in_platform = file + sound + wait + kernel + device;
    let platform_other = platform.saturating_sub(named_in_platform);
    let memory_event = r.minflt >= FAULTS_BIG || r.rss_growth_kb >= GROWTH_BIG_KB;
    let memory = if memory_event { r.minflt * FAULT_NS } else { 0 };
    // What the backend spent making things the draw needed, on the device thread: the main
    // thread waits for it when it reaches the device (device_wait) and it is named apart.
    let pipeline = g(Counter::PipelineNs);
    let shader = g(Counter::ShaderNs);
    let texture = g(Counter::TextureNs) + g(Counter::BufferNs);
    // What no counter covers is the game's own code (and the host's work around it).
    let residual = wall.saturating_sub(platform.saturating_add(join).saturating_add(gpu));
    let mut buckets: Vec<(Cause, u64)> = vec![
        (Cause::FileIo, file),
        (Cause::PipelineCreate, pipeline),
        (Cause::ShaderCompile, shader),
        (Cause::TextureUpload, texture),
        (Cause::LockContention, kernel),
        (Cause::RenderWait, render),
        (Cause::DeviceWait, device + gpu),
        (Cause::GameLogic, residual),
        (Cause::GcAlloc, memory),
    ];
    if loading {
        // The loading screen is its own cause: the work is the game's, whichever buckets lead.
        buckets.push((Cause::Loading, wall));
    }
    buckets.retain(|b| b.1 > 0);
    buckets.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let cause = pick(&buckets, wall, loading, platform_other, memory_event);
    Verdict {
        cause,
        buckets,
        platform_other_ns: platform_other,
    }
}

fn pick(
    buckets: &[(Cause, u64)],
    wall: u64,
    loading: bool,
    platform_other: u64,
    memory_event: bool,
) -> Cause {
    if loading {
        return Cause::Loading;
    }
    if memory_event {
        if let Some(&(_, ns)) = buckets.iter().find(|b| b.0 == Cause::GcAlloc) {
            // Page faults are a cause when they are a real part of the frame.
            if ns * 5 >= wall {
                return Cause::GcAlloc;
            }
        }
    }
    // A named cause leads when it holds a third of the frame; game logic is what is left when
    // the main thread was running the game's code for most of it.
    for &(cause, ns) in buckets {
        if matches!(cause, Cause::Loading | Cause::GcAlloc | Cause::GameLogic) {
            continue;
        }
        if ns * 3 >= wall {
            return cause;
        }
    }
    if let Some(&(_, ns)) = buckets.iter().find(|b| b.0 == Cause::GameLogic) {
        if ns * 2 >= wall && platform_other * 2 < ns {
            return Cause::GameLogic;
        }
    }
    Cause::Unknown
}

/// The rolling median of the last [`MEDIAN_WINDOW`] frames' wall times, recomputed every few
/// frames so the frame path stays a store.
pub struct Median {
    window: [u64; MEDIAN_WINDOW],
    filled: usize,
    next: usize,
    since: usize,
    cached: u64,
}

pub const MEDIAN_WINDOW: usize = 128;
const MEDIAN_EVERY: usize = 8;

impl Default for Median {
    fn default() -> Self {
        Median {
            window: [0; MEDIAN_WINDOW],
            filled: 0,
            next: 0,
            since: 0,
            cached: 0,
        }
    }
}

impl Median {
    /// Takes a frame's wall time; the median before it counts it is what the frame is judged by.
    pub fn push(&mut self, wall_ns: u64) {
        self.window[self.next] = wall_ns;
        self.next = (self.next + 1) % MEDIAN_WINDOW;
        self.filled = (self.filled + 1).min(MEDIAN_WINDOW);
        self.since += 1;
        if self.since >= MEDIAN_EVERY || self.filled <= 16 {
            self.since = 0;
            let mut copy = [0u64; MEDIAN_WINDOW];
            copy[..self.filled].copy_from_slice(&self.window[..self.filled]);
            let part = &mut copy[..self.filled];
            part.sort_unstable();
            self.cached = part[part.len() / 2];
        }
    }

    pub fn get(&self) -> u64 {
        self.cached
    }

    pub fn is_warm(&self) -> bool {
        self.filled >= 16
    }
}

/// The level a frame must pass to be a hitch: the larger of `factor` times the median and the
/// threshold.
pub fn limit_ns(median_ns: u64, threshold_ms: f64, factor: u64) -> u64 {
    median_ns
        .saturating_mul(factor)
        .max((threshold_ms * 1e6) as u64)
}

/// One of the worst hitches of the run.
#[derive(Clone, Debug, PartialEq)]
pub struct Worst {
    /// The guest functions (hex addresses) or platform call the sampler found, hottest first.
    pub top: Vec<String>,
    pub frame: u64,
    pub end_ms: u64,
    pub wall_ms: f64,
    pub cause: Cause,
    pub trigger: String,
}

/// The run's hitch numbers: frames and hitches, by cause, the worst few, the share of frames
/// over 16.7, 33.3, 50 and 100 ms.
#[derive(Clone, Debug)]
pub struct Stats {
    /// Frames that are not a loading screen (the numbers below are about these).
    pub frames: u64,
    pub hitches: u64,
    pub by_cause: [(u64, u64); 11],
    pub over: [u64; 4],
    pub worst: Vec<Worst>,
    pub total_wall_ns: u64,
    /// Seconds of play (frames showing play), of loading screens, and the frames of the latter.
    pub play_ns: u64,
    pub loading_ns: u64,
    pub loading_frames: u64,
    pub max_ns: u64,
    /// Wall times of the measured frames in buckets of [`BUCKET_US`] microseconds, the last
    /// bucket holding everything slower.
    pub hist: Vec<u32>,
    pub slow_windows: u64,
    /// Hitches that belong to a load (a loading screen, or a world change with none): load time,
    /// not stutter, so left out of the rate, and their wall time kept as its own number.
    pub loading_hitches: u64,
    pub loading_hitch_ns: u64,
}

pub const BUCKET_US: u64 = 100;
pub const BUCKETS: usize = 10_001;

impl Default for Stats {
    fn default() -> Self {
        Stats {
            frames: 0,
            hitches: 0,
            by_cause: [(0, 0); 11],
            over: [0; 4],
            worst: Vec::new(),
            total_wall_ns: 0,
            play_ns: 0,
            loading_ns: 0,
            loading_frames: 0,
            max_ns: 0,
            hist: vec![0; BUCKETS],
            slow_windows: 0,
            loading_hitches: 0,
            loading_hitch_ns: 0,
        }
    }
}

/// The frame time levels the summary counts, milliseconds.
pub const LEVELS_MS: [f64; 4] = [16.7, 33.4, 50.0, 100.0];

impl Stats {
    pub fn frame(&mut self, wall_ns: u64, screen: Screen) {
        if screen == Screen::Loading {
            self.loading_frames += 1;
            self.loading_ns += wall_ns;
            return;
        }
        if screen == Screen::Play {
            self.play_ns += wall_ns;
        }
        self.max_ns = self.max_ns.max(wall_ns);
        let b = ((wall_ns / 1000 / BUCKET_US) as usize).min(BUCKETS - 1);
        self.hist[b] += 1;
        self.frames += 1;
        self.total_wall_ns += wall_ns;
        for (o, l) in self.over.iter_mut().zip(LEVELS_MS) {
            if wall_ns as f64 > l * 1e6 {
                *o += 1;
            }
        }
    }

    pub fn hitch(&mut self, w: Worst) {
        self.hitches += 1;
        if w.cause == Cause::Loading {
            self.loading_hitches += 1;
            self.loading_hitch_ns += (w.wall_ms * 1e6) as u64;
        }
        let i = Cause::ALL.iter().position(|c| *c == w.cause).unwrap_or(10);
        self.by_cause[i].0 += 1;
        self.by_cause[i].1 += (w.wall_ms * 1000.0) as u64;
        self.worst.push(w);
        self.worst
            .sort_by(|a, b| b.wall_ms.total_cmp(&a.wall_ms).then(a.frame.cmp(&b.frame)));
        self.worst.truncate(5);
    }

    /// A hitch already counted in play (frame `frame`, `wall_ms`, `cause`) turns out to belong to
    /// a load (a screen switch that came right after it): it moves to the loads, and to the
    /// `loading` cause.
    pub fn to_load(&mut self, frame: u64, wall_ms: f64, cause: Cause) {
        if cause == Cause::Loading {
            return;
        }
        self.loading_hitches += 1;
        self.loading_hitch_ns += (wall_ms * 1e6) as u64;
        let at = |c: Cause| Cause::ALL.iter().position(|x| *x == c).unwrap_or(10);
        let us = (wall_ms * 1000.0) as u64;
        let from = &mut self.by_cause[at(cause)];
        from.0 = from.0.saturating_sub(1);
        from.1 = from.1.saturating_sub(us);
        let to = &mut self.by_cause[at(Cause::Loading)];
        to.0 += 1;
        to.1 += us;
        if let Some(w) = self.worst.iter_mut().find(|w| w.frame == frame) {
            w.cause = Cause::Loading;
        }
    }

    /// The frame time (ms) at quantile `q` (0.99), from the buckets: its upper edge.
    pub fn percentile_ms(&self, q: f64) -> f64 {
        if self.frames == 0 {
            return 0.0;
        }
        let want = ((self.frames as f64 * q).ceil() as u64).max(1);
        let mut seen = 0u64;
        for (i, n) in self.hist.iter().enumerate() {
            seen += u64::from(*n);
            if seen >= want {
                return if i == BUCKETS - 1 {
                    self.max_ns as f64 / 1e6
                } else {
                    ((i as u64 + 1) * BUCKET_US) as f64 / 1000.0
                };
            }
        }
        self.max_ns as f64 / 1e6
    }

    /// Average frames a second over the measured frames.
    pub fn fps(&self) -> f64 {
        if self.total_wall_ns == 0 {
            0.0
        } else {
            self.frames as f64 * 1e9 / self.total_wall_ns as f64
        }
    }

    /// Hitches in steady play: those that do not belong to a load.
    pub fn play_hitches(&self) -> u64 {
        self.hitches.saturating_sub(self.loading_hitches)
    }

    /// Hitches a minute of the measured frames, the hitches of loads left out.
    pub fn per_minute(&self) -> f64 {
        let minutes = self.total_wall_ns as f64 / 6e10;
        if minutes <= 0.0 {
            0.0
        } else {
            self.play_hitches() as f64 / minutes
        }
    }

    /// The summary as lines of text for the terminal.
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        let pct = |n: u64| {
            if self.frames == 0 {
                0.0
            } else {
                n as f64 * 100.0 / self.frames as f64
            }
        };
        let mut head = format!(
            "hitches: {} in {} frames ({:.1} a minute); frames over",
            self.hitches,
            self.frames,
            self.per_minute()
        );
        for (l, n) in LEVELS_MS.iter().zip(self.over) {
            head.push_str(&format!(" {l} ms {:.2}%,", pct(n)));
        }
        out.push(head.trim_end_matches(',').to_string());
        if self.loading_hitches > 0 {
            out.push(format!(
                "loads: {} slow frames, {} ms (not in the rate)",
                self.loading_hitches,
                self.loading_hitch_ns / 1_000_000
            ));
        }
        let mut causes: Vec<String> = Cause::ALL
            .iter()
            .zip(self.by_cause)
            .filter(|(_, (n, _))| *n > 0)
            .map(|(c, (n, ms))| format!("{} x{n} ({} ms)", c.name(), ms / 1000))
            .collect();
        if !causes.is_empty() {
            causes.sort();
            out.push(format!("hitches by cause: {}", causes.join(", ")));
        }
        for w in &self.worst {
            out.push(format!(
                "worst: {:.1} ms at frame {} ({} s into the run), {}, trigger {}",
                w.wall_ms,
                w.frame,
                w.end_ms / 1000,
                w.cause.name(),
                w.trigger
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(wall_ms: u64, set: &[(Counter, u64)]) -> Rec {
        let mut r = Rec {
            wall_ns: wall_ms * 1_000_000,
            ..Rec::default()
        };
        for (c, ms) in set {
            r.d[*c as usize] = ms * 1_000_000;
        }
        r
    }

    #[test]
    fn a_file_stall_is_file_io() {
        let r = rec(80, &[(Counter::FileNs, 60), (Counter::PlatformNs, 62)]);
        let v = classify(&r, false);
        assert_eq!(v.cause, Cause::FileIo);
        assert_eq!(v.buckets[0].0, Cause::FileIo);
    }

    #[test]
    fn named_causes_each_lead_when_they_hold_a_fifth() {
        for (counter, cause) in [
            (Counter::PipelineNs, Cause::PipelineCreate),
            (Counter::ShaderNs, Cause::ShaderCompile),
            (Counter::TextureNs, Cause::TextureUpload),
            (Counter::BufferNs, Cause::TextureUpload),
            (Counter::KernelWaitNs, Cause::LockContention),
            (Counter::RenderWaitNs, Cause::RenderWait),
            (Counter::DeviceWaitNs, Cause::DeviceWait),
            (Counter::GpuWaitNs, Cause::DeviceWait),
        ] {
            let r = rec(100, &[(counter, 50)]);
            assert_eq!(classify(&r, false).cause, cause, "{counter:?}");
        }
    }

    #[test]
    fn plain_game_code_is_game_logic() {
        let r = rec(60, &[(Counter::PlatformNs, 3)]);
        assert_eq!(classify(&r, false).cause, Cause::GameLogic);
    }

    #[test]
    fn a_frame_in_unnamed_platform_calls_is_unknown() {
        let r = rec(60, &[(Counter::PlatformNs, 50)]);
        assert_eq!(classify(&r, false).cause, Cause::Unknown);
    }

    #[test]
    fn the_loading_screen_is_its_own_cause() {
        let r = rec(300, &[(Counter::FileNs, 200)]);
        let v = classify(&r, true);
        assert_eq!(v.cause, Cause::Loading);
        assert!(v.buckets.iter().any(|b| b.0 == Cause::FileIo));
    }

    #[test]
    fn page_faults_are_gc_alloc_when_they_are_a_real_share() {
        let mut r = rec(40, &[]);
        r.minflt = 30_000; // 18 ms estimated: nearly half
        assert_eq!(classify(&r, false).cause, Cause::GcAlloc);
        let mut small = rec(40, &[]);
        small.minflt = 100;
        assert_eq!(classify(&small, false).cause, Cause::GameLogic);
    }

    #[test]
    fn the_median_follows_the_window() {
        let mut m = Median::default();
        for _ in 0..64 {
            m.push(7_000_000);
        }
        assert!(m.is_warm());
        assert_eq!(m.get(), 7_000_000);
        for _ in 0..200 {
            m.push(14_000_000);
        }
        assert_eq!(m.get(), 14_000_000);
    }

    #[test]
    fn the_limit_is_three_medians_or_the_threshold() {
        assert_eq!(limit_ns(7_000_000, 25.0, 3), 25_000_000);
        assert_eq!(limit_ns(12_000_000, 25.0, 3), 36_000_000);
    }

    #[test]
    fn stats_count_levels_causes_and_the_worst_five() {
        let mut s = Stats::default();
        for ms in [7, 17, 40, 60, 120, 7] {
            s.frame(ms * 1_000_000, Screen::Play);
        }
        assert_eq!(s.over, [4, 3, 2, 1]);
        for (i, ms) in [30.0, 90.0, 45.0, 200.0, 33.0, 31.0, 28.0]
            .into_iter()
            .enumerate()
        {
            s.hitch(Worst {
                top: Vec::new(),
                frame: i as u64,
                end_ms: 1000 * i as u64,
                wall_ms: ms,
                cause: if i % 2 == 0 {
                    Cause::FileIo
                } else {
                    Cause::Loading
                },
                trigger: "none".into(),
            });
        }
        assert_eq!(s.hitches, 7);
        assert_eq!(s.worst.len(), 5);
        assert_eq!(s.worst[0].wall_ms, 200.0);
        let text = s.lines().join("\n");
        assert!(text.contains("hitches: 7 in 6 frames"), "{text}");
        assert!(text.contains("file_io x4"), "{text}");
        assert!(text.contains("worst: 200.0 ms at frame 3"), "{text}");
    }

    #[test]
    fn loading_hitches_are_counted_but_not_in_the_rate() {
        let mut s = Stats::default();
        for _ in 0..600 {
            s.frame(100_000_000, Screen::Play); // one minute
        }
        for cause in [Cause::Loading, Cause::Loading, Cause::FileIo] {
            s.hitch(Worst {
                top: Vec::new(),
                frame: 1,
                end_ms: 1,
                wall_ms: 90.0,
                cause,
                trigger: "none".into(),
            });
        }
        assert_eq!(s.hitches, 3);
        assert_eq!((s.play_hitches(), s.loading_hitch_ns), (1, 180_000_000));
        assert!((s.per_minute() - 1.0).abs() < 0.01, "{}", s.per_minute());
        assert!(s
            .lines()
            .iter()
            .any(|l| l == "loads: 2 slow frames, 180 ms (not in the rate)"));
    }
}
