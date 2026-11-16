//! Percentiles and CPU clocks.

/// A sample set summarised.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Summary {
    pub n: usize,
    pub mean: f64,
    pub min: f64,
    pub p50: f64,
    pub p90: f64,
    pub p99: f64,
    pub max: f64,
}

/// The summary of `values` (sorted in place). The percentile `p` is the nearest rank.
pub fn summarize(values: &mut [f64]) -> Summary {
    if values.is_empty() {
        return Summary::default();
    }
    values.sort_by(|a, b| a.total_cmp(b));
    let n = values.len();
    let at = |p: f64| values[(((n as f64) * p).ceil() as usize).clamp(1, n) - 1];
    Summary {
        n,
        mean: values.iter().sum::<f64>() / n as f64,
        min: values[0],
        p50: at(0.50),
        p90: at(0.90),
        p99: at(0.99),
        max: values[n - 1],
    }
}

/// Nanoseconds of CPU time the calling thread has used, where the platform tells.
pub fn thread_cpu_ns() -> Option<u64> {
    #[cfg(unix)]
    return clock(libc::CLOCK_THREAD_CPUTIME_ID);
    #[cfg(not(unix))]
    None
}

/// Nanoseconds of CPU time all the process's threads have used, where the platform tells.
pub fn process_cpu_ns() -> Option<u64> {
    #[cfg(unix)]
    return clock(libc::CLOCK_PROCESS_CPUTIME_ID);
    #[cfg(not(unix))]
    None
}

#[cfg(unix)]
fn clock(id: libc::clockid_t) -> Option<u64> {
    let mut t = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `t` is a valid timespec for the call to fill.
    let ok = unsafe { libc::clock_gettime(id, &mut t) } == 0;
    ok.then(|| t.tv_sec as u64 * 1_000_000_000 + t.tv_nsec as u64)
}

/// The CPU time each thread of the process has used, by thread name (Linux: the scheduler's own
/// count in nanoseconds, `/proc/self/task/*/schedstat`); empty where there is none.
pub fn threads_cpu_ns() -> Vec<(String, u64)> {
    let mut out: Vec<(String, u64)> = Vec::new();
    let Ok(dir) = std::fs::read_dir("/proc/self/task") else {
        return out;
    };
    for e in dir.flatten() {
        let p = e.path();
        let ns = std::fs::read_to_string(p.join("schedstat"))
            .ok()
            .and_then(|s| s.split_whitespace().next()?.parse::<u64>().ok());
        let name = std::fs::read_to_string(p.join("comm")).unwrap_or_default();
        if let Some(ns) = ns {
            let name = name.trim().to_string();
            match out.iter_mut().find(|(n, _)| *n == name) {
                Some(x) => x.1 += ns,
                None => out.push((name, ns)),
            }
        }
    }
    out
}

/// The cost of reading the wall clock twice, in nanoseconds (what timing one call adds).
pub fn timer_overhead_ns() -> f64 {
    let n = 100_000;
    let start = std::time::Instant::now();
    let mut sink = 0u64;
    for _ in 0..n {
        let a = std::time::Instant::now();
        sink = sink.wrapping_add(a.elapsed().as_nanos() as u64);
    }
    std::hint::black_box(sink);
    start.elapsed().as_nanos() as f64 / f64::from(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_are_nearest_rank() {
        let mut v: Vec<f64> = (1..=100).map(f64::from).collect();
        v.reverse();
        let s = summarize(&mut v);
        assert_eq!(
            (s.n, s.min, s.p50, s.p90, s.p99, s.max),
            (100, 1.0, 50.0, 90.0, 99.0, 100.0)
        );
        assert!((s.mean - 50.5).abs() < 1e-9);
        assert_eq!(summarize(&mut []), Summary::default());
    }

    #[test]
    fn cpu_clocks_advance() {
        if let (Some(a), Some(p)) = (thread_cpu_ns(), process_cpu_ns()) {
            let mut x = 0u64;
            for i in 0..2_000_000u64 {
                x = x.wrapping_add(std::hint::black_box(i));
            }
            std::hint::black_box(x);
            assert!(thread_cpu_ns().unwrap() > a);
            assert!(process_cpu_ns().unwrap() >= p);
        }
    }
}
