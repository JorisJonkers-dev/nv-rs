// SPDX-License-Identifier: GPL-3.0-only
//! The start check of the memory guard (the card's part of `scripts/fork/bench/guard.py`): a run is refused when
//! the bench runs are halted or when the card's free VRAM or GTT cannot hold the run's budget plus a margin kept for
//! the desktop, and the engine gets the VRAM budget as `NV_GPU_BUDGET_MB` (it refuses allocations past it with a
//! `stop:` line). The host memory cap is the `gamerun` scope's (`nv-rs-scene` `--mem`).
//! The shape is the old guard's: VRAM share 0.5 and margin 2048 MiB, GTT share 0.25 and margin 1024 MiB.
//! Not here (they stay in the Python guard until its callers are gone): the 100 ms sampler that stops a run
//! before the card's use reaches its cap, the kernel-log and desktop-compositor check, and writing the halt file.

use std::path::{Path, PathBuf};

const MIB: f64 = 1_048_576.0;
const VRAM_SHARE: f64 = 0.5;
const GTT_SHARE: f64 = 0.25;
const VRAM_MARGIN_MIB: f64 = 2048.0;
const GTT_MARGIN_MIB: f64 = 1024.0;

/// The card's use and totals in MiB (every process on the card counts).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuMem {
    pub vram_used: f64,
    pub vram_total: f64,
    pub gtt_used: f64,
    pub gtt_total: f64,
}

/// What a run may use of the card.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Caps {
    pub vram_mib: u64,
    pub gtt_mib: u64,
}

fn number(dir: &Path, name: &str) -> Option<f64> {
    std::fs::read_to_string(dir.join(name))
        .ok()?
        .trim()
        .parse::<f64>()
        .ok()
}

/// The card's memory from its amdgpu sysfs directory (`mem_info_vram_used` and the three more), or `None`.
pub fn read_gpu(dir: &Path) -> Option<GpuMem> {
    let m = GpuMem {
        vram_used: number(dir, "mem_info_vram_used")? / MIB,
        vram_total: number(dir, "mem_info_vram_total")? / MIB,
        gtt_used: number(dir, "mem_info_gtt_used")? / MIB,
        gtt_total: number(dir, "mem_info_gtt_total")? / MIB,
    };
    (m.vram_total > 0.0 && m.gtt_total > 0.0).then_some(m)
}

/// The card with the most VRAM under `root` (`/sys/class/drm`: `card*/device`), or `None`.
pub fn find_gpu(root: &Path) -> Option<PathBuf> {
    let mut best: Option<(f64, PathBuf)> = None;
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        let dir = entry.path().join("device");
        if let Some(total) = number(&dir, "mem_info_vram_total") {
            if best.as_ref().is_none_or(|(t, _)| total > *t) {
                best = Some((total, dir));
            }
        }
    }
    best.map(|(_, d)| d)
}

/// The run's budget on this card: half the VRAM and a quarter of the GTT.
pub fn default_caps(gpu: &GpuMem) -> Caps {
    Caps {
        vram_mib: (gpu.vram_total * VRAM_SHARE) as u64,
        gtt_mib: (gpu.gtt_total * GTT_SHARE) as u64,
    }
}

/// Refuses a run whose budget plus the desktop's margin does not fit in the card's free memory now.
pub fn preflight(caps: &Caps, gpu: &GpuMem) -> Result<(), String> {
    let free_v = gpu.vram_total - gpu.vram_used;
    let free_g = gpu.gtt_total - gpu.gtt_used;
    if free_v < caps.vram_mib as f64 + VRAM_MARGIN_MIB {
        return Err(format!("VRAM: {free_v:.0} MiB free of {:.0}, the run needs {} plus {VRAM_MARGIN_MIB:.0} kept for the desktop", gpu.vram_total, caps.vram_mib));
    }
    if free_g < caps.gtt_mib as f64 + GTT_MARGIN_MIB {
        return Err(format!("GTT: {free_g:.0} MiB free of {:.0}, the run needs {} plus {GTT_MARGIN_MIB:.0} kept for the desktop", gpu.gtt_total, caps.gtt_mib));
    }
    Ok(())
}

/// The halt file's reason when the bench runs are halted (`~/.cache/nv-rs/bench.halt`, written by the Python guard, removed
/// by `nv-rs-dev bench clear-halt`), else `None`.
pub fn halted(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let reason = text
        .split("\"reason\"")
        .nth(1)
        .and_then(|r| r.split('"').nth(1))
        .unwrap_or("unreadable halt file");
    Some(reason.to_string())
}

/// The check before an engine start: `Ok(Some(caps))` when the card has room (hand `caps.vram_mib` to the engine as
/// `NV_GPU_BUDGET_MB`), `Ok(None)` when the card cannot be read here (no amdgpu sysfs: another vendor or system; the caller
/// says so), `Err` with the reason when the run is refused.
pub fn check(sysfs_root: &Path, halt_file: &Path) -> Result<Option<Caps>, String> {
    if let Some(reason) = halted(halt_file) {
        return Err(format!(
            "bench runs are halted: {reason} (`nv-rs-dev bench clear-halt` removes {})",
            halt_file.display()
        ));
    }
    let Some(gpu) = find_gpu(sysfs_root).and_then(|d| read_gpu(&d)) else {
        return Ok(None);
    };
    let caps = default_caps(&gpu);
    preflight(&caps, &gpu)?;
    Ok(Some(caps))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(root: &Path, name: &str, vram: (u64, u64), gtt: (u64, u64)) {
        let d = root.join(name).join("device");
        std::fs::create_dir_all(&d).unwrap();
        for (n, v) in [
            ("mem_info_vram_used", vram.0),
            ("mem_info_vram_total", vram.1),
            ("mem_info_gtt_used", gtt.0),
            ("mem_info_gtt_total", gtt.1),
        ] {
            std::fs::write(d.join(n), format!("{}\n", v * 1_048_576)).unwrap();
        }
    }

    fn tmp(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fork-scene-guard-{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_card_with_the_most_vram_is_read_and_its_budget_is_half_and_a_quarter() {
        let t = tmp("find");
        card(&t, "card0", (100, 512), (10, 1024));
        card(&t, "card1", (3000, 24576), (500, 32768));
        std::fs::create_dir_all(t.join("renderD128")).unwrap();
        let gpu = read_gpu(&find_gpu(&t).unwrap()).unwrap();
        assert_eq!((gpu.vram_used, gpu.vram_total), (3000.0, 24576.0));
        assert_eq!(
            default_caps(&gpu),
            Caps {
                vram_mib: 12288,
                gtt_mib: 8192
            }
        );
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn a_run_is_refused_when_the_free_memory_cannot_hold_the_budget_and_the_margin() {
        let t = tmp("pre");
        let halt = t.join("none");
        card(&t, "card0", (3000, 24576), (500, 32768));
        assert_eq!(
            check(&t, &halt).unwrap(),
            Some(Caps {
                vram_mib: 12288,
                gtt_mib: 8192
            })
        );
        // 24576 - 12000 = 12576 free: less than 12288 + 2048.
        card(&t, "card0", (12000, 24576), (500, 32768));
        let e = check(&t, &halt).unwrap_err();
        assert!(e.starts_with("VRAM: 12576 MiB free of 24576"), "{e}");
        card(&t, "card0", (3000, 24576), (25000, 32768));
        assert!(check(&t, &halt).unwrap_err().starts_with("GTT:"));
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn a_halt_file_refuses_and_no_card_is_not_a_refusal() {
        let t = tmp("halt");
        let halt = t.join("bench.halt");
        assert_eq!(check(&t, &halt).unwrap(), None);
        std::fs::write(
            &halt,
            r#"{"time": "2026-11-22T05:00:00", "reason": "kernel: GPU reset"}"#,
        )
        .unwrap();
        let e = check(&t, &halt).unwrap_err();
        assert!(
            e.contains("kernel: GPU reset") && e.contains("clear-halt"),
            "{e}"
        );
        std::fs::write(&halt, "garbage").unwrap();
        assert!(check(&t, &halt)
            .unwrap_err()
            .contains("unreadable halt file"));
        std::fs::remove_dir_all(&t).unwrap();
    }
}
