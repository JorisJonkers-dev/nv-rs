// SPDX-License-Identifier: GPL-3.0-only
//! The heap guard (nv-rs#227): with `NV_HEAP_STOP_FRACTION` set, a run ends gracefully, with a
//! stop line and a structured stop report, when the share of the guest heap in use reaches it,
//! instead of meeting the heap's exhaustion later. Off unless set; read-only (one counters read
//! every [`CHECK_EVERY`] frames), no change to any allocation path.

use engine::Engine;

/// `NV_HEAP_STOP_FRACTION`: the share of the guest heap in use (0 to 1, or a percentage above 1
/// up to 100). `None` for unset, unreadable or out of range.
pub fn fraction(value: Option<&str>) -> Option<f64> {
    let v: f64 = value?.trim().trim_end_matches('%').parse().ok()?;
    let v = if v > 1.0 { v / 100.0 } else { v };
    (v > 0.0 && v <= 1.0).then_some(v)
}

/// Whether `used` of `capacity` bytes reaches `fraction`.
pub fn over(used: u64, capacity: u64, fraction: f64) -> bool {
    capacity > 0 && used as f64 >= capacity as f64 * fraction
}

/// The stop line's text.
pub fn stop_text(fraction: f64, s: &engine_core::HeapStats) -> String {
    format!(
        "graceful stop: the guest heap is {:.1}% in use ({} of {} MB; NV_HEAP_STOP_FRACTION {:.0}%); \
         largest free block {} KB, highest address {:08x}",
        100.0 * s.used as f64 / s.capacity.max(1) as f64,
        s.used >> 20,
        s.capacity >> 20,
        fraction * 100.0,
        s.largest_free >> 10,
        s.peak
    )
}

/// How often the guard looks at the heap, in frames.
pub const CHECK_EVERY: u64 = 30;

#[derive(Default)]
pub struct Guard {
    fraction: Option<Option<f64>>,
    frames: u64,
}

impl Guard {
    /// Once a frame: looks every [`CHECK_EVERY`] frames and, at the fraction, logs the stop line,
    /// writes the stop report and asks the run to end (once).
    pub fn sample(&mut self, e: &Engine) {
        let fraction = *self.fraction.get_or_insert_with(|| {
            fraction(std::env::var("NV_HEAP_STOP_FRACTION").ok().as_deref())
        });
        let Some(fraction) = fraction else {
            return;
        };
        self.frames += 1;
        if self.frames % CHECK_EVERY != 0 || fork_log::stop_requested() {
            return;
        }
        let stats = e.mem.heap_stats();
        if !over(stats.used, stats.capacity, fraction) {
            return;
        }
        let why = stop_text(fraction, &stats);
        fork_log::error!(target: "stop", "{why}");
        fork_log::report_stop(&why);
        fork_log::request_stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fraction_is_off_unless_set_and_takes_a_share_or_a_percentage() {
        assert_eq!(fraction(None), None);
        assert_eq!(fraction(Some("")), None);
        assert_eq!(fraction(Some("0")), None);
        assert_eq!(fraction(Some("abc")), None);
        assert_eq!(fraction(Some("0.9")), Some(0.9));
        assert_eq!(fraction(Some("90%")), Some(0.9));
        assert_eq!(fraction(Some("90")), Some(0.9));
        assert_eq!(fraction(Some("101")), None);
    }

    #[test]
    fn the_heap_is_over_at_the_fraction() {
        assert!(!over(89, 100, 0.9));
        assert!(over(90, 100, 0.9));
        assert!(!over(5, 0, 0.9));
    }

    #[test]
    fn the_stop_line_says_use_capacity_and_the_switch() {
        let s = engine_core::HeapStats {
            capacity: 3000 << 20,
            used: 2700 << 20,
            largest_free: 64 << 10,
            peak: 0x6f00_0000,
            ..Default::default()
        };
        let text = stop_text(0.9, &s);
        assert!(text.starts_with("graceful stop: the guest heap is 90.0% in use (2700 of 3000 MB;"));
        assert!(text.contains("NV_HEAP_STOP_FRACTION 90%"));
        assert!(text.contains("highest address 6f000000"));
    }
}
