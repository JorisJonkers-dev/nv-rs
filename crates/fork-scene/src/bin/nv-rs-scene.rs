// SPDX-License-Identifier: GPL-3.0-only
//! `nv-rs-scene compare A B`: the difference between two pictures, or between the same-named PNGs
//! of two folders (a determinism check, or two variants of the same places). Exit 0 when every
//! pair was compared; with `--max-mean N` exit 1 when a pair's mean difference is above N.

use fork_scene::diff::{compare, decode_png, Difference};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// A variant's run: the engine's exit status and the seconds it took, or why it did not start.
type RunResult = Result<(i32, f64), String>;

/// `NAME=VALUE` for `--env`.
fn env_pair(s: &str) -> Result<(String, String), String> {
    s.split_once('=')
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .filter(|(k, _)| !k.is_empty())
        .ok_or(format!("--env wants NAME=VALUE, got {s}"))
}

/// `KEY=VALUE` for `--ini`.
fn ini_pair(s: &str) -> Result<(String, String), String> {
    s.split_once('=')
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .filter(|(k, _)| !k.is_empty())
        .ok_or(format!("--ini wants KEY=VALUE, got {s}"))
}

/// `--grids N`: the loaded square is N by N cells (an odd number, 3 to 21).
fn grids_value(s: &str) -> Result<String, String> {
    match s.parse::<u32>() {
        Ok(n) if n % 2 == 1 && (3..=21).contains(&n) => Ok(n.to_string()),
        _ => Err(format!("--grids is an odd number from 3 to 21, not {s}")),
    }
}

fn load(p: &Path) -> Result<fork_scene::diff::Picture, String> {
    decode_png(&std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?)
        .map_err(|e| format!("{}: {e}", p.display()))
}

fn pngs(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "png"))
        .collect();
    v.sort();
    Ok(v)
}

fn line(name: &str, d: &Difference) -> String {
    format!(
        "{name}: mean {:.3}, max {}, changed {:.3}% of pixels (channel difference above {})",
        d.mean,
        d.max,
        d.changed_pct,
        fork_scene::diff::THRESHOLD
    )
}

fn run(args: &[String]) -> Result<bool, String> {
    let mut paths = Vec::new();
    let mut max_mean = None;
    let mut diff_out: Option<PathBuf> = None;
    let mut noise: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--max-mean" {
            max_mean = Some(
                it.next()
                    .and_then(|v| v.parse::<f64>().ok())
                    .ok_or("--max-mean needs a number")?,
            );
        } else if a == "--noise" {
            noise = Some(PathBuf::from(
                it.next()
                    .ok_or("--noise needs the folder of a second run of A")?,
            ));
        } else if a == "--diff-out" {
            diff_out = Some(PathBuf::from(it.next().ok_or("--diff-out needs a folder")?));
        } else {
            paths.push(PathBuf::from(a));
        }
    }
    let [a, b] = paths.as_slice() else {
        return Err(
            "usage: nv-rs-scene compare [--max-mean N] [--diff-out DIR] [--noise A2] A B  (two PNGs or two folders)".into(),
        );
    };
    let mut ok = true;
    let check = |name: &str, d: Difference, ok: &mut bool| {
        println!("{}", line(name, &d));
        *ok &= max_mean.is_none_or(|m| d.mean <= m);
    };
    if a.is_dir() && b.is_dir() {
        for pa in pngs(a)? {
            let name = pa.file_name().unwrap().to_string_lossy().into_owned();
            let pb = b.join(&name);
            if !pb.exists() {
                println!("{name}: missing in {}", b.display());
                ok = false;
                continue;
            }
            let (pic_a, pic_b) = (load(&pa)?, load(&pb)?);
            if let Some(n) = &noise {
                let again = load(&n.join(&name))?;
                let (d, masked) = fork_scene::diff::compare_without_noise(&pic_a, &again, &pic_b)?;
                check(&format!("{name} (without the {masked:.2}% that differs between the two runs of the first folder)"), d, &mut ok);
                continue;
            }
            check(&name, compare(&pic_a, &pic_b)?, &mut ok);
            if let Some(dir) = &diff_out {
                let (w, h, rgb) = fork_scene::diff::difference_picture(&pic_a, &pic_b)?;
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                let png = fork_scene::png::encode_rgb(w, h, &rgb)?;
                std::fs::write(dir.join(&name), png).map_err(|e| e.to_string())?;
            }
        }
    } else {
        check(
            &a.display().to_string(),
            compare(&load(a)?, &load(b)?)?,
            &mut ok,
        );
    }
    Ok(ok)
}

/// The memory cap of every engine start: each start is a `gamerun` scope (a systemd scope in `nv-games.slice` with a
/// hard `MemoryMax` and no swap, labelled in the monitor) unless `--no-cap` is given or the `--wrap` words already
/// contain `gamerun`. The default limit is a share of a total budget (`TOTAL_BUDGET_MIB`), `--mem` overrides it.
#[derive(Clone, Debug, Default)]
struct Cap {
    /// `--label`: the base of the scope's name (a run adds its variant's name); empty: `scene-<kind>-<pid>`.
    label: String,
    who: String,
    mem: String,
    off: bool,
}

impl Cap {
    /// Takes `--label`, `--who`, `--mem` or `--no-cap` when `a` is one; false when it is not.
    fn take(&mut self, a: &str, it: &mut std::slice::Iter<String>) -> Result<bool, String> {
        match a {
            "--label" => self.label = it.next().cloned().ok_or("--label needs a value")?,
            "--who" => self.who = it.next().cloned().ok_or("--who needs a value")?,
            "--mem" => self.mem = it.next().cloned().ok_or("--mem needs a value (10G)")?,
            "--no-cap" => self.off = true,
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// The words that put a command in the scope, for the start named `name` of a run of `kind`; empty when the
    /// cap is off or the wrap words already hold `gamerun`.
    fn words(
        &self,
        kind: &str,
        name: &str,
        wrap: &[String],
        concurrent: usize,
    ) -> Result<Vec<String>, String> {
        if self.off || wrap.iter().any(|w| w.ends_with("gamerun")) {
            return Ok(Vec::new());
        }
        let home = std::env::var("HOME").unwrap_or_default();
        let gamerun = format!("{home}/.local/bin/gamerun");
        if !Path::new(&gamerun).exists() {
            return Err(format!(
                "{gamerun} is not there: a game run is memory-capped by default; install it or give --no-cap"
            ));
        }
        let clean = |t: &str| {
            t.chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                        c
                    } else {
                        '-'
                    }
                })
                .collect::<String>()
        };
        let label = if self.label.is_empty() {
            format!("scene-{kind}-{}-{}", clean(name), std::process::id())
        } else if name == kind {
            clean(&self.label)
        } else {
            format!("{}-{}", clean(&self.label), clean(name))
        };
        let mut w = vec![gamerun, label, "--who".into()];
        w.push(if self.who.is_empty() {
            "engine".into()
        } else {
            self.who.clone()
        });
        w.push("--mem".into());
        w.push(self.limit(concurrent));
        w.push("--".into());
        Ok(w)
    }

    /// The limit of each start (gamerun's `--mem` value): `--mem` as given, else the total budget shared by the starts
    /// that run together, never below what one start needs.
    fn limit(&self, concurrent: usize) -> String {
        if !self.mem.is_empty() {
            return self.mem.clone();
        }
        let n = concurrent.max(1) as u64;
        format!(
            "{}M",
            (TOTAL_BUDGET_MIB / n).clamp(ONE_START_MIB, ONE_START_MAX_MIB)
        )
    }

    /// The line printed at the start of a run.
    fn describe(&self, wrap: &[String], concurrent: usize) -> String {
        if self.off {
            "memory cap: OFF (--no-cap): the engine can use all the memory of the machine".into()
        } else if wrap.iter().any(|w| w.ends_with("gamerun")) {
            "memory cap: the --wrap words start gamerun, which holds the cap".into()
        } else {
            let per = self.limit(concurrent);
            let how = if self.mem.is_empty() {
                format!(
                    "the budget of {} MiB for the whole invocation shared by {} start(s) at once, between {ONE_START_MIB} and {ONE_START_MAX_MIB} MiB each",
                    TOTAL_BUDGET_MIB,
                    concurrent.max(1)
                )
            } else {
                "from --mem".to_string()
            };
            format!(
                "memory cap: {per} per engine start ({how}; a gamerun scope in nv-games.slice, hard limit, no swap)"
            )
        }
    }
}

/// The default memory budget of one invocation (MiB), shared by the starts that run together: the owner's desktop once
/// failed from about 12 GiB in one process, so no run is allowed more in all by default. `--mem` overrides it per start.
const TOTAL_BUDGET_MIB: u64 = 12288;
/// What one start needs at least (MiB): the cap never goes below it. Measured peaks are about 1.7 GiB (the plain game) and
/// 1.9 GiB (a Tale of Two Wastelands folder), docs/fork/SCENE.md: twice that.
const ONE_START_MIB: u64 = 4096;
/// The most one start gets by default (MiB): three times the measured peak, so a start that grows out of bounds is
/// stopped long before the desktop notices.
const ONE_START_MAX_MIB: u64 = 6144;

/// What every variant's run shares.
struct Opts {
    cap: Cap,
    /// `run`, `soak` or `bench`: the default label of a start.
    kind: String,
    /// Engine starts running at once (`--jobs` for a run, 1 for a soak or bench).
    concurrent: usize,
    viewer: PathBuf,
    game: PathBuf,
    places: PathBuf,
    out: PathBuf,
    userdata: Option<PathBuf>,
    wrap: Vec<String>,
    settle: String,
    timeout: String,
    resolution: String,
    /// The plugin list, in load order; empty: the plugins found at the overlay's top, sorted.
    plugin_list: Vec<String>,
    /// Top-level entries of an overlay left out (such as `meshes`, `textures`, `lodsettings`).
    overlay_skip: Vec<String>,
    extra_env: Vec<(String, String)>,
    /// `--ini KEY=VALUE`: lines of the staged `Fallout.ini` rewritten before the start (`--grids N` is `uGridsToLoad=N`).
    ini: Vec<(String, String)>,
    /// `--gpu-sysfs DIR`: where the card's memory is read for the start check (default `/sys/class/drm`).
    gpu_sysfs: PathBuf,
    /// Seconds the engine's log may stay silent before the start counts as hung and is killed (0: no watch).
    stall_s: u64,
    /// A sweep's report: the rows of the places the engine finishes are appended to it as they appear in the log, so a sweep
    /// that is killed still leaves its record.
    progress: Option<PathBuf>,
}

/// Appends a table row to the sweep report for each place the engine has soaked since `offset` (the log's bytes already
/// read); lines not yet complete are left for the next call.
fn progress_poll(report: &Path, log: &Path, offset: &mut u64) {
    use std::io::{Read, Seek, SeekFrom, Write};
    let Ok(mut f) = std::fs::File::open(log) else {
        return;
    };
    if f.seek(SeekFrom::Start(*offset)).is_err() {
        return;
    }
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        return;
    }
    let Some(end) = buf.iter().rposition(|b| *b == b'\n') else {
        return;
    };
    *offset += end as u64 + 1;
    let mut rows = String::new();
    for line in String::from_utf8_lossy(&buf[..end]).lines() {
        let Ok(v) = fork_scene::json_lite::fields(line, &["message"]) else {
            continue;
        };
        if let Some(r) = v[0].strip_prefix("scene: soak ") {
            let name = r.split_whitespace().next().unwrap_or("");
            rows.push_str(&format!("| {name} | ok: {r} |\n"));
        }
    }
    if !rows.is_empty() {
        if let Ok(mut out) = std::fs::OpenOptions::new().append(true).open(report) {
            let _ = out.write_all(rows.as_bytes());
        }
    }
}

/// The exit status [`run_one`] reports for a start it killed because its log went silent.
const HUNG: i32 = -99;

/// `nv-rs-scene run`: every variant of the game's data on one place file, one engine start each (in
/// parallel up to `--jobs`), then the first variant's pictures against every other's.
fn run_variants(args: &[String]) -> Result<bool, String> {
    let (mut viewer, mut game, mut places, mut out, mut userdata) = (None, None, None, None, None);
    let (mut variants, mut wrap, mut settle, mut timeout, mut jobs) = (
        Vec::new(),
        Vec::new(),
        "120".to_string(),
        "120".to_string(),
        3usize,
    );
    let mut resolution = "1280x720".to_string();
    let mut cap = Cap::default();
    let mut quick = false;
    let mut extra_env: Vec<(String, String)> = Vec::new();
    let mut ini: Vec<(String, String)> = Vec::new();
    let mut gpu_sysfs = PathBuf::from("/sys/class/drm");
    let (mut plugin_list, mut overlay_skip): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |what: &str| it.next().cloned().ok_or(format!("{what} needs a value"));
        match a.as_str() {
            "--viewer" => viewer = Some(PathBuf::from(value(a)?)),
            "--game" => game = Some(PathBuf::from(value(a)?)),
            "--places" => places = Some(PathBuf::from(value(a)?)),
            "--out" => out = Some(PathBuf::from(value(a)?)),
            "--userdata" => userdata = Some(PathBuf::from(value(a)?)),
            "--variant" => variants.push(value(a)?),
            "--wrap" => wrap.push(value(a)?),
            "--settle" => settle = value(a)?,
            "--timeout" => timeout = value(a)?,
            "--resolution" => resolution = value(a)?,
            "--only" => extra_env.push(("NV_SCENE_PLACES".to_string(), value(a)?)),
            "--quick" => quick = true,
            "--env" => extra_env.push(env_pair(&value(a)?)?),
            "--ini" => ini.push(ini_pair(&value(a)?)?),
            "--grids" => ini.push(("uGridsToLoad".into(), grids_value(&value(a)?)?)),
            "--gpu-sysfs" => gpu_sysfs = PathBuf::from(value(a)?),
            "--plugins" => plugin_list = value(a)?.split(',').map(String::from).collect(),
            "--overlay-skip" => overlay_skip = value(a)?.split(',').map(String::from).collect(),
            "--jobs" => jobs = value(a)?.parse().map_err(|_| "--jobs is a number")?,
            other => {
                if !cap.take(other, &mut it)? {
                    return Err(format!("unknown argument {other}"));
                }
            }
        }
    }
    if quick {
        // A quick look: small pictures and a short settle (the picture is not the one to judge; it shows what a change did).
        resolution = "640x360".to_string();
        settle = "100".to_string();
    }
    let usage = "usage: nv-rs-scene run --viewer BIN --game DIR --places FILE --out DIR [--userdata TEMPLATE] [--variant NAME[=OVERLAY]]... [--settle N] [--timeout S] [--resolution WxH] [--only a,b] [--quick] [--grids N] [--ini KEY=VALUE]... [--gpu-sysfs DIR] [--jobs N] [--game-lock FILE --label NAME [--who NAME]] [--wrap WORD]...";
    let (Some(viewer), Some(game), Some(places), Some(out)) = (viewer, game, places, out) else {
        return Err(usage.into());
    };
    if variants.is_empty() {
        variants.push("base".into());
    }
    let variants: Vec<(String, Vec<PathBuf>)> = variants
        .iter()
        .map(|v| match v.split_once('=') {
            Some((n, o)) => (n.to_string(), o.split('+').map(PathBuf::from).collect()),
            None => (v.clone(), Vec::new()),
        })
        .collect();
    let opts = Opts {
        cap,
        kind: "run".into(),
        concurrent: variants.len().min(jobs.max(1)),
        viewer,
        game,
        places,
        out: out.clone(),
        userdata,
        wrap,
        settle,
        timeout,
        resolution,
        plugin_list,
        overlay_skip,
        extra_env,
        ini,
        gpu_sysfs,
        stall_s: 0,
        progress: None,
    };
    println!("{}", opts.cap.describe(&opts.wrap, opts.concurrent));
    let started = std::time::Instant::now();
    let mut results: Vec<(String, RunResult)> = Vec::new();
    for chunk in variants.chunks(jobs.max(1)) {
        std::thread::scope(|scope| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|(name, overlay)| {
                    let opts = &opts;
                    scope.spawn(move || {
                        let t = std::time::Instant::now();
                        let r = run_one(name, overlay, opts);
                        (
                            name.clone(),
                            r.map(|code| (code, t.elapsed().as_secs_f64())),
                        )
                    })
                })
                .collect();
            for h in handles {
                results.push(h.join().expect("a variant's thread"));
            }
        });
    }
    let mut ok = true;
    for (name, r) in &results {
        match r {
            Ok((0, s)) => println!(
                "{name}: ok, {s:.0} s, pictures in {}",
                out.join(name).join("pictures").display()
            ),
            Ok((code, s)) => {
                ok = false;
                println!(
                    "{name}: the engine ended with status {code} after {s:.0} s (see {})",
                    out.join(name).join("run.log").display()
                );
            }
            Err(e) => {
                ok = false;
                println!("{name}: {e}");
            }
        }
    }
    println!("all variants: {:.0} s", started.elapsed().as_secs_f64());
    if ok && variants.len() > 1 {
        let base = out.join(&variants[0].0).join("pictures");
        for (name, _) in &variants[1..] {
            println!("--- {} against {}", name, variants[0].0);
            let other = out.join(name).join("pictures");
            let paths = [base.display().to_string(), other.display().to_string()];
            run(&paths)?;
        }
    }
    Ok(ok)
}

fn run_one(name: &str, overlays: &[PathBuf], o: &Opts) -> Result<i32, String> {
    // The card's part of the memory guard: refused when the runs are halted or the card has no room for the budget.
    let halt_file = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".cache/nv-rs/bench.halt"))
        .unwrap_or_default();
    let gpu = fork_scene::guard::check(&o.gpu_sysfs, &halt_file)
        .map_err(|e| format!("refused by the memory guard: {e}"))?;
    let dir = o.out.join(name);
    // A packed overlay names its plugins: every plugin file at its top.
    let mut plugins = Vec::new();
    for o in overlays {
        for e in std::fs::read_dir(o)
            .map_err(|e| format!("{}: {e}", o.display()))?
            .flatten()
        {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.to_ascii_lowercase().ends_with(".esp") || n.to_ascii_lowercase().ends_with(".esm")
            {
                plugins.push(n);
            }
        }
        plugins.sort();
    }
    if !o.plugin_list.is_empty() {
        plugins = o.plugin_list.clone();
    }
    let staged = fork_scene::stage::stage(
        &o.game,
        overlays.first().map(PathBuf::as_path),
        &o.overlay_skip,
        &plugins,
        o.userdata.as_deref(),
        &dir,
    )
    .map_err(|e| format!("staging: {e}"))?;
    for (key, value) in &o.ini {
        fork_scene::stage::set_ini(&staged, key, value).map_err(|e| format!("staging: {e}"))?;
    }
    // Later overlays (a generated LOD folder over a load order's data folder) are laid over the first, all of
    // their files, their plugins after it.
    for later in overlays.iter().skip(1) {
        fork_scene::stage::merge_over(later, &staged.game.join("Data"), &[])
            .map_err(|e| format!("staging {}: {e}", later.display()))?;
    }
    let log = std::fs::File::create(dir.join("run.log")).map_err(|e| e.to_string())?;
    let mut cmd_words: Vec<std::ffi::OsString> = o.wrap.iter().map(Into::into).collect();
    let cap_words = o.cap.words(&o.kind, name, &o.wrap, o.concurrent)?;
    cmd_words.extend(cap_words.iter().cloned().map(std::ffi::OsString::from));
    cmd_words.push((&o.viewer).into());
    cmd_words.push((&staged.game).into());
    let mut cmd = std::process::Command::new(&cmd_words[0]);
    cmd.args(&cmd_words[1..])
        .current_dir(&staged.game)
        .env("XDG_DATA_HOME", &staged.xdg[0])
        .env("XDG_CONFIG_HOME", &staged.xdg[1])
        .env("XDG_CACHE_HOME", cache_home(&staged.xdg[2]))
        .env("XDG_STATE_HOME", &staged.xdg[3])
        .env("NV_D3D9", "wgpu")
        .env("NV_D3D9_OFFSCREEN", "1")
        .env("NV_SOUND", "out")
        .env("NV_SOUND_BACKEND", "null")
        .env("NV_RESOLUTION", &o.resolution)
        .env("NV_THREADS", "real")
        .env("NV_LOG_JSON", dir.join("log.jsonl"))
        .env("NV_LOG_TERM", "warn")
        .env("NV_SCENE", &o.places)
        .env("NV_SCENE_SETTLE_FRAMES", &o.settle)
        .env("NV_SCENE_TIMEOUT_S", &o.timeout)
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log);
    if let Some(caps) = &gpu {
        cmd.env("NV_GPU_BUDGET_MB", caps.vram_mib.to_string());
    }
    for (k, v) in &o.extra_env {
        cmd.env(k, v);
    }
    if !o.extra_env.iter().any(|(k, _)| k == "NV_SCENE_SOAK_FRAMES") {
        cmd.env("NV_SCENE_OUT", dir.join("pictures"));
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("{}: {e}", cmd_words[0].to_string_lossy()))?;
    // The scope's memory, read while the engine runs (the scope is gone when it ends).
    let scope_prefix = cap_words.get(1).map(|l| format!("nvgame-{l}-"));
    let mut mem = MemSeen::default();
    let log_path = dir.join("log.jsonl");
    let mut progress_at = 0u64;
    let mut progress_clock = std::time::Instant::now();
    let mut last_moved = std::time::Instant::now();
    let mut last_len = 0u64;
    let status = loop {
        if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
            break st;
        }
        if let Some(prefix) = &scope_prefix {
            mem.poll(prefix);
        }
        if let Some(report) = &o.progress {
            if progress_clock.elapsed().as_secs() >= 3 {
                progress_clock = std::time::Instant::now();
                progress_poll(report, &log_path, &mut progress_at);
            }
        }
        if o.stall_s > 0 {
            // A hung engine writes nothing: kill the start (its scope, so the game process goes too) when the log has not
            // grown for the stall limit.
            let len = std::fs::metadata(&log_path).map_or(0, |m| m.len());
            if len != last_len {
                last_len = len;
                last_moved = std::time::Instant::now();
            } else if last_moved.elapsed().as_secs() >= o.stall_s {
                if let Some(scope) = mem.dir.as_ref().and_then(|d| d.file_name()) {
                    let _ = std::process::Command::new("systemctl")
                        .args(["--user", "kill", "--signal=SIGKILL"])
                        .arg(scope)
                        .status();
                }
                let _ = child.kill();
                let _ = child.wait();
                println!(
                    "{name}: hung: the log was silent for {} s, the start was killed",
                    o.stall_s
                );
                return Ok(HUNG);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    };
    if let Some(report) = &o.progress {
        progress_poll(report, &log_path, &mut progress_at);
    }
    if mem.peak > 0 {
        let line = mem.describe(name);
        println!("{line}");
        let _ = std::fs::write(
            dir.join("memory.txt"),
            format!(
                "{line}\npeak_mib {}\nanon_mib {}\n",
                mem.peak >> 20,
                mem.anon >> 20
            ),
        );
    }
    Ok(status.code().unwrap_or(-1))
}

/// The cache folder a start runs with: one folder kept between starts (`~/.cache/nv-rs/scene-cache`), so the drawing layer's
/// pipeline cache and the driver's shader cache are warm from the second start on (1.4 s of the start, the same pictures);
/// the engine writes its pipeline cache file under a temporary name and renames it, so starts at the same time share it.
/// `NV_RS_SCENE_COLD_CACHE=1` gives each start its own empty folder (`own`), as `--env XDG_CACHE_HOME=DIR` gives it DIR.
fn cache_home(own: &Path) -> PathBuf {
    if std::env::var_os("NV_RS_SCENE_COLD_CACHE").is_some() {
        return own.to_path_buf();
    }
    match std::env::var_os("HOME") {
        Some(home) => {
            let dir = PathBuf::from(home).join(".cache/nv-rs/scene-cache");
            if std::fs::create_dir_all(&dir).is_ok() {
                return dir;
            }
            own.to_path_buf()
        }
        None => own.to_path_buf(),
    }
}

/// The highest memory of a scope seen while it ran: the cgroup's `memory.peak` (it counts the page cache the run read,
/// which the kernel gives back under pressure) and the largest anonymous memory (`memory.stat`, what the process itself holds).
#[derive(Default)]
struct MemSeen {
    peak: u64,
    anon: u64,
    dir: Option<PathBuf>,
}

impl MemSeen {
    fn find(prefix: &str) -> Option<PathBuf> {
        let uid = std::fs::read_to_string("/proc/self/loginuid")
            .ok()?
            .trim()
            .to_string();
        let root = PathBuf::from(format!(
            "/sys/fs/cgroup/user.slice/user-{uid}.slice/user@{uid}.service"
        ));
        let mut stack = vec![(root, 0)];
        while let Some((d, depth)) = stack.pop() {
            for e in std::fs::read_dir(&d).ok()?.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.starts_with(prefix) && n.ends_with(".scope") {
                    return Some(e.path());
                }
                if depth < 3 && n.ends_with(".slice") {
                    stack.push((e.path(), depth + 1));
                }
            }
        }
        None
    }

    fn poll(&mut self, prefix: &str) {
        if self.dir.is_none() {
            self.dir = Self::find(prefix);
        }
        let Some(d) = &self.dir else { return };
        let read = |f: &str| std::fs::read_to_string(d.join(f)).ok();
        if let Some(p) = read("memory.peak").and_then(|t| t.trim().parse::<u64>().ok()) {
            self.peak = self.peak.max(p);
        }
        if let Some(a) = read("memory.stat").and_then(|t| {
            t.lines().find_map(|l| {
                l.strip_prefix("anon ")
                    .and_then(|v| v.trim().parse::<u64>().ok())
            })
        }) {
            self.anon = self.anon.max(a);
        }
    }

    fn describe(&self, name: &str) -> String {
        format!(
            "memory {name}: peak {} MiB of the scope (with the page cache the run read), {} MiB anonymous at most",
            self.peak >> 20,
            self.anon >> 20
        )
    }
}

/// `nv-rs-scene soak`: one engine start visits every place of the file and runs `--frames` frames at
/// each; the verdict counts stops, error lines and warnings and prints the frame times of each place.
fn soak(args: &[String]) -> Result<bool, String> {
    Ok(soak_run(args)?.ok)
}

/// What a soak found.
struct SoakResult {
    ok: bool,
    code: i32,
    secs: f64,
    scan: LogScan,
    warmup: u32,
    passes: u32,
}

/// The soak's log: the frame-time rows and the counts the verdict uses.
#[derive(Default)]
struct LogScan {
    rows: Vec<String>,
    errors: u32,
    warnings: u32,
    stops: u32,
    last_place: String,
}

fn scan_log(path: &Path) -> LogScan {
    let log = std::fs::read_to_string(path).unwrap_or_default();
    let mut s = LogScan::default();
    for line in log.lines() {
        let Ok(v) = fork_scene::json_lite::fields(line, &["level", "message"]) else {
            continue;
        };
        let (level, msg) = (&v[0], &v[1]);
        match level.as_str() {
            "error" | "fatal" => s.errors += 1,
            "warn" => s.warnings += 1,
            _ => {}
        }
        if msg.contains("Game's main stopped") {
            s.stops += 1;
        }
        if let Some(r) = msg.strip_prefix("scene: sending the player to ") {
            s.last_place = r.split_whitespace().next().unwrap_or("").to_string();
        }
        if let Some(r) = msg.strip_prefix("scene: soak ") {
            s.rows.push(r.to_string());
        }
    }
    s
}

fn soak_run(args: &[String]) -> Result<SoakResult, String> {
    let (mut viewer, mut game, mut places, mut out, mut userdata) = (None, None, None, None, None);
    let (mut plugin_list, mut overlay_skip): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    let mut overlay: Option<PathBuf> = None;
    let mut extra_env: Vec<(String, String)> = Vec::new();
    let mut ini: Vec<(String, String)> = Vec::new();
    let mut gpu_sysfs = PathBuf::from("/sys/class/drm");
    let (mut frames, mut wrap, mut realtime, mut timeout) =
        ("600".to_string(), Vec::new(), false, "300".to_string());
    let mut exclusive = false;
    let mut stall_s = 0u64;
    let mut progress: Option<PathBuf> = None;
    let mut cap = Cap::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |what: &str| it.next().cloned().ok_or(format!("{what} needs a value"));
        match a.as_str() {
            "--viewer" => viewer = Some(PathBuf::from(value(a)?)),
            "--game" => game = Some(PathBuf::from(value(a)?)),
            "--places" => places = Some(PathBuf::from(value(a)?)),
            "--out" => out = Some(PathBuf::from(value(a)?)),
            "--userdata" => userdata = Some(PathBuf::from(value(a)?)),
            "--progress" => progress = Some(PathBuf::from(value(a)?)),
            "--stall" => {
                stall_s = value(a)?
                    .parse()
                    .map_err(|_| "--stall is a number of seconds".to_string())?
            }
            "--frames" => frames = value(a)?,
            "--warmup" => extra_env.push(("NV_SCENE_SOAK_WARMUP".into(), value(a)?)),
            "--passes" => extra_env.push(("NV_SCENE_SOAK_PASSES".into(), value(a)?)),
            "--env" => extra_env.push(env_pair(&value(a)?)?),
            "--ini" => ini.push(ini_pair(&value(a)?)?),
            "--grids" => ini.push(("uGridsToLoad".into(), grids_value(&value(a)?)?)),
            "--gpu-sysfs" => gpu_sysfs = PathBuf::from(value(a)?),
            "--plugins" => plugin_list = value(a)?.split(',').map(String::from).collect(),
            "--overlay" => overlay = Some(PathBuf::from(value(a)?)),
            "--overlay-skip" => overlay_skip = value(a)?.split(',').map(String::from).collect(),
            "--timeout" => timeout = value(a)?,
            "--wrap" => wrap.push(value(a)?),
            "--realtime" => realtime = true,
            "--exclusive" => exclusive = true,
            other => {
                if !cap.take(other, &mut it)? {
                    return Err(format!("unknown argument {other}"));
                }
            }
        }
    }
    let (Some(viewer), Some(game), Some(places), Some(out)) = (viewer, game, places, out) else {
        return Err("usage: nv-rs-scene soak --viewer BIN --game DIR --places FILE --out DIR [--frames N] [--warmup N] [--passes N] [--timeout S] [--realtime] [--userdata TPL] [--grids N] [--ini KEY=VALUE]... [--wrap WORD]...".into());
    };
    let mut extra = vec![("NV_SCENE_SOAK_FRAMES".to_string(), frames.clone())];
    extra.extend(extra_env);
    if realtime {
        extra.push(("NV_SCENE_REALTIME".into(), "1".into()));
    }
    let t = std::time::Instant::now();
    let opts = Opts {
        cap,
        concurrent: 1,
        kind: if exclusive { "bench" } else { "soak" }.into(),
        viewer,
        game,
        places: places.clone(),
        out: out.clone(),
        userdata,
        wrap,
        settle: "60".into(),
        timeout,
        resolution: "1280x720".into(),
        plugin_list,
        overlay_skip,
        extra_env: extra,
        ini,
        gpu_sysfs,
        stall_s,
        progress,
    };
    println!("{}", opts.cap.describe(&opts.wrap, opts.concurrent));
    let overlays: Vec<PathBuf> = overlay.into_iter().collect();
    let code = run_one("soak", &overlays, &opts)?;
    let secs = t.elapsed().as_secs_f64();
    if !exclusive {
        println!("timing rows not valid, machine shared (a bench is run with `bench`, which takes the lock exclusively)");
    }
    let scan = scan_log(&out.join("soak").join("log.jsonl"));
    let (errors, warnings, stops) = (scan.errors, scan.warnings, scan.stops);
    let (rows, last_place) = (&scan.rows, scan.last_place.clone());
    println!("place, frames, frame times (ms):");
    for r in rows {
        println!("  {r}");
    }
    // `bench.json`: machine-readable numbers per place, for `nv-rs-scene bench-compare`.
    let mut json = String::from("{\"schema\":\"nv-rs-scene-bench/1\",\"places\":[");
    for (i, r) in rows.iter().enumerate() {
        let w: Vec<&str> = r.split_whitespace().collect();
        // "<name> frames N avg_ms X p99_ms Y max_ms Z low1_fps W"
        if w.len() >= 11 {
            if i > 0 {
                json.push(',');
            }
            json.push_str(&format!(
                "{{\"place\":\"{}\",\"frames\":{},\"avg_ms\":{},\"p99_ms\":{},\"max_ms\":{},\"low1_fps\":{},\"passes\":{},\"spread_pct\":{}}}",
                w[0], w[2], w[4], w[6], w[8], w[10],
                w.get(12).unwrap_or(&"1"),
                w.get(14).unwrap_or(&"0")
            ));
        }
    }
    json.push_str(&format!(
        "],\"real_clock\":{realtime},\"exclusive_lock\":{exclusive},\"machine\":{}}}\n",
        machine_notes()
    ));
    let _ = std::fs::write(out.join("soak").join("bench.json"), &json);
    let n_places = match opts.extra_env.iter().find(|(k, _)| k == "NV_SCENE_PLACES") {
        Some((_, names)) => names.split(',').count(),
        None => std::fs::read_to_string(&places).map_or(0, |t| t.matches("[[place]]").count()),
    };
    println!(
        "soak: {} of {n_places} places soaked for {frames} frames, exit status {code}, {stops} stops, {errors} error lines, {warnings} warnings, {secs:.0} s{}",
        rows.len(),
        if code != 0 { format!("; ended at place {last_place}, see {}", out.join("soak").join("run.log").display()) } else { String::new() }
    );
    let ok = code == 0 && stops == 0 && errors == 0 && rows.len() == n_places;
    let (warmup, passes) = (
        env_value(&opts.extra_env, "NV_SCENE_SOAK_WARMUP", 120),
        env_value(&opts.extra_env, "NV_SCENE_SOAK_PASSES", 1),
    );
    Ok(SoakResult {
        ok,
        code,
        secs,
        scan,
        warmup,
        passes,
    })
}

/// `nv-rs-scene sweep`: the stability sweep. Like `soak` (same arguments, `--frames` 300 and `--timeout` 120 unless given,
/// `--stall` 120), but a place that stops, hangs or is put back does not end it: the failure is recorded with the end of the
/// log, and the engine is started again at the places after it. The report is `OUT/sweep.md`; the exit status is 0 only
/// when every place loaded, held its frames and the engine left cleanly.
fn sweep(args: &[String]) -> Result<bool, String> {
    // The sweep runs in a child of its own (`--worker`), watched by this process: a child that is killed (a signal, an
    // out-of-memory kill) or panics is reported here, in the report file and on the last line, with a non-zero status.
    if !args.iter().any(|a| a == "--worker") {
        return sweep_supervised(args);
    }
    sweep_worker(args)
}

/// The signal that ended a child (none off Unix, where a process ends only with a status).
#[cfg(unix)]
fn killed_by(status: &std::process::ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.signal()
}

#[cfg(not(unix))]
fn killed_by(_: &std::process::ExitStatus) -> Option<i32> {
    None
}

/// Runs the sweep in a worker child and says plainly how it ended when it did not end with its summary.
fn sweep_supervised(args: &[String]) -> Result<bool, String> {
    let out = args
        .iter()
        .position(|a| a == "--out")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .ok_or("sweep needs --out DIR")?;
    // `NV_RS_SCENE_WORKER_EXE` names another program to run as the worker (the tests use it to kill one).
    let me = match std::env::var_os("NV_RS_SCENE_WORKER_EXE") {
        Some(p) => PathBuf::from(p),
        None => std::env::current_exe().map_err(|e| e.to_string())?,
    };
    let status = std::process::Command::new(me)
        .arg("sweep")
        .args(args)
        .arg("--worker")
        .status()
        .map_err(|e| format!("the sweep could not start its worker: {e}"))?;
    let finished =
        std::fs::read_to_string(out.join("sweep.md")).is_ok_and(|t| !t.contains("(in progress"));
    match (status.code(), killed_by(&status)) {
        (Some(0), _) if finished => Ok(true),
        (Some(1), _) if finished => Ok(false),
        (code, signal) => {
            let how = match (code, signal) {
                (_, Some(sig)) => format!("killed by signal {sig}"),
                (Some(c), _) => format!("exit status {c}"),
                _ => "an unknown end".to_string(),
            };
            let line = format!(
                "sweep: the sweep ended without its summary ({how}); the places finished before it are in {}",
                out.join("sweep.md").display()
            );
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .append(true)
                .open(out.join("sweep.md"))
            {
                let _ = writeln!(f, "\n{line}");
            }
            Err(line)
        }
    }
}

fn sweep_worker(args: &[String]) -> Result<bool, String> {
    let args: Vec<String> = args.iter().filter(|a| *a != "--worker").cloned().collect();
    let args = &args[..];
    let value_of = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let places_path = PathBuf::from(value_of("--places").ok_or("sweep needs --places FILE")?);
    let out = PathBuf::from(value_of("--out").ok_or("sweep needs --out DIR")?);
    let all: Vec<String> = fork_scene::PlaceFile::parse(
        &std::fs::read_to_string(&places_path)
            .map_err(|e| format!("{}: {e}", places_path.display()))?,
    )?
    .places
    .into_iter()
    .map(|p| p.name)
    .collect();
    // `--only a,b` narrows the sweep, as for `run`.
    let mut remaining: Vec<String> = match value_of("--only") {
        Some(only) => {
            let want: Vec<&str> = only.split(',').collect();
            all.iter()
                .filter(|n| want.contains(&n.as_str()))
                .cloned()
                .collect()
        }
        None => all,
    };
    let mut base: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--out" || a == "--only" {
            it.next();
        } else {
            base.push(a.clone());
        }
    }
    for (flag, default) in [
        ("--frames", "300"),
        ("--timeout", "120"),
        ("--stall", "120"),
    ] {
        if !base.iter().any(|a| a == flag) {
            base.push(flag.into());
            base.push(default.into());
        }
    }
    let total = remaining.len();
    let report = out.join("sweep.md");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    std::fs::write(
        &report,
        format!(
            "# Sweep of {} (in progress: a report without a summary line below the title is one that did not finish)\n\n| place | result |\n| --- | --- |\n",
            places_path.display()
        ),
    )
    .map_err(|e| e.to_string())?;
    base.push("--progress".into());
    base.push(report.display().to_string());
    let mut passed: Vec<(String, String)> = Vec::new();
    let mut failed: Vec<(String, String)> = Vec::new();
    let mut warnings = 0;
    let mut starts = 0;
    let t = std::time::Instant::now();
    while !remaining.is_empty() {
        starts += 1;
        if starts > total + 1 {
            return Err("the sweep started more engines than it has places: stopped".into());
        }
        let mut a = base.clone();
        a.extend([
            "--out".into(),
            out.join(format!("run{starts}")).display().to_string(),
        ]);
        a.extend([
            "--env".into(),
            format!("NV_SCENE_PLACES={}", remaining.join(",")),
        ]);
        let r = soak_run(&a)?;
        warnings += r.scan.warnings;
        let done = r.scan.rows.len().min(remaining.len());
        for (name, row) in remaining.iter().zip(&r.scan.rows) {
            passed.push((name.clone(), row.clone()));
        }
        remaining.drain(..done);
        if r.ok || remaining.is_empty() {
            if !r.ok {
                // Every place held its frames, but the engine did not leave cleanly or logged errors.
                failed.push((
                    "(after the last place)".into(),
                    format!(
                        "exit status {}, {} stops, {} error lines",
                        r.code, r.scan.stops, r.scan.errors
                    ),
                ));
            }
            break;
        }
        let bad = remaining.remove(0);
        let log = out
            .join(format!("run{starts}"))
            .join("soak")
            .join("run.log");
        // What the engine said: its error lines (the log's), else the last lines of its output.
        let said = |path: &Path, keep: &dyn Fn(&str) -> bool, n: usize| -> Vec<String> {
            let mut v: Vec<String> = std::fs::read_to_string(path)
                .unwrap_or_default()
                .lines()
                .filter(|l| keep(l))
                .map(|l| l.chars().take(300).collect())
                .collect();
            let skip = v.len().saturating_sub(n);
            v.drain(..skip);
            v
        };
        let jsonl = std::fs::read_to_string(
            out.join(format!("run{starts}"))
                .join("soak")
                .join("log.jsonl"),
        )
        .unwrap_or_default();
        let mut tail: Vec<String> = jsonl
            .lines()
            .filter_map(|l| fork_scene::json_lite::fields(l, &["level", "message"]).ok())
            .filter(|v| v[0] == "error" || v[0] == "fatal")
            .map(|v| v[1].chars().take(300).collect())
            .collect();
        let skip = tail.len().saturating_sub(3);
        tail.drain(..skip);
        if tail.is_empty() {
            tail = said(&log, &|l| !l.starts_with("hitch:"), 4);
        }
        let why = if r.code == HUNG {
            "hung (the engine's log went silent past the stall limit)".to_string()
        } else {
            format!(
                "exit status {}, {} stops, {} error lines",
                r.code, r.scan.stops, r.scan.errors
            )
        };
        failed.push((
            bad,
            format!(
                "{why}; log {}; last lines: {}",
                log.display(),
                tail.join(" | ")
            ),
        ));
    }
    let mut md = format!(
        "# Sweep of {}\n\n{} of {total} places loaded and held {} frames; {} failed; {warnings} warnings; {starts} engine starts, {:.0} s.\n\n",
        places_path.display(),
        passed.len(),
        value_of("--frames").unwrap_or_else(|| "300".into()),
        failed.len(),
        t.elapsed().as_secs_f64()
    );
    md.push_str("| place | result |\n| --- | --- |\n");
    for (n, row) in &passed {
        md.push_str(&format!("| {n} | ok: {row} |\n"));
    }
    for (n, why) in &failed {
        md.push_str(&format!("| {n} | FAILED: {why} |\n"));
    }
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    std::fs::write(out.join("sweep.md"), &md).map_err(|e| e.to_string())?;
    println!("{md}");
    println!("sweep report: {}", out.join("sweep.md").display());
    Ok(failed.is_empty())
}

fn env_value(env: &[(String, String)], key: &str, default: u32) -> u32 {
    env.iter()
        .rev()
        .find(|(k, _)| k == key)
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(default)
}

/// What the machine was doing, as a JSON object for `bench.json`: the CPU frequency governor, the load average, the CPU
/// model (a bench on a machine that was busy, or on another governor, says so in its own file).
fn machine_fields() -> (String, String, String) {
    let read = |p: &str| {
        std::fs::read_to_string(p)
            .map(|t| t.trim().to_string())
            .unwrap_or_default()
    };
    let governor = read("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor");
    let load = read("/proc/loadavg");
    let load = load
        .split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ");
    let model = read("/proc/cpuinfo")
        .lines()
        .find(|l| l.starts_with("model name"))
        .and_then(|l| l.split_once(':'))
        .map(|(_, v)| v.trim().to_string())
        .unwrap_or_default();
    (governor, load, model)
}

fn machine_notes() -> String {
    let (g, l, c) = machine_fields();
    format!(
        "{{\"governor\":{},\"loadavg\":{},\"cpu\":{}}}",
        fork_scene::bench::esc(&g),
        fork_scene::bench::esc(&l),
        fork_scene::bench::esc(&c)
    )
}

/// The built-in suites: a name and the place file it stands for (the files are in `docs/fork/scenes`).
const SUITES: [(&str, &str); 4] = [
    (
        "quick",
        include_str!("../../../../docs/fork/scenes/bench-quick.toml"),
    ),
    (
        "mojave",
        include_str!("../../../../docs/fork/scenes/mojave-far.toml"),
    ),
    (
        "soak",
        include_str!("../../../../docs/fork/scenes/soak-far-moves.toml"),
    ),
    (
        "capital",
        include_str!("../../../../docs/fork/scenes/ttw-capital.toml"),
    ),
];

/// Other `nv-viewer` processes running now (a bench that shared the machine is marked noisy).
fn other_viewers() -> u32 {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return 0;
    };
    dir.flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .chars()
                .all(|c| c.is_ascii_digit())
        })
        .filter(|e| {
            std::fs::read_to_string(e.path().join("comm"))
                .is_ok_and(|c| c.trim().starts_with("nv-viewer"))
        })
        .count() as u32
}

/// `nv-rs-scene bench`: a soak on the real clock under the exclusive game lock (`--lock FILE`), with the numbers written
/// to `<out>/soak/bench.json` and, in the old harness's shape, `<out>/soak/summary.json` and the suite files:
/// `--suite NAME` takes a built-in place file (`quick`, `mojave`, `soak`, `capital`) instead of `--places`;
/// `--state DIR` (default `~/.cache/nv-rs/scene-bench`) keeps the suites and baselines; `--save-baseline NAME` stores this
/// run's rows, `--baseline NAME` shows the change against a stored one (and the previous suite is always shown).
fn bench(args: &[String]) -> Result<bool, String> {
    let mut rest = Vec::new();
    let (mut lock, mut suite, mut state) = (None, None, None);
    let (mut save_baseline, mut baseline): (Option<String>, Option<String>) = (None, None);
    let mut out_dir = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |what: &str| it.next().cloned().ok_or(format!("{what} needs a value"));
        match a.as_str() {
            "--lock" => lock = Some(value(a)?),
            "--suite" => suite = Some(value(a)?),
            "--state" => state = Some(PathBuf::from(value(a)?)),
            "--save-baseline" => save_baseline = Some(value(a)?),
            "--baseline" => baseline = Some(value(a)?),
            "--out" => {
                let v = value(a)?;
                out_dir = Some(PathBuf::from(&v));
                rest.push(a.clone());
                rest.push(v);
            }
            _ => rest.push(a.clone()),
        }
    }
    let lock = lock.ok_or(
        "bench needs --lock FILE (the shared game lock file: the bench takes it exclusively)",
    )?;
    let suite_name = suite.clone().unwrap_or_else(|| "places".into());
    if let Some(name) = &suite {
        let (_, text) = SUITES.iter().find(|(n, _)| n == name).ok_or_else(|| {
            let known: Vec<&str> = SUITES.iter().map(|(n, _)| *n).collect();
            format!(
                "no suite `{name}` (the built-in ones: {})",
                known.join(", ")
            )
        })?;
        let out = out_dir.clone().ok_or("--out is needed")?;
        std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
        let file = out.join(format!("suite-{name}.toml"));
        std::fs::write(&file, text).map_err(|e| format!("{}: {e}", file.display()))?;
        rest.push("--places".into());
        rest.push(file.display().to_string());
    }
    // The wrap words come first, so the lock is taken before the engine starts and held until it has ended.
    let mut full: Vec<String> = [
        "--realtime",
        "--exclusive",
        "--passes",
        "5",
        "--wrap",
        "flock",
        "--wrap",
        "-x",
        "--wrap",
        &lock,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    full.extend(rest);
    let noisy_before = other_viewers() > 0;
    let r = soak_run(&full)?;
    let out = out_dir.ok_or("--out is needed")?;
    let noisy = noisy_before || other_viewers() > 0;
    // The files of the old harness's shape.
    let name = format!("{suite_name}-{}", stamp());
    let rows: Vec<fork_scene::bench::PlaceRow> = r
        .scan
        .rows
        .iter()
        .filter_map(|l| fork_scene::bench::PlaceRow::parse(l))
        .collect();
    let notes = machine_fields();
    let facts = fork_scene::bench::RunFacts {
        name: name.clone(),
        scenario: suite_name.clone(),
        resolution: "1280x720".into(),
        limiters: fork_scene::bench::limiters_in(|k| std::env::var(k).ok()),
        wall_s: r.secs,
        noisy,
        exit_status: r.code,
        stops: r.scan.stops,
        errors: r.scan.errors,
        warnings: r.scan.warnings,
        governor: notes.0,
        loadavg: notes.1,
        cpu: notes.2,
        warmup: r.warmup,
        passes: r.passes,
    };
    let summary = fork_scene::bench::summary_json(&suite_name, &rows, &facts);
    let write = |p: PathBuf, text: &str| {
        std::fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))
    };
    write(out.join("soak").join("summary.json"), &summary)?;
    let heads: Vec<_> = rows
        .iter()
        .map(|row| fork_scene::bench::headline(&suite_name, row, &facts))
        .collect();
    let state = state.unwrap_or_else(|| {
        PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache/nv-rs/scene-bench")
    });
    let previous = previous_suite(&state, &name);
    let base = baseline.as_ref().and_then(|b| {
        std::fs::read_to_string(state.join("baselines").join(format!("{b}.json")))
            .ok()
            .map(|t| fork_scene::bench::known_from(&t))
    });
    let md = fork_scene::bench::suite_markdown(
        &name,
        &heads,
        previous.as_ref(),
        base.as_ref(),
        baseline.as_deref(),
        "1280x720",
    );
    write(out.join("soak").join("suite.md"), &md)?;
    let suite_dir = state.join(&name);
    std::fs::create_dir_all(&suite_dir).map_err(|e| format!("{}: {e}", suite_dir.display()))?;
    write(
        suite_dir.join("suite.json"),
        &fork_scene::bench::suite_json(&name, &heads),
    )?;
    write(suite_dir.join("suite.md"), &md)?;
    if let Some(b) = &save_baseline {
        let dir = state.join("baselines");
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        write(
            dir.join(format!("{b}.json")),
            &fork_scene::bench::baseline_json(&heads),
        )?;
    }
    println!("{md}");
    println!(
        "summary: {}",
        out.join("soak").join("summary.json").display()
    );
    Ok(r.ok && !heads.iter().any(|h| !h.checks_ok))
}

/// `YYYYMMDDTHHMMSS` (UTC) for a run's name.
fn stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (days, rem) = (secs / 86400, secs % 86400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// The rows of the newest suite in `state` older than `name`, or `None`.
fn previous_suite(state: &Path, name: &str) -> Option<fork_scene::bench::Known> {
    let prefix = name.split('-').next()?.to_string();
    let mut dirs: Vec<String> = std::fs::read_dir(state)
        .ok()?
        .flatten()
        .filter(|e| e.path().join("suite.json").exists())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(&format!("{prefix}-")) && n.as_str() < name)
        .collect();
    dirs.sort();
    let last = dirs.pop()?;
    let text = std::fs::read_to_string(state.join(last).join("suite.json")).ok()?;
    Some(fork_scene::bench::known_from(&text))
}

/// `nv-rs-scene bench-compare A.json B.json`: per place, the average frame time, the 1% low and the largest frame of
/// both and the change from A to B in percent (positive: B is slower).
fn bench_compare(args: &[String]) -> Result<bool, String> {
    let [a, b] = args else {
        return Err("usage: nv-rs-scene bench-compare A.json B.json".into());
    };
    let rows = |p: &String| -> Result<Vec<(String, f64, f64, f64)>, String> {
        let text = std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?;
        let mut out = Vec::new();
        for chunk in text.split("{\"place\":\"").skip(1) {
            let name = chunk.split('"').next().unwrap_or("").to_string();
            let num = |key: &str| -> Result<f64, String> {
                let at = chunk
                    .find(&format!("\"{key}\":"))
                    .ok_or(format!("{p}: no {key} for {name}"))?;
                let rest = &chunk[at + key.len() + 3..];
                let end = rest.find([',', '}']).unwrap_or(rest.len());
                rest[..end]
                    .parse::<f64>()
                    .map_err(|e| format!("{p}: {key}: {e}"))
            };
            out.push((
                name.clone(),
                num("avg_ms")?,
                num("low1_fps")?,
                num("max_ms")?,
            ));
        }
        Ok(out)
    };
    let (ra, rb) = (rows(a)?, rows(b)?);
    println!("place: avg ms A -> B (change), 1% low fps A -> B, worst frame ms A -> B");
    for (name, avg_a, low_a, max_a) in &ra {
        match rb.iter().find(|r| &r.0 == name) {
            Some((_, avg_b, low_b, max_b)) => println!(
                "{name}: {avg_a:.2} -> {avg_b:.2} ({:+.1}%), {low_a:.1} -> {low_b:.1}, {max_a:.1} -> {max_b:.1}",
                100.0 * (avg_b - avg_a) / avg_a.max(1e-9)
            ),
            None => println!("{name}: not in B"),
        }
    }
    Ok(true)
}

/// The arguments of `run`, `soak` and `bench` made ready for the viewer's own start: a relative path is resolved against
/// the current directory (the viewer runs in another one), and `--game-lock FILE` becomes the wrap words that take the
/// shared game lock (`flock -s FILE`); `--label`, `--who`, `--mem` and `--no-cap` are the memory cap's ([`Cap`]).
fn expand_args(args: &[String], cwd: &Path) -> Result<Vec<String>, String> {
    let absolute = |p: &str| {
        let path = Path::new(p);
        if path.is_absolute() {
            p.to_string()
        } else {
            cwd.join(path).display().to_string()
        }
    };
    let (mut out, mut lock) = (Vec::new(), None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |what: &str| it.next().cloned().ok_or(format!("{what} needs a value"));
        match a.as_str() {
            "--viewer" | "--game" | "--places" | "--out" | "--userdata" | "--lock"
            | "--overlay" => {
                out.push(a.clone());
                out.push(absolute(&value(a)?));
            }
            "--variant" => {
                out.push(a.clone());
                let v = value(a)?;
                out.push(match v.split_once('=') {
                    Some((n, o)) => format!(
                        "{n}={}",
                        o.split('+').map(&absolute).collect::<Vec<_>>().join("+")
                    ),
                    None => v,
                });
            }
            "--game-lock" => lock = Some(absolute(&value(a)?)),
            _ => out.push(a.clone()),
        }
    }
    let mut words: Vec<&str> = Vec::new();
    if let Some(l) = &lock {
        words.extend(["flock", "-s", l]);
    }
    for w in words {
        out.push("--wrap".into());
        out.push(w.into());
    }
    Ok(out)
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(
        args.first().map(String::as_str),
        Some("run" | "soak" | "bench" | "sweep")
    ) {
        let cwd = std::env::current_dir().unwrap_or_default();
        match expand_args(&args[1..], &cwd) {
            Ok(rest) => args.splice(1.., rest).for_each(drop),
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::from(2);
            }
        }
    }
    let result = match args.first().map(String::as_str) {
        Some("compare") => run(&args[1..]),
        Some("run") => run_variants(&args[1..]),
        Some("soak") => soak(&args[1..]),
        Some("bench") => bench(&args[1..]),
        Some("sweep") => sweep(&args[1..]),
        Some("bench-compare") => bench_compare(&args[1..]),
        _ => {
            eprintln!("usage: nv-rs-scene compare [--max-mean N] [--diff-out DIR] A B | run --viewer BIN --game DIR --places FILE --out DIR [--variant NAME[=OVERLAY]]...");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fork_scene::png::encode_rgb;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nv-rs-scene-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn picture(dir: &Path, name: &str, rgb: [u8; 3], odd: u8) {
        std::fs::create_dir_all(dir).unwrap();
        let mut px = Vec::new();
        for i in 0..4 {
            let v = if i == 3 { [odd; 3] } else { rgb };
            px.extend_from_slice(&v);
        }
        std::fs::write(dir.join(name), encode_rgb(2, 2, &px).unwrap()).unwrap();
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn env_pairs_need_a_name_and_an_equals_sign() {
        assert_eq!(env_pair("A=b=c").unwrap(), ("A".into(), "b=c".into()));
        assert!(env_pair("novalue").is_err());
        assert!(env_pair("=x").is_err());
    }

    #[test]
    fn compare_of_two_folders_reports_each_picture_and_a_missing_one() {
        let t = tmp("cmp");
        picture(&t.join("a"), "one.png", [10, 10, 10], 10);
        picture(&t.join("a"), "two.png", [10, 10, 10], 10);
        picture(&t.join("b"), "one.png", [10, 10, 10], 200);
        let a = t.join("a").display().to_string();
        let b = t.join("b").display().to_string();
        // two.png is missing in b: not ok.
        assert_eq!(run(&[a.clone(), b.clone()]), Ok(false));
        // Over the mean limit: not ok; the diff pictures are written.
        let d = t.join("d").display().to_string();
        assert_eq!(
            run(&s(&["--max-mean", "0.1", "--diff-out", &d, &a, &b])),
            Ok(false)
        );
        assert!(t.join("d/one.png").exists());
        // Two single files with a generous limit: ok.
        let (fa, fb) = (
            t.join("a/one.png").display().to_string(),
            t.join("b/one.png").display().to_string(),
        );
        assert_eq!(run(&s(&["--max-mean", "100", &fa, &fb])), Ok(true));
        assert!(run(&s(&["--max-mean", "x", &fa, &fb])).is_err());
        assert!(run(&s(&[&fa])).unwrap_err().contains("usage"));
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn compare_with_a_noise_run_leaves_the_unstable_pixel_out() {
        let t = tmp("noise");
        picture(&t.join("a"), "p.png", [10, 10, 10], 10);
        picture(&t.join("a2"), "p.png", [10, 10, 10], 100); // the last pixel is unstable
        picture(&t.join("b"), "p.png", [10, 10, 10], 255);
        let p = |n: &str| t.join(n).display().to_string();
        assert_eq!(run(&s(&["--noise", &p("a2"), &p("a"), &p("b")])), Ok(true));
        assert!(run(&s(&["--noise", &p("nowhere"), &p("a"), &p("b")])).is_err());
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn bench_compare_prints_the_change_and_refuses_bad_files() {
        let t = tmp("bench");
        let row = |p: &str, avg: f64| {
            format!("{{\"place\":\"{p}\",\"frames\":10,\"avg_ms\":{avg},\"p99_ms\":9,\"max_ms\":10,\"low1_fps\":100}}")
        };
        let a = format!("{{\"places\":[{},{}]}}", row("x", 5.0), row("y", 6.0));
        let b = format!("{{\"places\":[{}]}}", row("x", 5.5));
        std::fs::write(t.join("a.json"), a).unwrap();
        std::fs::write(t.join("b.json"), b).unwrap();
        let (pa, pb) = (
            t.join("a.json").display().to_string(),
            t.join("b.json").display().to_string(),
        );
        assert_eq!(bench_compare(&[pa.clone(), pb.clone()]), Ok(true));
        assert!(bench_compare(std::slice::from_ref(&pa)).is_err());
        assert!(bench_compare(&[pa, t.join("none.json").display().to_string()]).is_err());
        std::fs::remove_dir_all(&t).unwrap();
    }

    /// A viewer made of a shell script: it writes the log lines a scene run writes and a picture per place.
    #[cfg(unix)]
    fn fake_viewer(dir: &Path, exit: i32) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let png = dir.join("one.png");
        std::fs::write(&png, encode_rgb(2, 2, &[7; 12]).unwrap()).unwrap();
        let script = dir.join("viewer.sh");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nmkdir -p \"$NV_SCENE_OUT\" 2>/dev/null\n[ -n \"$NV_SCENE_OUT\" ] && cp '{}' \"$NV_SCENE_OUT/a.png\"\n\
                 echo '{{\"level\":\"info\",\"message\":\"scene: sending the player to a from the Menu screen\"}}' >> \"$NV_LOG_JSON\"\n\
                 echo '{{\"level\":\"info\",\"message\":\"scene: soak a frames 10 avg_ms 5.00 p99_ms 6.00 max_ms 7.00 low1_fps 150.0\"}}' >> \"$NV_LOG_JSON\"\n\
                 echo '{{\"level\":\"warn\",\"message\":\"a warning\"}}' >> \"$NV_LOG_JSON\"\nexit {exit}\n",
                png.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        script
    }

    #[cfg(unix)]
    fn install(t: &Path) -> PathBuf {
        let g = t.join("install");
        std::fs::create_dir_all(g.join("Data")).unwrap();
        std::fs::write(g.join("FalloutNV.exe"), "x").unwrap();
        std::fs::write(g.join("Data/FalloutNV.esm"), "x").unwrap();
        let places = t.join("places.toml");
        std::fs::write(
            &places,
            "version = 1\n[[place]]\nname = \"a\"\nworld = \"W\"\nx = 1\ny = 2\nz = 3\n",
        )
        .unwrap();
        g
    }

    #[test]
    fn a_start_is_memory_capped_unless_no_cap_or_gamerun_is_wrapped() {
        let mut cap = Cap::default();
        let mut it = s(&["--label", "pics", "--who", "me", "--mem", "6G"])
            .into_iter()
            .collect::<Vec<_>>();
        let rest = it.split_off(0);
        let mut iter = rest.iter();
        while let Some(a) = iter.next() {
            assert!(cap.take(a, &mut iter).unwrap());
        }
        assert!(!cap.take("--viewer", &mut iter).unwrap());
        assert!(cap.describe(&[], 1).contains("6G"));
        // The default: the budget shared by the starts at once, between the floor and the ceiling.
        let d = Cap::default();
        assert_eq!(d.limit(1), "6144M");
        assert_eq!(d.limit(2), "6144M");
        assert_eq!(d.limit(3), "4096M");
        assert_eq!(d.limit(6), "4096M");
        assert!(d.describe(&[], 3).contains("4096M per engine start"));
        // With the cap off, or gamerun among the wrap words, nothing is added.
        let off = Cap {
            off: true,
            ..Cap::default()
        };
        assert!(off.words("run", "base", &[], 1).unwrap().is_empty());
        assert!(off.describe(&[], 1).contains("OFF"));
        let wrapped = s(&["flock", "/x/gamerun"]);
        assert!(Cap::default()
            .words("run", "base", &wrapped, 1)
            .unwrap()
            .is_empty());
        // Otherwise gamerun comes with the label, the user and the limit (when gamerun is installed).
        if let Ok(w) = cap.words("run", "base", &[], 1) {
            assert!(w[0].ends_with("/.local/bin/gamerun"));
            assert_eq!(&w[1..], ["pics-base", "--who", "me", "--mem", "6G", "--"]);
        }
        if let Ok(w) = cap.words("soak", "soak", &[], 1) {
            assert_eq!(w[1], "pics");
        }
    }

    #[cfg(unix)]
    #[test]
    fn relative_paths_are_resolved_and_the_lock_and_label_become_wrap_words() {
        let a = expand_args(
            &s(&[
                "--places",
                "p.toml",
                "--viewer",
                "/bin/v",
                "--variant",
                "x=o1+/abs",
                "--game-lock",
                "g.lock",
                "--label",
                "lbl",
                "--settle",
                "50",
            ]),
            Path::new("/work"),
        )
        .unwrap();
        assert_eq!(&a[..2], ["--places", "/work/p.toml"]);
        assert_eq!(&a[2..4], ["--viewer", "/bin/v"]);
        assert_eq!(&a[4..6], ["--variant", "x=/work/o1+/abs"]);
        assert_eq!(&a[6..8], ["--label", "lbl"]);
        assert_eq!(&a[8..10], ["--settle", "50"]);
        let wraps: Vec<&str> = a[10..]
            .iter()
            .filter(|w| *w != "--wrap")
            .map(String::as_str)
            .collect();
        assert_eq!(wraps, ["flock", "-s", "/work/g.lock"]);
    }

    #[cfg(unix)]
    #[test]
    fn run_stages_each_variant_starts_the_viewer_and_compares_their_pictures() {
        let t = tmp("run");
        let g = install(&t);
        let overlay = t.join("overlay");
        std::fs::create_dir_all(overlay.join("meshes")).unwrap();
        std::fs::write(overlay.join("Lod.esp"), "x").unwrap();
        let viewer = fake_viewer(&t, 0);
        let args = s(&[
            "--no-cap",
            "--gpu-sysfs",
            "/nonexistent",
            "--viewer",
            &viewer.display().to_string(),
            "--game",
            &g.display().to_string(),
            "--places",
            &t.join("places.toml").display().to_string(),
            "--out",
            &t.join("out").display().to_string(),
            "--quick",
            "--only",
            "a",
            "--env",
            "NV_X=1",
            "--plugins",
            "FalloutNV.esm,Lod.esp",
            "--variant",
            "base",
            "--variant",
            &format!("lod={}", overlay.display()),
        ]);
        assert_eq!(run_variants(&args), Ok(true));
        assert!(
            t.join("out/base/pictures/a.png").exists() && t.join("out/lod/pictures/a.png").exists()
        );
        let list = t.join("out/lod/xdg/data/nv-rs/userdata/AppData/Local/FalloutNV/plugins.txt");
        assert_eq!(
            std::fs::read_to_string(list).unwrap(),
            "FalloutNV.esm\nLod.esp\n"
        );
        assert!(run_variants(&s(&["--viewer"])).is_err());
        assert!(run_variants(&s(&["--bogus"])).is_err());
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn ini_options_and_the_grid_shorthand_are_checked() {
        assert_eq!(
            ini_pair("uGridsToLoad = 7").unwrap(),
            ("uGridsToLoad".into(), "7".into())
        );
        assert!(ini_pair("noequals").is_err());
        assert!(ini_pair("=1").is_err());
        assert_eq!(grids_value("9").unwrap(), "9");
        for bad in ["4", "1", "23", "x", "-3"] {
            assert!(grids_value(bad).is_err(), "{bad}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_failing_viewer_makes_the_run_fail_and_a_missing_one_is_an_error() {
        let t = tmp("fail");
        let g = install(&t);
        let common = |viewer: &Path| {
            s(&[
                "--no-cap",
                "--gpu-sysfs",
                "/nonexistent",
                "--viewer",
                &viewer.display().to_string(),
                "--game",
                &g.display().to_string(),
                "--places",
                &t.join("places.toml").display().to_string(),
                "--out",
                &t.join("o").display().to_string(),
                "--variant",
                "v",
            ])
        };
        assert_eq!(run_variants(&common(&fake_viewer(&t, 70))), Ok(false));
        let _ = std::fs::remove_dir_all(t.join("o"));
        assert_eq!(run_variants(&common(&t.join("no-such-viewer"))), Ok(false));
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn soak_and_bench_read_the_log_count_and_write_bench_json() {
        let t = tmp("soak");
        let g = install(&t);
        let viewer = fake_viewer(&t, 0);
        let args = |out: &str| {
            s(&[
                "--no-cap",
                "--gpu-sysfs",
                "/nonexistent",
                "--viewer",
                &viewer.display().to_string(),
                "--game",
                &g.display().to_string(),
                "--places",
                &t.join("places.toml").display().to_string(),
                "--out",
                out,
                "--frames",
                "10",
            ])
        };
        let out = t.join("soakout").display().to_string();
        assert_eq!(soak(&args(&out)), Ok(true));
        let json = std::fs::read_to_string(t.join("soakout/soak/bench.json")).unwrap();
        assert!(
            json.contains("\"place\":\"a\"") && json.contains("\"low1_fps\":150.0"),
            "{json}"
        );
        // bench: needs a lock file; the fake `flock` is the real one when present, so only the argument checks run here.
        assert!(bench(&args(&t.join("b").display().to_string()))
            .unwrap_err()
            .contains("--lock"));
        assert!(soak(&s(&["--bogus"])).is_err());
        assert!(soak(&s(&["--frames", "5"])).is_err());
        std::fs::remove_dir_all(&t).unwrap();
    }

    /// A viewer that soaks every place it is given except `bad`, where it logs an error and ends with status 70.
    #[cfg(unix)]
    fn picky_viewer(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let script = dir.join("picky.sh");
        std::fs::write(
            &script,
            r#"#!/bin/sh
for p in $(echo "$NV_SCENE_PLACES" | tr ',' ' '); do
  echo '{"level":"info","message":"scene: sending the player to '$p' from the Menu screen"}' >> "$NV_LOG_JSON"
  if [ "$p" = bad ]; then
    echo '{"level":"error","message":"scene: place bad: no cell"}' >> "$NV_LOG_JSON"
    exit 70
  fi
  echo '{"level":"info","message":"scene: soak '$p' frames 10 avg_ms 5.00 p99_ms 6.00 max_ms 7.00 low1_fps 150.0"}' >> "$NV_LOG_JSON"
done
exit 0
"#,
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        script
    }

    #[cfg(unix)]
    #[test]
    fn a_sweep_records_a_failing_place_and_goes_on_from_the_next() {
        let t = tmp("sweep");
        let g = install(&t);
        let places = t.join("sweep.toml");
        let place =
            |n: &str| format!("[[place]]\nname = \"{n}\"\nworld = \"W\"\nx = 1\ny = 2\nz = 3\n");
        std::fs::write(
            &places,
            format!(
                "version = 1\n{}{}{}{}",
                place("a"),
                place("bad"),
                place("c"),
                place("d")
            ),
        )
        .unwrap();
        let viewer = picky_viewer(&t);
        let out = t.join("out");
        let args = s(&[
            "--no-cap",
            "--gpu-sysfs",
            "/nonexistent",
            "--viewer",
            &viewer.display().to_string(),
            "--game",
            &g.display().to_string(),
            "--places",
            &places.display().to_string(),
            "--out",
            &out.display().to_string(),
            "--frames",
            "10",
        ]);
        assert_eq!(sweep_worker(&args), Ok(false));
        let md = std::fs::read_to_string(out.join("sweep.md")).unwrap();
        assert!(md.contains("3 of 4 places loaded"), "{md}");
        assert!(md.contains("2 engine starts"), "{md}");
        for ok in ["| a | ok", "| c | ok", "| d | ok"] {
            assert!(md.contains(ok), "{ok} in {md}");
        }
        assert!(
            md.contains("| bad | FAILED: exit status 70") && md.contains("no cell"),
            "{md}"
        );
        // --only narrows the list; all good places: the sweep passes.
        let mut only = args.clone();
        only.extend(s(&["--only", "a,c"]));
        let out2 = t.join("out2").display().to_string();
        let pos = only.iter().position(|x| x == "--out").unwrap();
        only[pos + 1] = out2;
        assert_eq!(sweep_worker(&only), Ok(true));
        assert!(sweep_worker(&s(&["--out", "x"])).is_err());
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_sweep_killed_in_its_worker_says_so_and_leaves_its_rows() {
        use std::os::unix::fs::PermissionsExt;
        let t = tmp("sweepkill");
        let out = t.join("out");
        std::fs::create_dir_all(&out).unwrap();
        // The worker writes an unfinished report with one row, then dies of SIGKILL.
        let worker = t.join("worker.sh");
        std::fs::write(
            &worker,
            format!(
                "#!/bin/sh\nprintf '# Sweep of x (in progress: a\\n| a | ok: a |\\n' > '{}'\nkill -9 $$\n",
                out.join("sweep.md").display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::env::set_var("NV_RS_SCENE_WORKER_EXE", &worker);
        let r = sweep(&s(&["--out", &out.display().to_string()]));
        std::env::remove_var("NV_RS_SCENE_WORKER_EXE");
        let e = r.unwrap_err();
        assert!(
            e.contains("killed by signal 9") && e.contains("without its summary"),
            "{e}"
        );
        let md = std::fs::read_to_string(out.join("sweep.md")).unwrap();
        assert!(
            md.contains("| a | ok: a |") && md.contains("killed by signal 9"),
            "{md}"
        );
        std::fs::remove_dir_all(&t).unwrap();
    }
}
