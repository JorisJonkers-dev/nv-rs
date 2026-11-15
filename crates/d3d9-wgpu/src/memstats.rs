//! The layer's memory counters, published for the fork's `NV_MEMLOG` timeline (`fork-memlog`).
//!
//! The device posts the numbers it already keeps (bytes held for read-backs, bind groups,
//! pipelines) every [`PUBLISH_EVERY`] presents into relaxed atomics; [`snapshot`] reads them from
//! any thread. Read-only for the rest of the engine: nothing here changes what is drawn.

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

/// Presents between two posts (a sum over the kept levels each time).
pub(crate) const PUBLISH_EVERY: u64 = 30;

static KEPT_BYTES: AtomicU64 = AtomicU64::new(0);
static KEPT_LEVELS: AtomicU64 = AtomicU64::new(0);
static BIND_GROUPS: AtomicU64 = AtomicU64::new(0);
static PIPELINES: AtomicU64 = AtomicU64::new(0);
static PRESENTS: AtomicU64 = AtomicU64::new(0);

/// The counters at the last post; all zero before the first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuMemStats {
    /// Host copies kept for read-backs.
    pub kept_bytes: u64,
    pub kept_levels: u64,
    pub bind_groups: u64,
    pub pipelines: u64,
    pub presents: u64,
}

pub(crate) fn publish(s: GpuMemStats) {
    KEPT_BYTES.store(s.kept_bytes, Relaxed);
    KEPT_LEVELS.store(s.kept_levels, Relaxed);
    BIND_GROUPS.store(s.bind_groups, Relaxed);
    PIPELINES.store(s.pipelines, Relaxed);
    PRESENTS.store(s.presents, Relaxed);
}

/// The last posted counters.
pub fn snapshot() -> GpuMemStats {
    GpuMemStats {
        kept_bytes: KEPT_BYTES.load(Relaxed),
        kept_levels: KEPT_LEVELS.load(Relaxed),
        bind_groups: BIND_GROUPS.load(Relaxed),
        pipelines: PIPELINES.load(Relaxed),
        presents: PRESENTS.load(Relaxed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_then_snapshot() {
        let s = GpuMemStats {
            kept_bytes: 9,
            kept_levels: 2,
            bind_groups: 3,
            pipelines: 4,
            presents: 60,
        };
        publish(s);
        assert_eq!(snapshot(), s);
    }
}
