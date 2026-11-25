//! The layer's memory counters, published for the fork's `NV_MEMLOG` timeline (`fork-memlog`).
//!
//! The device posts the numbers it already keeps (bytes held for read-backs, bind groups,
//! pipelines, the video memory it made by kind) every [`PUBLISH_EVERY`] presents into relaxed
//! atomics; [`snapshot`] reads them from any thread. Read-only for the rest of the engine: nothing
//! here changes what is drawn.

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

/// Presents between two posts (a sum over the kept levels each time).
pub(crate) const PUBLISH_EVERY: u64 = 30;

static KEPT_BYTES: AtomicU64 = AtomicU64::new(0);
static KEPT_LEVELS: AtomicU64 = AtomicU64::new(0);
static BIND_GROUPS: AtomicU64 = AtomicU64::new(0);
static PIPELINES: AtomicU64 = AtomicU64::new(0);
static PRESENTS: AtomicU64 = AtomicU64::new(0);
static PARTS: [AtomicU64; PARTS_LEN] = [const { AtomicU64::new(0) }; PARTS_LEN];

/// How many numbers [`Parts`] holds.
const PARTS_LEN: usize = 15;

/// The layer's memory by kind, bytes: what it made on the card (charged to its budget,
/// [`crate::budget`]), the host copies it keeps, and the driver allocator's totals.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Parts {
    /// Sampled textures kept block-compressed (DXT as BC).
    pub tex_compressed: u64,
    /// Sampled textures not block-compressed.
    pub tex_plain: u64,
    /// Colour render targets of one sample, with their copies.
    pub targets: u64,
    /// Multisampled colour targets.
    pub targets_msaa: u64,
    /// Depth and stencil surfaces.
    pub depth: u64,
    /// The game's vertex buffers.
    pub vertex: u64,
    /// The game's index buffers.
    pub index: u64,
    /// The layer's own buffers charged to its budget (the sun shadows' lists).
    pub own: u64,
    /// Textures (all kinds) alive.
    pub textures: u64,
    /// The host copy of each vertex and index buffer of the game's.
    pub buffer_copies: u64,
    /// The driver allocator: bytes handed out and bytes reserved from the driver (0 where the
    /// backend gives no report).
    pub alloc_allocated: u64,
    pub alloc_reserved: u64,
    /// Compiled shader modules kept.
    pub modules: u64,
    /// `ReadSurface` answers given from the kept host copies so far, and their bytes.
    pub reads: u64,
    pub read_bytes: u64,
}

static READS: AtomicU64 = AtomicU64::new(0);
static READ_BYTES: AtomicU64 = AtomicU64::new(0);

/// Counts one `ReadSurface` answered with `bytes` bytes.
pub(crate) fn note_read(bytes: u64) {
    READS.fetch_add(1, Relaxed);
    READ_BYTES.fetch_add(bytes, Relaxed);
}

/// The reads counted so far ([`note_read`]).
pub(crate) fn reads() -> (u64, u64) {
    (READS.load(Relaxed), READ_BYTES.load(Relaxed))
}

impl Parts {
    fn to_array(self) -> [u64; PARTS_LEN] {
        [
            self.tex_compressed,
            self.tex_plain,
            self.targets,
            self.targets_msaa,
            self.depth,
            self.vertex,
            self.index,
            self.own,
            self.textures,
            self.buffer_copies,
            self.alloc_allocated,
            self.alloc_reserved,
            self.modules,
            self.reads,
            self.read_bytes,
        ]
    }

    fn from_array(a: [u64; PARTS_LEN]) -> Self {
        Parts {
            tex_compressed: a[0],
            tex_plain: a[1],
            targets: a[2],
            targets_msaa: a[3],
            depth: a[4],
            vertex: a[5],
            index: a[6],
            own: a[7],
            textures: a[8],
            buffer_copies: a[9],
            alloc_allocated: a[10],
            alloc_reserved: a[11],
            modules: a[12],
            reads: a[13],
            read_bytes: a[14],
        }
    }

    /// `key=value` pairs for the timeline line, in a fixed order.
    pub fn pairs(&self) -> [(&'static str, u64); PARTS_LEN] {
        let a = self.to_array();
        [
            ("gpu_tex_bc", a[0]),
            ("gpu_tex_plain", a[1]),
            ("gpu_targets", a[2]),
            ("gpu_targets_msaa", a[3]),
            ("gpu_depth", a[4]),
            ("gpu_vertex", a[5]),
            ("gpu_index", a[6]),
            ("gpu_own", a[7]),
            ("gpu_textures", a[8]),
            ("gpu_buffer_copies", a[9]),
            ("gpu_alloc_allocated", a[10]),
            ("gpu_alloc_reserved", a[11]),
            ("gpu_modules", a[12]),
            ("gpu_reads", a[13]),
            ("gpu_read_bytes", a[14]),
        ]
    }
}

/// The counters at the last post; all zero before the first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuMemStats {
    /// Host copies kept for read-backs.
    pub kept_bytes: u64,
    pub kept_levels: u64,
    pub bind_groups: u64,
    pub pipelines: u64,
    pub presents: u64,
    pub parts: Parts,
}

pub(crate) fn publish(s: GpuMemStats) {
    for (slot, v) in PARTS.iter().zip(s.parts.to_array()) {
        slot.store(v, Relaxed);
    }
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
        parts: Parts::from_array(std::array::from_fn(|k| PARTS[k].load(Relaxed))),
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
            parts: Parts {
                tex_compressed: 1,
                tex_plain: 2,
                targets: 3,
                targets_msaa: 4,
                depth: 5,
                vertex: 6,
                index: 7,
                own: 8,
                textures: 9,
                buffer_copies: 10,
                alloc_allocated: 11,
                alloc_reserved: 12,
                modules: 13,
                reads: 14,
                read_bytes: 15,
            },
        };
        publish(s);
        assert_eq!(snapshot(), s);
        let keys: Vec<_> = s.parts.pairs().iter().map(|p| p.0).collect();
        assert_eq!(keys.len(), 15);
        assert_eq!(s.parts.pairs()[12], ("gpu_modules", 13));
        assert!(keys.iter().all(|k| k.starts_with("gpu_")));
    }
}
