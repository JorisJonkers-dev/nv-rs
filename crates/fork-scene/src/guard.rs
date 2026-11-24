// SPDX-License-Identifier: GPL-3.0-only
//! The start check of the memory guard (the card's part of `scripts/fork/bench/guard.py`): a run is refused when
//! the bench runs are halted or when the card's free VRAM or GTT cannot hold the run's budget plus a margin kept for
//! the desktop, and the engine gets the VRAM budget as `NV_GPU_BUDGET_MB` (it refuses allocations past it with a
//! `stop:` line). The host memory cap is the `gamerun` scope's (`nv-rs-scene` `--mem`).
//! The shape is the old guard's: VRAM share 0.5 and margin 2048 MiB, GTT margin 1024 MiB; the GTT share is 0.4 (it was the
//! old guard's 0.25, too little for the modded load orders' textures), `NV_GUARD_GTT_SHARE` sets it, at most 0.5.
//! Also here, the other pieces of the old Python guard: the sampler that stops a run before the card's use reaches its
//! cap ([`stop_at`], [`over`]; the scene tool reads the card every 100 ms while an engine runs), the session check (a GPU reset
//! or an out-of-memory command submission in the kernel log, or the desktop's compositor gone: [`session_trouble`]) and the
//! halt file ([`halt`], [`clear_halt`]; a run stopped by the guard halts every later run until `nv-rs-scene guard clear-halt`).

use std::path::{Path, PathBuf};

const MIB: f64 = 1_048_576.0;
const VRAM_SHARE: f64 = 0.5;
/// The run's share of the card's GTT by default, and the most `NV_GUARD_GTT_SHARE` may set.
const GTT_SHARE: f64 = 0.4;
const GTT_SHARE_MAX: f64 = 0.5;
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

/// The GTT share from the value of `NV_GUARD_GTT_SHARE`: a number above 0, at most 0.5 (a larger one is taken as
/// 0.5); 0.4 without the variable or with a value that is not such a number.
pub fn gtt_share_from(value: Option<&str>) -> f64 {
    match value.and_then(|v| v.trim().parse::<f64>().ok()) {
        Some(share) if share.is_finite() && share > 0.0 => share.min(GTT_SHARE_MAX),
        _ => GTT_SHARE,
    }
}

/// The run's budget on this card: half the VRAM and the GTT share ([`gtt_share_from`], 0.4 by default) of the GTT.
pub fn default_caps(gpu: &GpuMem) -> Caps {
    let share = gtt_share_from(std::env::var("NV_GUARD_GTT_SHARE").ok().as_deref());
    caps_with(gpu, share)
}

/// The budget with a given GTT share.
fn caps_with(gpu: &GpuMem, gtt_share: f64) -> Caps {
    Caps {
        vram_mib: (gpu.vram_total * VRAM_SHARE) as u64,
        gtt_mib: (gpu.gtt_total * gtt_share) as u64,
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
    Ok(start_check(sysfs_root, halt_file)?.map(|g| g.caps))
}

/// The halt file: fixed under the home folder, with no override (a halt is not skipped by pointing a run at another file).
pub fn halt_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".cache/nv-rs/bench.halt"))
        .unwrap_or_default()
}

/// What a start's guard knows: the caps, the card's use when the start was checked, and where to read the card again.
#[derive(Clone, Debug, PartialEq)]
pub struct Guarded {
    pub caps: Caps,
    pub base: GpuMem,
    pub dir: PathBuf,
}

/// [`check`] that keeps what the sampler needs.
pub fn start_check(sysfs_root: &Path, halt_file: &Path) -> Result<Option<Guarded>, String> {
    if let Some(reason) = halted(halt_file) {
        return Err(format!(
            "bench runs are halted: {reason} (`nv-rs-scene guard clear-halt` removes {})",
            halt_file.display()
        ));
    }
    let Some(dir) = find_gpu(sysfs_root) else {
        return Ok(None);
    };
    let Some(base) = read_gpu(&dir) else {
        return Ok(None);
    };
    let caps = default_caps(&base);
    preflight(&caps, &base)?;
    Ok(Some(Guarded { caps, base, dir }))
}

/// The guard stops a run at this share of the way to a limit, so it stops before the limit, not after.
pub const STOP_AT: f64 = 0.9;
/// Seconds between two readings of the card during a run.
pub const SAMPLE_S: f64 = 0.1;

/// The use (MiB, the whole card) at which the sampler stops a run: [`STOP_AT`] of the way to the lower of the run's cap over
/// the use at the start and the card's total less the margin kept for the desktop. VRAM first, GTT second.
pub fn stop_at(caps: &Caps, base: &GpuMem) -> (f64, f64) {
    let one = |used0: f64, cap: u64, total: f64, margin: f64| {
        let ceiling = (used0 + cap as f64).min(total - margin);
        used0 + (ceiling - used0).max(0.0) * STOP_AT
    };
    (
        one(
            base.vram_used,
            caps.vram_mib,
            base.vram_total,
            VRAM_MARGIN_MIB,
        ),
        one(base.gtt_used, caps.gtt_mib, base.gtt_total, GTT_MARGIN_MIB),
    )
}

/// Why the run must stop now, or `None`.
pub fn over(caps: &Caps, base: &GpuMem, now: &GpuMem) -> Option<String> {
    let (lv, lg) = stop_at(caps, base);
    if now.vram_used >= lv {
        return Some(format!(
            "VRAM {:.0} MiB reached the guard's stop at {lv:.0} (cap {} over {:.0} at start)",
            now.vram_used, caps.vram_mib, base.vram_used
        ));
    }
    if now.gtt_used >= lg {
        return Some(format!(
            "GTT {:.0} MiB reached the guard's stop at {lg:.0} (cap {} over {:.0} at start)",
            now.gtt_used, caps.gtt_mib, base.gtt_used
        ));
    }
    None
}

/// The engine's own refusal of an allocation past its `NV_GPU_BUDGET_MB`, as its log says it.
pub const ENGINE_REFUSAL: &str = "the layer's video-memory budget is used up";

/// The desktop's compositors (process names as `/proc/PID/comm` has them) whose loss means the session is in trouble.
pub const DESKTOP_NAMES: [&str; 5] = [
    "gnome-shell",
    ".gnome-shell-wr",
    "kwin_wayland",
    "kwin_wayland_wr",
    "mutter",
];

/// Kernel log phrases (on an `amdgpu` or `[drm]` line) that mean the GPU or the session is in trouble.
pub const KERNEL_TROUBLE: [&str; 8] = [
    "not enough memory for command submission",
    "gpu reset",
    "gpu_recover",
    "amdgpu_job_timedout",
    "gpu recovery",
    "device lost",
    "vram is lost",
    "ring ",
];

/// The pids of the desktop's compositors running now (by their process name), read from `proc` (`/proc`).
pub fn desktop_pids(proc_dir: &Path) -> Vec<u32> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir(proc_dir) else {
        return out;
    };
    for e in dir.flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        if let Ok(name) = std::fs::read_to_string(e.path().join("comm")) {
            if DESKTOP_NAMES.contains(&name.trim()) {
                out.push(pid);
            }
        }
    }
    out.sort_unstable();
    out
}

/// The kernel log since a time (Unix seconds); empty when `journalctl` cannot read it.
pub fn kernel_lines_since(since_unix: u64) -> String {
    std::process::Command::new("journalctl")
        .args(["-k", "-q", "-o", "cat", "--no-pager", "--since"])
        .arg(format!("@{since_unix}"))
        .stderr(std::process::Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// Why the host session is in trouble, or `None`: a GPU reset or an out-of-memory submission in the kernel log, or a
/// desktop compositor that ran at the start and is gone (a pid in `own` is the run's, not the desktop's).
pub fn session_trouble(
    kernel_text: &str,
    desktop_at_start: &[u32],
    desktop_now: &[u32],
    own: &[u32],
) -> Option<String> {
    for line in kernel_text.lines() {
        if !line.contains("amdgpu") && !line.contains("[drm]") {
            continue;
        }
        let low = line.to_ascii_lowercase();
        if let Some(m) = KERNEL_TROUBLE.iter().find(|m| low.contains(*m)) {
            // `ring ... timeout` is the one phrase of two words apart.
            if *m == "ring " && !low.contains("timeout") {
                continue;
            }
            return Some(format!("kernel: {}", m.trim()));
        }
    }
    let gone: Vec<String> = desktop_at_start
        .iter()
        .filter(|p| !desktop_now.contains(p) && !own.contains(p))
        .map(u32::to_string)
        .collect();
    if gone.is_empty() {
        None
    } else {
        Some(format!(
            "the desktop's compositor is gone (pid {})",
            gone.join(", ")
        ))
    }
}

/// Writes the halt file (whole or not at all): every later run is refused until [`clear_halt`].
pub fn halt(path: &Path, reason: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(
        &tmp,
        format!(
            "{{\"time\": \"unix {secs}\", \"reason\": \"{}\"}}",
            reason.replace('\\', "/").replace('"', "'")
        ),
    )?;
    std::fs::rename(&tmp, path)
}

/// Removes the halt file; whether there was one.
pub fn clear_halt(path: &Path) -> bool {
    std::fs::remove_file(path).is_ok()
}

/// The sampler of one run: the card read every [`SAMPLE_S`] seconds by the caller, the session and the engine's own
/// refusal every second. [`Watch::step`] says why the run must stop, if it must.
pub struct Watch {
    pub guarded: Guarded,
    desktop_at_start: Vec<u32>,
    started_unix: u64,
    last_health: Option<std::time::Instant>,
    log_offset: u64,
    /// The highest card use seen: VRAM and GTT in MiB.
    pub peak: (f64, f64),
}

impl Watch {
    pub fn new(guarded: Guarded) -> Watch {
        let peak = (guarded.base.vram_used, guarded.base.gtt_used);
        Watch {
            guarded,
            desktop_at_start: desktop_pids(Path::new("/proc")),
            started_unix: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            last_health: None,
            log_offset: 0,
            peak,
        }
    }

    /// One sample: the reason to stop, or `None`. `engine_log` is the engine's output file (its refusal is looked for in
    /// what was added since the last call, once a second); `own` are pids of the run that are not the desktop's.
    pub fn step(&mut self, engine_log: &Path, own: &[u32]) -> Option<String> {
        let Some(now) = read_gpu(&self.guarded.dir) else {
            return Some("the card's memory can no longer be read: the run is not guarded".into());
        };
        self.peak = (
            self.peak.0.max(now.vram_used),
            self.peak.1.max(now.gtt_used),
        );
        if let Some(why) = over(&self.guarded.caps, &self.guarded.base, &now) {
            return Some(why);
        }
        if self
            .last_health
            .is_some_and(|t| t.elapsed().as_secs_f64() < 1.0)
        {
            return None;
        }
        self.last_health = Some(std::time::Instant::now());
        let kernel = kernel_lines_since(self.started_unix);
        if let Some(t) = session_trouble(
            &kernel,
            &self.desktop_at_start,
            &desktop_pids(Path::new("/proc")),
            own,
        ) {
            return Some(format!("session trouble: {t}"));
        }
        if let Some(text) = new_text(engine_log, &mut self.log_offset) {
            if text.contains(ENGINE_REFUSAL) {
                return Some(
                    "the engine refused an allocation past its GPU budget (NV_GPU_BUDGET_MB)"
                        .into(),
                );
            }
        }
        None
    }
}

/// What was added to a file since `offset` (moved on), or `None`.
fn new_text(path: &Path, offset: &mut u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    f.seek(SeekFrom::Start(*offset)).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    *offset += buf.len() as u64;
    Some(String::from_utf8_lossy(&buf).into_owned())
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
    fn the_card_with_the_most_vram_is_read_and_its_budget_is_half_and_four_tenths() {
        let t = tmp("find");
        card(&t, "card0", (100, 512), (10, 1024));
        card(&t, "card1", (3000, 24576), (500, 32768));
        std::fs::create_dir_all(t.join("renderD128")).unwrap();
        let gpu = read_gpu(&find_gpu(&t).unwrap()).unwrap();
        assert_eq!((gpu.vram_used, gpu.vram_total), (3000.0, 24576.0));
        assert_eq!(
            caps_with(&gpu, GTT_SHARE),
            Caps {
                vram_mib: 12288,
                gtt_mib: 13107
            }
        );
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn the_gtt_share_is_four_tenths_unless_set_and_never_above_a_half() {
        assert_eq!(gtt_share_from(None), 0.4);
        assert_eq!(gtt_share_from(Some("0.3")), 0.3);
        assert_eq!(gtt_share_from(Some(" 0.45 ")), 0.45);
        assert_eq!(gtt_share_from(Some("0.9")), 0.5, "clamped");
        for bad in ["", "x", "0", "-0.2", "NaN", "inf"] {
            assert_eq!(gtt_share_from(Some(bad)), 0.4, "{bad:?}");
        }
        let gpu = GpuMem {
            vram_used: 0.0,
            vram_total: 24576.0,
            gtt_used: 0.0,
            gtt_total: 24040.0,
        };
        // The card of this machine: about 9.4 GiB of GTT instead of 5.9.
        assert_eq!(caps_with(&gpu, 0.4).gtt_mib, 9616);
        assert_eq!(
            caps_with(&gpu, 0.4).vram_mib,
            12288,
            "the VRAM share is unchanged"
        );
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
                gtt_mib: (32768.0
                    * gtt_share_from(std::env::var("NV_GUARD_GTT_SHARE").ok().as_deref()))
                    as u64
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

    #[test]
    fn the_sampler_stops_before_the_cap_and_before_the_desktop_margin() {
        let caps = Caps {
            vram_mib: 1000,
            gtt_mib: 500,
        };
        let base = GpuMem {
            vram_used: 2000.0,
            vram_total: 24576.0,
            gtt_used: 100.0,
            gtt_total: 32768.0,
        };
        assert_eq!(stop_at(&caps, &base), (2900.0, 550.0));
        let now = |v: f64, g: f64| GpuMem {
            vram_used: v,
            gtt_used: g,
            ..base
        };
        assert_eq!(over(&caps, &base, &now(2800.0, 400.0)), None);
        assert!(over(&caps, &base, &now(2900.0, 400.0))
            .unwrap()
            .starts_with("VRAM 2900 MiB"));
        assert!(over(&caps, &base, &now(2000.0, 550.0))
            .unwrap()
            .starts_with("GTT 550 MiB"));
        // A card nearly full: the ceiling is the total less the margin, not the cap.
        let full = GpuMem {
            vram_used: 22000.0,
            ..base
        };
        assert_eq!(stop_at(&caps, &full).0, 22000.0 + 528.0 * 0.9);
    }

    #[test]
    fn the_session_is_in_trouble_for_a_gpu_reset_or_a_lost_compositor_but_not_for_the_runs_own_process(
    ) {
        let log = "usb 1-1: new device\namdgpu 0000:03:00.0: amdgpu: GPU reset begin!\n";
        assert_eq!(
            session_trouble(log, &[], &[], &[]).unwrap(),
            "kernel: gpu reset"
        );
        assert!(session_trouble(
            "amdgpu: Not enough memory for command submission",
            &[],
            &[],
            &[]
        )
        .unwrap()
        .contains("memory for command submission"));
        // A line about something else, or without the driver's name, is not trouble.
        assert_eq!(
            session_trouble("amdgpu: ring gfx_0.0.0 is fine", &[], &[], &[]),
            None
        );
        assert_eq!(
            session_trouble("GPU reset of some other device", &[], &[], &[]),
            None
        );
        assert!(
            session_trouble("amdgpu: ring gfx timeout, signaled seq=1", &[], &[], &[]).is_some()
        );
        assert_eq!(session_trouble("", &[10, 20], &[10, 20], &[]), None);
        assert!(session_trouble("", &[10, 20], &[10], &[])
            .unwrap()
            .contains("pid 20"));
        assert_eq!(session_trouble("", &[10, 20], &[10], &[20]), None);
    }

    #[test]
    fn the_halt_file_is_written_whole_read_back_and_cleared() {
        let t = tmp("halting");
        let file = t.join("sub/bench.halt");
        halt(&file, "VRAM 2900 MiB reached \"the\" stop").unwrap();
        let reason = halted(&file).unwrap();
        assert!(reason.starts_with("VRAM 2900 MiB reached"), "{reason}");
        assert!(check(&t, &file).unwrap_err().contains("clear-halt"));
        assert!(clear_halt(&file));
        assert!(!clear_halt(&file));
        assert_eq!(check(&t, &file).unwrap(), None);
        assert!(
            std::fs::read_dir(t.join("sub")).unwrap().next().is_none(),
            "no temporary file is left"
        );
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn a_watch_stops_a_run_when_the_card_fills_or_the_engine_says_its_budget_is_used_up() {
        let t = tmp("watch");
        card(&t, "card0", (3000, 24576), (500, 32768));
        let halt_file = t.join("none");
        let guarded = start_check(&t, &halt_file).unwrap().unwrap();
        let mut w = Watch::new(guarded);
        let log = t.join("run.log");
        assert_eq!(w.step(&log, &[]), None);
        std::fs::write(&log, format!("noise\nstop: {ENGINE_REFUSAL}\n")).unwrap();
        // The first health check was done by the step above; the log is read at the next due one.
        w.last_health = None;
        assert!(w.step(&log, &[]).unwrap().contains("GPU budget"));
        card(&t, "card0", (20000, 24576), (500, 32768));
        assert!(w.step(&log, &[]).unwrap().starts_with("VRAM"));
        assert!(w.peak.0 >= 20000.0);
        std::fs::remove_dir_all(&t).unwrap();
    }
}
