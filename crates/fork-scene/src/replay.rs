// SPDX-License-Identifier: GPL-3.0-only
//! `nv-rs-scene replay` (docs/fork/REPLAY.md): the replay bench of a recorded Direct3D 9 stream. The measurement runs the
//! `d3d9-replay-bench` binary under the exclusive game lock (the one `bench` takes), in a memory-capped scope, with the same
//! guard as every engine start (refused while runs are halted or the card has no room, the card read every 100 ms). The
//! streams are recordings of the game: private, they stay on this machine and never go into a repository.

use crate::tool::{Cap, Say};
use std::path::{Path, PathBuf};

/// The binary's name.
pub const BINARY: &str = "d3d9-replay-bench";

/// Where the recorded streams are kept (`NV_REPLAY_STREAMS`, else `~/.local/share/nv-rs-research/streams`).
pub fn streams_dir() -> PathBuf {
    std::env::var_os("NV_REPLAY_STREAMS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                .join(".local/share/nv-rs-research/streams")
        })
}

/// The built binary (`<target>/release/d3d9-replay-bench`; the target is `NV_DEV_TARGET`, else `~/.cache/nv-rs/target`).
pub fn binary() -> PathBuf {
    let target = std::env::var_os("NV_DEV_TARGET")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache/nv-rs/target")
        });
    target.join("release").join(BINARY)
}

/// The stream a name stands for: the path itself when it exists, else `<name>` or `<name>.nvrs` in the streams folder.
pub fn resolve(name: &str, streams: &Path) -> Option<PathBuf> {
    let p = Path::new(name);
    if p.exists() {
        return Some(p.to_path_buf());
    }
    [streams.join(name), streams.join(format!("{name}.nvrs"))]
        .into_iter()
        .find(|c| c.exists())
}

/// The streams in a folder: name and size in bytes, sorted.
pub fn listing(streams: &Path) -> Vec<(String, u64)> {
    let mut v: Vec<(String, u64)> = std::fs::read_dir(streams)
        .map(|d| {
            d.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "nvrs"))
                .map(|e| {
                    (
                        e.file_name().to_string_lossy().into_owned(),
                        e.metadata().map_or(0, |m| m.len()),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// A measurement.
#[derive(Clone, Debug)]
pub struct ReplayRun {
    pub binary: PathBuf,
    /// The binary's arguments (`run STREAM ...`).
    pub args: Vec<String>,
    /// The exclusive game lock file; `None`: no lock (a run that gives no numbers).
    pub lock: Option<PathBuf>,
    /// Seconds to wait for the lock before giving up with status 3 (`None`: wait for ever).
    pub wait_timeout_s: Option<f64>,
    /// A run is stopped after this many seconds (status 124).
    pub timeout_s: u64,
    pub gpu_sysfs: PathBuf,
    pub cap: Cap,
}

/// Takes the lock (waiting), runs the binary guarded and says `NOISY` when another `nv-viewer` ran. The exit status: the
/// binary's, 3 when the lock was not free in time, 4 when the guard refused, 124 when stopped after the timeout, 5 when
/// the guard stopped it.
pub fn run(req: &ReplayRun, say: Say) -> Result<i32, String> {
    let _held = match &req.lock {
        Some(path) => {
            let f = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(path)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            let start = std::time::Instant::now();
            let mut told = false;
            loop {
                if f.try_lock().is_ok() {
                    break;
                }
                if req
                    .wait_timeout_s
                    .is_some_and(|t| start.elapsed().as_secs_f64() >= t)
                {
                    say("bench lock not free in time");
                    return Ok(3);
                }
                if !told {
                    say(&format!(
                        "waiting for the bench lock {} (another run holds it)",
                        path.display()
                    ));
                    told = true;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            Some(f)
        }
        None => None,
    };
    let halt_file = crate::guard::halt_path();
    let guarded = match crate::guard::start_check(&req.gpu_sysfs, &halt_file) {
        Ok(g) => g,
        Err(e) => {
            say(&format!("refused by the memory guard: {e}"));
            return Ok(4);
        }
    };
    let words = req.cap.words("replay", "replay", &[], 1)?;
    let mut all: Vec<std::ffi::OsString> = words.iter().map(Into::into).collect();
    all.push((&req.binary).into());
    all.extend(req.args.iter().map(Into::into));
    let mut cmd = std::process::Command::new(&all[0]);
    cmd.args(&all[1..]);
    if let Some(g) = &guarded {
        cmd.env("NV_GPU_BUDGET_MB", g.caps.vram_mib.to_string());
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("{}: {e}", all[0].to_string_lossy()))?;
    let mut watch = guarded.map(crate::guard::Watch::new);
    let started = std::time::Instant::now();
    let mut noisy = crate::tool::other_viewers();
    let mut last_noise = std::time::Instant::now();
    let rc = loop {
        if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
            break st.code().unwrap_or(-1);
        }
        if let Some(w) = watch.as_mut() {
            if let Some(why) = w.step(Path::new(""), &[child.id()]) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = crate::guard::halt(&halt_file, &why);
                say(&format!("memory guard: {why}: stopped; later runs are halted (`nv-rs-scene guard clear-halt`)"));
                break 5;
            }
        }
        if started.elapsed().as_secs() >= req.timeout_s {
            say(&format!("the run was stopped after {} s", req.timeout_s));
            let _ = child.kill();
            let _ = child.wait();
            break 124;
        }
        if last_noise.elapsed().as_secs() >= 5 {
            last_noise = std::time::Instant::now();
            noisy = noisy.max(crate::tool::other_viewers());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    if noisy > 0 {
        say(&format!(
            "NOISY: {noisy} other nv-viewer process(es) ran during the measurement"
        ));
    }
    Ok(rc)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn tmp(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fork-scene-replay-{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_stream_is_a_path_or_a_name_in_the_streams_folder() {
        let t = tmp("resolve");
        std::fs::write(t.join("doc-room.nvrs"), "x").unwrap();
        std::fs::write(t.join("note.txt"), "x").unwrap();
        assert_eq!(resolve("doc-room", &t), Some(t.join("doc-room.nvrs")));
        assert_eq!(resolve("doc-room.nvrs", &t), Some(t.join("doc-room.nvrs")));
        assert_eq!(
            resolve(&t.join("note.txt").display().to_string(), &t),
            Some(t.join("note.txt"))
        );
        assert_eq!(resolve("none", &t), None);
        assert_eq!(listing(&t), vec![("doc-room.nvrs".to_string(), 1)]);
        std::fs::remove_dir_all(&t).unwrap();
    }

    fn request(t: &Path, args: &[&str]) -> ReplayRun {
        ReplayRun {
            binary: PathBuf::from("/bin/sh"),
            args: args.iter().map(|s| s.to_string()).collect(),
            lock: Some(t.join("game.lock")),
            wait_timeout_s: Some(0.2),
            timeout_s: 5,
            gpu_sysfs: PathBuf::from("/nonexistent"),
            cap: Cap {
                off: true,
                ..Cap::default()
            },
        }
    }

    #[test]
    fn a_run_returns_the_binarys_status_waits_for_the_lock_and_stops_after_the_timeout() {
        let t = tmp("run");
        let lines = std::sync::Mutex::new(Vec::<String>::new());
        let say = |l: &str| lines.lock().unwrap().push(l.to_string());
        assert_eq!(run(&request(&t, &["-c", "exit 7"]), &say).unwrap(), 7);
        // Another holder of the lock: not free in time, status 3.
        let holder = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(t.join("game.lock"))
            .unwrap();
        holder.lock().unwrap();
        assert_eq!(run(&request(&t, &["-c", "exit 0"]), &say).unwrap(), 3);
        assert!(lines
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.contains("not free in time")));
        drop(holder);
        let mut slow = request(&t, &["-c", "sleep 30"]);
        slow.timeout_s = 1;
        assert_eq!(run(&slow, &say).unwrap(), 124);
        std::fs::remove_dir_all(&t).unwrap();
    }
}
