//! The memory timeline behind `NV_MEMLOG` (docs/fork/MEMLOG.md).
//!
//! Off by default. `NV_MEMLOG=<seconds>` (or `1`, one second) makes the game's main loop print one
//! line of `key=value` numbers per interval: the guest heap, the host process and the GPU layer's
//! counters. No thread runs: [`MemLog::tick`] is one clock read per frame while off-interval, and
//! the caller passes the numbers in, so this crate reads nothing from the engine. A line holds
//! numbers only: no path, no mod name, no game data.

use std::time::{Duration, Instant};

/// The line's first word, for grepping and for `smoke_report.py`.
pub const TAG: &str = "NV_MEMLOG";

/// The guest heap's numbers (bytes; addresses for `top` and `peak`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Heap {
    pub capacity: u64,
    pub used: u64,
    pub free: u64,
    pub largest_free: u64,
    pub top: u64,
    pub peak_addr: u64,
}

/// Host process memory in bytes; `None` where the platform gives none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Host {
    pub rss: u64,
    pub rss_peak: u64,
}

/// The GPU layer's counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Gpu {
    pub kept_bytes: u64,
    pub kept_levels: u64,
    pub bind_groups: u64,
    pub pipelines: u64,
    pub presents: u64,
}

/// The sampler: the interval, the clock and the running peak of heap use.
#[derive(Debug)]
pub struct MemLog {
    every: Duration,
    start: Instant,
    last: Option<Instant>,
    peak_used: u64,
    lines: u64,
}

/// The interval `NV_MEMLOG` asks for: unset, empty, `0`, `off` or junk is off; `1` and up is
/// seconds (fractions such as `0.5` are allowed).
pub fn parse_interval(v: Option<&str>) -> Option<Duration> {
    let s = v?.trim();
    let secs: f64 = s.parse().ok()?;
    (0.05..=86_400.0)
        .contains(&secs)
        .then(|| Duration::from_secs_f64(secs))
}

impl MemLog {
    pub fn new(every: Duration, now: Instant) -> Self {
        Self {
            every,
            start: now,
            last: None,
            peak_used: 0,
            lines: 0,
        }
    }

    /// `Some` only when `NV_MEMLOG` asks for it.
    pub fn from_env() -> Option<Self> {
        let every = parse_interval(std::env::var("NV_MEMLOG").ok().as_deref())?;
        Some(Self::new(every, Instant::now()))
    }

    /// True when a line is due at `now`; the first one is due at once.
    pub fn due(&self, now: Instant) -> bool {
        self.last
            .map_or(true, |l| now.duration_since(l) >= self.every)
    }

    /// The line for these numbers at `now` and the frame count, and the clock moved on. Call it
    /// only when [`MemLog::due`] says so, so the numbers are not read for nothing.
    pub fn line(
        &mut self,
        now: Instant,
        frame: u64,
        heap: &Heap,
        host: &Host,
        gpu: &Gpu,
    ) -> String {
        self.last = Some(now);
        self.lines += 1;
        self.peak_used = self.peak_used.max(heap.used);
        format_line(
            now.duration_since(self.start).as_secs_f64(),
            self.lines,
            frame,
            heap,
            self.peak_used,
            host,
            gpu,
        )
    }
}

/// One line: `NV_MEMLOG t=… n=… frame=… heap_used=… …`, every value an unsigned integer except `t`
/// (seconds, three decimals). `heap_peak` is the highest `heap_used` the sampler has seen.
pub fn format_line(
    t: f64,
    n: u64,
    frame: u64,
    heap: &Heap,
    heap_peak: u64,
    host: &Host,
    gpu: &Gpu,
) -> String {
    format!(
        "{TAG} t={t:.3} n={n} frame={frame} heap_used={} heap_peak={} heap_cap={} heap_free={} \
         heap_largest_free={} heap_top={} heap_peak_addr={} rss={} rss_peak={} gpu_kept_bytes={} \
         gpu_kept_levels={} gpu_bind_groups={} gpu_pipelines={} gpu_presents={}",
        heap.used,
        heap_peak,
        heap.capacity,
        heap.free,
        heap.largest_free,
        heap.top,
        heap.peak_addr,
        host.rss,
        host.rss_peak,
        gpu.kept_bytes,
        gpu.kept_levels,
        gpu.bind_groups,
        gpu.pipelines,
        gpu.presents
    )
}

/// Resident set and its peak from the text of `/proc/self/status` (`VmRSS` and `VmHWM`, in kB).
/// A missing line reads as 0; the peak is never below the current value.
pub fn parse_proc_status(text: &str) -> Host {
    let kb = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))
            .and_then(|v| v.split_whitespace().next()?.parse::<u64>().ok())
            .map_or(0, |k| k.saturating_mul(1024))
    };
    let rss = kb("VmRSS");
    Host {
        rss,
        rss_peak: kb("VmHWM").max(rss),
    }
}

/// The host process now.
#[cfg(target_os = "linux")]
pub fn host_now() -> Host {
    std::fs::read_to_string("/proc/self/status")
        .map(|t| parse_proc_status(&t))
        .unwrap_or_default()
}

/// The host process now (`GetProcessMemoryInfo`: working set and its peak).
#[cfg(windows)]
pub fn host_now() -> Host {
    #[repr(C)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set: usize,
        working_set: usize,
        rest: [usize; 6],
    }
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(p: isize, c: *mut Counters, cb: u32) -> i32;
    }
    let mut c = Counters {
        cb: std::mem::size_of::<Counters>() as u32,
        page_fault_count: 0,
        peak_working_set: 0,
        working_set: 0,
        rest: [0; 6],
    };
    // SAFETY: `c` is a PROCESS_MEMORY_COUNTERS-sized block with its size set; the pseudo-handle needs no close.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    if ok == 0 {
        return Host::default();
    }
    Host {
        rss: c.working_set as u64,
        rss_peak: c.peak_working_set as u64,
    }
}

/// Other platforms: no host numbers.
#[cfg(not(any(target_os = "linux", windows)))]
pub fn host_now() -> Host {
    Host::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = "Name:\tnv-viewer\nVmPeak:\t 9000000 kB\nVmHWM:\t 2048000 kB\nVmRSS:\t 1024000 kB\nThreads:\t12\n";

    #[test]
    fn proc_status_parses() {
        let h = parse_proc_status(STATUS);
        assert_eq!(h.rss, 1_024_000 * 1024);
        assert_eq!(h.rss_peak, 2_048_000 * 1024);
    }

    #[test]
    fn proc_status_missing_and_junk() {
        assert_eq!(parse_proc_status(""), Host::default());
        let h = parse_proc_status("VmRSS: abc kB\nVmHWM:\t5 kB\n");
        assert_eq!((h.rss, h.rss_peak), (0, 5 * 1024));
        // The peak is never below the current value.
        assert_eq!(parse_proc_status("VmRSS: 9 kB\n").rss_peak, 9 * 1024);
        // VmRSSX is not VmRSS.
        assert_eq!(parse_proc_status("VmRSSX: 9 kB\n").rss, 0);
    }

    #[test]
    fn interval_parsing() {
        assert_eq!(parse_interval(None), None);
        for off in ["", "0", "off", "yes", "-3", "nan", "inf", "100000"] {
            assert_eq!(parse_interval(Some(off)), None, "{off}");
        }
        assert_eq!(parse_interval(Some("1")), Some(Duration::from_secs(1)));
        assert_eq!(parse_interval(Some(" 5 ")), Some(Duration::from_secs(5)));
        assert_eq!(
            parse_interval(Some("0.5")),
            Some(Duration::from_millis(500))
        );
    }

    fn sample() -> (Heap, Host, Gpu) {
        (
            Heap {
                capacity: 3_400_000_000,
                used: 100,
                free: 7,
                largest_free: 5,
                top: 0x2000_1000,
                peak_addr: 0x2000_2000,
            },
            Host {
                rss: 1,
                rss_peak: 2,
            },
            Gpu {
                kept_bytes: 3,
                kept_levels: 4,
                bind_groups: 5,
                pipelines: 6,
                presents: 7,
            },
        )
    }

    #[test]
    fn line_is_key_value_and_numbers_only() {
        let (h, o, g) = sample();
        let l = format_line(1.5, 2, 99, &h, 120, &o, &g);
        assert!(l.starts_with("NV_MEMLOG t=1.500 n=2 frame=99 heap_used=100 heap_peak=120 "));
        assert!(!l.contains('\n'));
        for kv in l.split(' ').skip(1) {
            let (k, v) = kv
                .split_once('=')
                .unwrap_or_else(|| panic!("not key=value: {kv}"));
            assert!(!k.is_empty());
            assert!(v.parse::<f64>().is_ok(), "{k}={v}");
        }
        // No path separator, no drive letter, no quote.
        assert!(!l.contains(['/', '\\', ':', '"']));
        assert!(l.contains("rss=1 rss_peak=2 gpu_kept_bytes=3 gpu_kept_levels=4 gpu_bind_groups=5 gpu_pipelines=6 gpu_presents=7"));
    }

    #[test]
    fn peak_never_falls_and_cadence() {
        let t0 = Instant::now();
        let mut m = MemLog::new(Duration::from_secs(2), t0);
        let (mut h, o, g) = sample();
        assert!(m.due(t0));
        let mut peaks = Vec::new();
        let mut now = t0;
        for used in [50u64, 300, 10] {
            h.used = used;
            assert!(m.due(now));
            let l = m.line(now, 1, &h, &o, &g);
            assert!(!m.due(now + Duration::from_secs(1)));
            let peak: u64 = l
                .split(' ')
                .find_map(|kv| kv.strip_prefix("heap_peak="))
                .unwrap()
                .parse()
                .unwrap();
            peaks.push(peak);
            now += Duration::from_secs(2);
        }
        assert_eq!(peaks, vec![50, 300, 300]);
    }
}
