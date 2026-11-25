//! A budget for the video memory this layer asks the driver for.
//!
//! A game that creates textures and render targets without end (or one of an absurd size) used
//! to run the card out of memory: the amdgpu driver then failed every submission of the whole
//! session, the compositor included. The layer counts the bytes of every texture and buffer it
//! makes (released when the object is dropped), refuses what goes past the budget or past a
//! sane size with one structured `stop:` line, and the game goes on without that object, as it
//! does when a card cannot make it.
//!
//! The budget is `NV_GPU_BUDGET_MB` (0 switches it off), else 80 % of the first DRM card's VRAM
//! (Linux, `mem_info_vram_total`), else none; a texture side over [`MAX_SIDE`] is always refused.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// The largest side of a texture the layer makes: Direct3D 9 hardware stops at 16384.
pub(crate) const MAX_SIDE: u32 = 16384;

/// The share of the card's video memory the layer may use when nothing says otherwise, in percent.
const DEFAULT_SHARE: u64 = 80;

/// Why an allocation was refused.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// A side past [`MAX_SIDE`] (or the device's own limit).
    Size,
    /// The live bytes would go past the budget.
    Budget,
}

/// What an object charged to the budget is, for the memory timeline's breakdown (`NV_MEMLOG`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A texture the game samples, block-compressed (DXT kept as BC on the card).
    Compressed,
    /// A texture the game samples, not block-compressed.
    Plain,
    /// A colour render target of one sample (also a sampled copy of one, a resolve's scratch).
    Target,
    /// A multisampled colour target.
    Multisampled,
    /// A depth or stencil surface.
    Depth,
    /// A vertex buffer of the game's.
    Vertex,
    /// An index buffer of the game's.
    Index,
    /// The layer's own (the sun shadows' lists of a caster's pieces).
    Own,
}

impl Kind {
    pub(crate) const ALL: [Kind; 8] = [
        Kind::Compressed,
        Kind::Plain,
        Kind::Target,
        Kind::Multisampled,
        Kind::Depth,
        Kind::Vertex,
        Kind::Index,
        Kind::Own,
    ];

    /// The kind of a texture of this format that is (`target`) or is not drawn into.
    pub(crate) fn of_texture(block: bool, depth: bool, target: bool, samples: u32) -> Kind {
        if depth {
            Kind::Depth
        } else if samples > 1 {
            Kind::Multisampled
        } else if target {
            Kind::Target
        } else if block {
            Kind::Compressed
        } else {
            Kind::Plain
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// The live bytes and the limit; shared by the device and every [`Charge`].
pub(crate) struct Budget {
    limit: u64,
    live: AtomicU64,
    peak: AtomicU64,
    refused: AtomicU64,
    objects: AtomicU64,
    /// Live bytes and objects of each [`Kind`].
    kind_bytes: [AtomicU64; 8],
    kind_objects: [AtomicU64; 8],
}

/// The bytes of one object, given back to the budget when it is dropped.
pub(crate) struct Charge {
    budget: Arc<Budget>,
    bytes: u64,
    kind: Kind,
}

impl Charge {
    /// The bytes charged.
    pub(crate) fn bytes(&self) -> u64 {
        self.bytes
    }
}

impl Drop for Charge {
    fn drop(&mut self) {
        self.budget.live.fetch_sub(self.bytes, Ordering::Relaxed);
        self.budget.objects.fetch_sub(1, Ordering::Relaxed);
        let k = self.kind.index();
        self.budget.kind_bytes[k].fetch_sub(self.bytes, Ordering::Relaxed);
        self.budget.kind_objects[k].fetch_sub(1, Ordering::Relaxed);
    }
}

/// The bytes of a texture: `levels` mip levels of `layers` layers, `samples` per pixel;
/// `bytes` is per pixel, or per 4x4 block when `block`.
pub(crate) fn texture_bytes(
    width: u32,
    height: u32,
    levels: u32,
    layers: u32,
    samples: u32,
    bytes: u32,
    block: bool,
) -> u64 {
    let mut total = 0u64;
    for level in 0..levels.max(1) {
        let w = u64::from((width >> level).max(1));
        let h = u64::from((height >> level).max(1));
        let cells = if block {
            w.div_ceil(4) * h.div_ceil(4)
        } else {
            w * h
        };
        total += cells * u64::from(bytes);
    }
    total * u64::from(layers.max(1)) * u64::from(samples.max(1))
}

/// The limit for a card with `vram_total` bytes and `NV_GPU_BUDGET_MB`'s value, 0 for none.
pub(crate) fn limit_for(env_mb: Option<&str>, vram_total: Option<u64>) -> u64 {
    if let Some(mb) = env_mb.and_then(|v| v.trim().parse::<u64>().ok()) {
        return mb.saturating_mul(1 << 20);
    }
    vram_total.map_or(0, |v| v / 100 * DEFAULT_SHARE)
}

fn sysfs_vram_total() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        for n in 0..8 {
            let p = format!("/sys/class/drm/card{n}/device/mem_info_vram_total");
            if let Some(v) = std::fs::read_to_string(p)
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok())
            {
                if v > 0 {
                    return Some(v);
                }
            }
        }
    }
    None
}

impl Budget {
    pub(crate) fn new(limit: u64) -> Arc<Self> {
        Arc::new(Self {
            limit,
            live: AtomicU64::new(0),
            peak: AtomicU64::new(0),
            refused: AtomicU64::new(0),
            objects: AtomicU64::new(0),
            kind_bytes: Default::default(),
            kind_objects: Default::default(),
        })
    }

    /// The budget this machine and environment ask for.
    pub(crate) fn from_environment() -> Arc<Self> {
        let env = std::env::var("NV_GPU_BUDGET_MB").ok();
        Self::new(limit_for(env.as_deref(), sysfs_vram_total()))
    }

    /// Takes `bytes` for an object of `kind`, `width` by `height`, or says why not.
    pub(crate) fn take(
        self: &Arc<Self>,
        kind: Kind,
        width: u32,
        height: u32,
        bytes: u64,
    ) -> Result<Charge, Refusal> {
        if width > MAX_SIDE || height > MAX_SIDE {
            self.refused.fetch_add(1, Ordering::Relaxed);
            return Err(Refusal::Size);
        }
        let now = self.live.fetch_add(bytes, Ordering::Relaxed) + bytes;
        if self.limit != 0 && now > self.limit {
            self.live.fetch_sub(bytes, Ordering::Relaxed);
            self.refused.fetch_add(1, Ordering::Relaxed);
            return Err(Refusal::Budget);
        }
        self.peak.fetch_max(now, Ordering::Relaxed);
        self.objects.fetch_add(1, Ordering::Relaxed);
        self.kind_bytes[kind.index()].fetch_add(bytes, Ordering::Relaxed);
        self.kind_objects[kind.index()].fetch_add(1, Ordering::Relaxed);
        Ok(Charge {
            budget: self.clone(),
            bytes,
            kind,
        })
    }

    /// Counts a refusal made by the caller (a side past the device's own limit).
    pub(crate) fn note_refused(&self) {
        self.refused.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn live(&self) -> u64 {
        self.live.load(Ordering::Relaxed)
    }

    pub(crate) fn refused(&self) -> u64 {
        self.refused.load(Ordering::Relaxed)
    }

    /// The live bytes and objects of `kind`.
    pub(crate) fn of_kind(&self, kind: Kind) -> (u64, u64) {
        (
            self.kind_bytes[kind.index()].load(Ordering::Relaxed),
            self.kind_objects[kind.index()].load(Ordering::Relaxed),
        )
    }

    /// One report line: live and peak bytes, objects, the limit and the refusals.
    pub(crate) fn report(&self) -> String {
        let mb = |b: u64| b as f64 / 1_048_576.0;
        format!(
            "gpu memory made by the layer: {:.1} MB live in {} objects (peak {:.1} MB), budget {} MB, {} refused",
            mb(self.live()),
            self.objects.load(Ordering::Relaxed),
            mb(self.peak.load(Ordering::Relaxed)),
            self.limit / (1 << 20),
            self.refused()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_of_a_texture_count_levels_layers_and_samples() {
        // 4x4 RGBA8 with 3 levels: 64 + 16 + 4.
        assert_eq!(texture_bytes(4, 4, 3, 1, 1, 4, false), 84);
        assert_eq!(texture_bytes(4, 4, 1, 6, 1, 4, false), 384);
        assert_eq!(texture_bytes(4, 4, 1, 1, 4, 4, false), 256);
        // DXT1: 8 bytes a block, a 1x1 level is one block.
        assert_eq!(texture_bytes(8, 8, 4, 1, 1, 8, true), 32 + 8 + 8 + 8);
    }

    #[test]
    fn the_limit_is_the_variable_then_a_share_of_the_card_then_none() {
        assert_eq!(limit_for(Some("512"), Some(1 << 40)), 512 << 20);
        assert_eq!(limit_for(Some("0"), Some(1 << 40)), 0);
        assert_eq!(limit_for(Some("x"), Some(100)), 80);
        assert_eq!(limit_for(None, Some(1000)), 800);
        assert_eq!(limit_for(None, None), 0);
    }

    #[test]
    fn an_absurd_side_is_refused_and_nothing_is_charged() {
        let b = Budget::new(0);
        assert_eq!(b.take(Kind::Plain, 65536, 4, 1).err(), Some(Refusal::Size));
        assert_eq!(
            b.take(Kind::Plain, 4, 1 << 30, 1).err(),
            Some(Refusal::Size)
        );
        assert_eq!(b.live(), 0);
        assert_eq!(b.refused(), 2);
    }

    #[test]
    fn bytes_past_the_budget_are_refused_and_dropped_objects_give_theirs_back() {
        let b = Budget::new(1000);
        let a = b.take(Kind::Plain, 8, 8, 600).unwrap();
        assert_eq!(b.take(Kind::Plain, 8, 8, 600).err(), Some(Refusal::Budget));
        assert_eq!(b.live(), 600);
        drop(a);
        assert_eq!(b.live(), 0);
        assert!(b.take(Kind::Plain, 8, 8, 600).is_ok());
        assert!(b.report().contains("1 refused"));
    }

    #[test]
    fn each_kind_counts_its_live_bytes_and_objects() {
        let b = Budget::new(0);
        let bc = b.take(Kind::Compressed, 8, 8, 32).unwrap();
        let bc2 = b.take(Kind::Compressed, 8, 8, 32).unwrap();
        let vb = b.take(Kind::Vertex, 1, 1, 100).unwrap();
        assert_eq!(b.of_kind(Kind::Compressed), (64, 2));
        assert_eq!(b.of_kind(Kind::Vertex), (100, 1));
        assert_eq!(b.of_kind(Kind::Target), (0, 0));
        assert_eq!(bc.bytes(), 32);
        drop(bc);
        drop(vb);
        assert_eq!(b.of_kind(Kind::Compressed), (32, 1));
        assert_eq!(b.of_kind(Kind::Vertex), (0, 0));
        assert_eq!(b.live(), 32);
        drop(bc2);
        assert_eq!(b.live(), 0);
    }

    #[test]
    fn a_texture_kind_follows_depth_samples_target_then_blocks() {
        assert_eq!(Kind::of_texture(true, false, false, 1), Kind::Compressed);
        assert_eq!(Kind::of_texture(false, false, false, 1), Kind::Plain);
        assert_eq!(Kind::of_texture(false, false, true, 1), Kind::Target);
        assert_eq!(Kind::of_texture(false, false, true, 4), Kind::Multisampled);
        assert_eq!(Kind::of_texture(false, true, false, 4), Kind::Depth);
    }

    #[test]
    fn a_leak_of_render_targets_stops_at_the_budget() {
        // 1920x1080 RGBA8 targets made and kept without end: the 24 GB card's 80 % budget.
        let b = Budget::new(limit_for(None, Some(24 << 30)));
        let each = texture_bytes(1920, 1080, 1, 1, 1, 4, false);
        let mut kept = Vec::new();
        while let Ok(c) = b.take(Kind::Target, 1920, 1080, each) {
            kept.push(c);
            assert!(kept.len() < 10_000, "the budget never stopped the leak");
        }
        assert!(b.live() <= b.limit);
        assert!(kept.len() > 1000);
    }
}
