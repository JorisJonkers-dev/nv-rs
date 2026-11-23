// SPDX-License-Identifier: GPL-3.0-only
//! `nv-rs-scene`: the command-line face of the scene tool. It reads the arguments into the typed requests of
//! `fork_scene::tool` (the library, docs/fork/SCENE.md "Using this from another program"), calls the library, and prints
//! what the library returns. `compare A B` exits 0 when every pair was compared; with `--max-mean N` it exits 1 when a
//! pair's mean difference is above N. Every command exits 2 for a request it cannot carry out.

use fork_scene::tool::{
    self, BenchCompareRequest, BenchRequest, Cap, CappedLauncher, CompareRequest, LightRequest,
    RunRequest, SoakRequest, Start, SweepRequest, Variant,
};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The live lines of a command go to the terminal.
fn say(line: &str) {
    println!("{line}");
}

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

fn number<T: std::str::FromStr>(flag: &str, v: &str) -> Result<T, String> {
    v.parse()
        .map_err(|_| format!("{flag} is a number, not {v}"))
}

fn run(args: &[String]) -> Result<bool, String> {
    let mut paths = Vec::new();
    let mut req = CompareRequest::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--max-mean" {
            req.max_mean = Some(
                it.next()
                    .and_then(|v| v.parse::<f64>().ok())
                    .ok_or("--max-mean needs a number")?,
            );
        } else if a == "--noise" {
            req.noise = Some(PathBuf::from(
                it.next()
                    .ok_or("--noise needs the folder of a second run of A")?,
            ));
        } else if a == "--diff-out" {
            req.diff_out = Some(PathBuf::from(it.next().ok_or("--diff-out needs a folder")?));
        } else {
            paths.push(PathBuf::from(a));
        }
    }
    let [a, b] = paths.as_slice() else {
        return Err(
            "usage: nv-rs-scene compare [--max-mean N] [--diff-out DIR] [--noise A2] A B  (two PNGs or two folders)".into(),
        );
    };
    (req.a, req.b) = (a.clone(), b.clone());
    let result = tool::compare(&req)?;
    println!("{}", result.summary());
    Ok(result.ok)
}

/// `light [--within PCT] A B`: the light on the ground of B's pictures against A's (exit 1 when the mean sunlit ratio is
/// more than PCT per cent from 1, a picture is missing, or one has POINTS more near-black pixels).
fn light(args: &[String]) -> Result<bool, String> {
    let mut paths = Vec::new();
    let mut req = LightRequest::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--within" {
            req.within_pct = Some(number(
                "--within",
                it.next().ok_or("--within needs a number")?,
            )?);
        } else if a == "--max-black-gain" {
            req.max_black_gain = Some(number(
                "--max-black-gain",
                it.next().ok_or("--max-black-gain needs a number")?,
            )?);
        } else {
            paths.push(PathBuf::from(a));
        }
    }
    let [a, b] = paths.as_slice() else {
        return Err(
            "usage: nv-rs-scene light [--within PCT] [--max-black-gain POINTS] A B  (two folders of pictures)".into(),
        );
    };
    (req.a, req.b) = (a.clone(), b.clone());
    let result = tool::light(&req)?;
    println!("{}", result.summary());
    Ok(result.ok)
}

/// Takes `--label`, `--who`, `--mem` or `--no-cap` when `a` is one; false when it is not.
fn take_cap(cap: &mut Cap, a: &str, it: &mut std::slice::Iter<String>) -> Result<bool, String> {
    match a {
        "--label" => cap.label = it.next().cloned().ok_or("--label needs a value")?,
        "--who" => cap.who = it.next().cloned().ok_or("--who needs a value")?,
        "--mem" => cap.mem = it.next().cloned().ok_or("--mem needs a value (10G)")?,
        "--no-cap" => cap.off = true,
        _ => return Ok(false),
    }
    Ok(true)
}

/// The flags every command that starts an engine takes.
fn take_common(
    a: &str,
    it: &mut std::slice::Iter<String>,
    start: &mut Start,
    launcher: &mut CappedLauncher,
) -> Result<bool, String> {
    let mut value = |what: &str| it.next().cloned().ok_or(format!("{what} needs a value"));
    match a {
        "--viewer" => start.viewer = PathBuf::from(value(a)?),
        "--game" => start.game = PathBuf::from(value(a)?),
        "--places" => start.places = PathBuf::from(value(a)?),
        "--out" => start.out = PathBuf::from(value(a)?),
        "--userdata" => start.userdata = Some(PathBuf::from(value(a)?)),
        "--only" => start.only = value(a)?.split(',').map(String::from).collect(),
        "--env" => start.env.push(env_pair(&value(a)?)?),
        "--ini" => start.ini.push(ini_pair(&value(a)?)?),
        "--grids" => start
            .ini
            .push(("uGridsToLoad".into(), grids_value(&value(a)?)?)),
        "--gpu-sysfs" => start.gpu_sysfs = PathBuf::from(value(a)?),
        "--plugins" => start.plugins = value(a)?.split(',').map(String::from).collect(),
        "--overlay-skip" => start.overlay_skip = value(a)?.split(',').map(String::from).collect(),
        "--timeout" => start.timeout_s = number(a, &value(a)?)?,
        "--game-lock" => start.game_lock = Some(PathBuf::from(value(a)?)),
        "--wrap" => launcher.wrap.push(value(a)?),
        _ => return take_cap(&mut launcher.cap, a, it),
    }
    Ok(true)
}

/// The request's required paths are given.
fn complete(start: &Start, usage: &str) -> Result<(), String> {
    let missing = [&start.viewer, &start.game, &start.places, &start.out]
        .iter()
        .any(|p| p.as_os_str().is_empty());
    if missing {
        return Err(usage.into());
    }
    Ok(())
}

fn empty_start() -> Start {
    Start::new(
        PathBuf::new(),
        PathBuf::new(),
        PathBuf::new(),
        PathBuf::new(),
    )
}

fn run_variants(args: &[String]) -> Result<bool, String> {
    let mut req = RunRequest::new(empty_start());
    let mut launcher = CappedLauncher::default();
    let mut variants: Vec<String> = Vec::new();
    let mut variant_env: Vec<(String, (String, String))> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |what: &str| it.next().cloned().ok_or(format!("{what} needs a value"));
        match a.as_str() {
            "--variant" => variants.push(value(a)?),
            "--variant-env" => {
                let v = value(a)?;
                let (name, pair) = v
                    .split_once(':')
                    .ok_or(format!("--variant-env {v}: write NAME:KEY=VALUE"))?;
                variant_env.push((name.to_string(), env_pair(pair)?));
            }
            "--settle" => req.settle_frames = number(a, &value(a)?)?,
            "--resolution" => req.resolution = value(a)?,
            "--quick" => req.quick = true,
            "--jobs" => req.jobs = number(a, &value(a)?)?,
            other => {
                if !take_common(other, &mut it, &mut req.start, &mut launcher)? {
                    return Err(format!("unknown argument {other}"));
                }
            }
        }
    }
    let usage = "usage: nv-rs-scene run --viewer BIN --game DIR --places FILE --out DIR [--userdata TEMPLATE] [--variant NAME[=OVERLAY]]... [--variant-env NAME:KEY=VALUE]... [--settle N] [--timeout S] [--resolution WxH] [--only a,b] [--quick] [--grids N] [--ini KEY=VALUE]... [--gpu-sysfs DIR] [--jobs N] [--game-lock FILE --label NAME [--who NAME]] [--wrap WORD]...";
    complete(&req.start, usage)?;
    req.variants = variants
        .iter()
        .map(|v| match v.split_once('=') {
            Some((n, o)) => Variant {
                name: n.to_string(),
                overlays: o.split('+').map(PathBuf::from).collect(),
                env: Vec::new(),
            },
            None => Variant {
                name: v.clone(),
                overlays: Vec::new(),
                env: Vec::new(),
            },
        })
        .collect();
    for (name, pair) in variant_env {
        let v = req
            .variants
            .iter_mut()
            .find(|v| v.name == name)
            .ok_or(format!("--variant-env {name}:...: no --variant {name}"))?;
        v.env.push(pair);
    }
    let result = tool::run(&req, &launcher, &say)?;
    print!("{}", result.summary());
    Ok(result.ok)
}

/// The soak flags, on top of `req` (the defaults of the command).
fn parse_soak(
    args: &[String],
    mut req: SoakRequest,
    launcher: &mut CappedLauncher,
) -> Result<SoakRequest, String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |what: &str| it.next().cloned().ok_or(format!("{what} needs a value"));
        match a.as_str() {
            "--progress" => req.progress = Some(PathBuf::from(value(a)?)),
            "--stall" => req.stall_s = number(a, &value(a)?)?,
            "--frames" => req.frames = number(a, &value(a)?)?,
            "--warmup" => req.warmup = Some(number(a, &value(a)?)?),
            "--passes" => req.passes = Some(number(a, &value(a)?)?),
            "--overlay" => req.overlay = Some(PathBuf::from(value(a)?)),
            "--realtime" => req.realtime = true,
            "--exclusive" => req.exclusive = true,
            other => {
                if !take_common(other, &mut it, &mut req.start, launcher)? {
                    return Err(format!("unknown argument {other}"));
                }
            }
        }
    }
    Ok(req)
}

const SOAK_USAGE: &str = "usage: nv-rs-scene soak --viewer BIN --game DIR --places FILE --out DIR [--frames N] [--warmup N] [--passes N] [--timeout S] [--realtime] [--userdata TPL] [--grids N] [--ini KEY=VALUE]... [--wrap WORD]...";

fn soak(args: &[String]) -> Result<bool, String> {
    let mut launcher = CappedLauncher::default();
    let req = parse_soak(args, SoakRequest::new(empty_start()), &mut launcher)?;
    complete(&req.start, SOAK_USAGE)?;
    let result = tool::soak(&req, &launcher, &say)?;
    print!("{}", result.summary());
    Ok(result.ok)
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
    let mut launcher = CappedLauncher::default();
    let req = SweepRequest {
        soak: parse_soak(&args, SweepRequest::new(empty_start()).soak, &mut launcher)?,
    };
    if req.soak.start.places.as_os_str().is_empty() {
        return Err("sweep needs --places FILE".into());
    }
    if req.soak.start.out.as_os_str().is_empty() {
        return Err("sweep needs --out DIR".into());
    }
    let result = tool::sweep(&req, &launcher, &say)?;
    println!("{}", result.markdown);
    println!("sweep report: {}", result.report.display());
    Ok(result.ok)
}

fn bench(args: &[String]) -> Result<bool, String> {
    let mut launcher = CappedLauncher::default();
    let mut req = BenchRequest::new(empty_start(), PathBuf::new());
    let rest: Vec<String> = {
        // The bench's own flags are taken out; the others are the soak's.
        let mut rest = Vec::new();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            let mut value = |what: &str| it.next().cloned().ok_or(format!("{what} needs a value"));
            match a.as_str() {
                "--lock" => req.lock = PathBuf::from(value(a)?),
                "--suite" => req.suite = Some(value(a)?),
                "--state" => req.state = Some(PathBuf::from(value(a)?)),
                "--save-baseline" => req.save_baseline = Some(value(a)?),
                "--baseline" => req.baseline = Some(value(a)?),
                _ => rest.push(a.clone()),
            }
        }
        rest
    };
    if req.lock.as_os_str().is_empty() {
        return Err(
            "bench needs --lock FILE (the shared game lock file: the bench takes it exclusively)"
                .into(),
        );
    }
    req.soak = parse_soak(&rest, req.soak, &mut launcher)?;
    if req.suite.is_some() {
        if req.soak.start.out.as_os_str().is_empty() {
            return Err("--out is needed".into());
        }
    } else {
        complete(&req.soak.start, SOAK_USAGE)?;
    }
    let result = tool::bench(&req, &launcher, &say)?;
    println!("{}", result.markdown);
    println!("summary: {}", result.summary.display());
    Ok(result.ok)
}

/// `nv-rs-scene guard status | clear-halt`: the halt file the memory guard writes when it stops a run.
fn guard_command(args: &[String]) -> Result<bool, String> {
    let file = fork_scene::guard::halt_path();
    match args.first().map(String::as_str) {
        Some("status") => {
            match fork_scene::guard::halted(&file) {
                Some(reason) => println!("runs are halted: {reason} ({})", file.display()),
                None => println!("runs are not halted"),
            }
            Ok(true)
        }
        Some("clear-halt") => {
            println!(
                "{}",
                if fork_scene::guard::clear_halt(&file) {
                    "the halt is cleared"
                } else {
                    "runs were not halted"
                }
            );
            Ok(true)
        }
        _ => Err("usage: nv-rs-scene guard status | clear-halt".into()),
    }
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

fn dev_target() -> PathBuf {
    std::env::var_os("NV_DEV_TARGET")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".cache/nv-rs/target"))
}

/// `nv-rs-scene matrix --ref R ... [--baseline R] [--suite quick] --game DIR --lock FILE [--repo DIR] [--out DIR]
/// [--no-test] [--no-bench]`: builds, tests and benches many refs and compares each with the baseline.
fn matrix_command(args: &[String]) -> Result<bool, String> {
    let mut launcher = CappedLauncher::default();
    let mut req = fork_scene::matrix::MatrixRequest {
        repo: std::env::current_dir().unwrap_or_default(),
        refs: Vec::new(),
        baseline: None,
        suite: "quick".into(),
        game: PathBuf::new(),
        lock: PathBuf::new(),
        out: home().join(".local/state/nv-rs/matrix").join(tool::stamp()),
        bin_cache: home().join(".cache/nv-rs/matrix-bin"),
        target: dev_target(),
        test: true,
        bench: true,
        userdata: None,
        gpu_sysfs: PathBuf::from("/sys/class/drm"),
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |what: &str| it.next().cloned().ok_or(format!("{what} needs a value"));
        match a.as_str() {
            "--ref" => req.refs.push(value(a)?),
            "--baseline" => req.baseline = Some(value(a)?),
            "--suite" => req.suite = value(a)?,
            "--game" => req.game = PathBuf::from(value(a)?),
            "--lock" => req.lock = PathBuf::from(value(a)?),
            "--repo" => req.repo = PathBuf::from(value(a)?),
            "--out" => req.out = PathBuf::from(value(a)?),
            "--userdata" => req.userdata = Some(PathBuf::from(value(a)?)),
            "--gpu-sysfs" => req.gpu_sysfs = PathBuf::from(value(a)?),
            "--no-test" => req.test = false,
            "--no-bench" => req.bench = false,
            other => {
                if !take_cap(&mut launcher.cap, other, &mut it)? {
                    return Err(format!("unknown argument {other}"));
                }
            }
        }
    }
    if req.bench && (req.game.as_os_str().is_empty() || req.lock.as_os_str().is_empty()) {
        return Err("usage: nv-rs-scene matrix --ref R [--ref R]... [--baseline R] [--suite quick|mojave|soak|capital] --game DIR --lock FILE [--repo DIR] [--out DIR] [--no-test] [--no-bench]".into());
    }
    let result = fork_scene::matrix::matrix(&req, &launcher, &say)?;
    println!("{}", result.markdown);
    println!("report: {}", result.dir.join("matrix.md").display());
    Ok(result.ok)
}

/// `nv-rs-scene clean [--all] [--yes] [--repo DIR]`: lists what it would remove with sizes; `--yes` removes it.
fn clean_command(args: &[String]) -> Result<bool, String> {
    let mut repo = std::env::current_dir().unwrap_or_default();
    let (mut all, mut apply) = (false, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--all" => all = true,
            "--yes" => apply = true,
            "--repo" => repo = PathBuf::from(it.next().ok_or("--repo needs a value")?),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let lines = fork_scene::matrix::clean(
        &repo,
        &home().join(".cache/nv-rs/matrix-bin"),
        &dev_target(),
        &home().join(".local/state/nv-rs/matrix"),
        all,
        apply,
    )?;
    for l in &lines {
        println!("{}{l}", if apply { "removed " } else { "would remove " });
    }
    if lines.is_empty() {
        println!("nothing to remove");
    } else if !apply {
        println!("nothing was removed: give --yes to remove these");
    }
    Ok(true)
}

/// `nv-rs-scene replay streams | info|pack|synth ARGS | run STREAM [ARGS] [--lock FILE] [--wait-timeout S] [--no-lock]`.
fn replay_command(args: &[String]) -> Result<bool, String> {
    use fork_scene::replay as r;
    let usage = "usage: nv-rs-scene replay streams | info|pack|synth ARGS | run STREAM [ARGS] [--lock FILE] [--wait-timeout SECONDS] [--no-lock] [--label NAME] [--no-cap]";
    let (cmd, rest) = args.split_first().ok_or(usage)?;
    if cmd == "streams" {
        for (name, size) in r::listing(&r::streams_dir()) {
            println!("{name}\t{size} bytes");
        }
        return Ok(true);
    }
    if !["info", "pack", "synth", "run"].contains(&cmd.as_str()) {
        return Err(usage.into());
    }
    let mut cap = Cap::default();
    let (mut lock, mut wait, mut use_lock, mut passed) = (
        home().join("Games/Fallout New Vegas RS/game.lock"),
        None,
        true,
        Vec::new(),
    );
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--lock" if cmd == "run" => {
                lock = PathBuf::from(it.next().ok_or("--lock needs a value")?)
            }
            "--wait-timeout" if cmd == "run" => {
                wait = Some(number(
                    "--wait-timeout",
                    it.next().ok_or("--wait-timeout needs a value")?,
                )?)
            }
            "--no-lock" if cmd == "run" => use_lock = false,
            "--label" | "--who" | "--mem" | "--no-cap" if cmd == "run" => {
                take_cap(&mut cap, a, &mut it)?;
            }
            _ => passed.push(a.clone()),
        }
    }
    if matches!(cmd.as_str(), "info" | "run") {
        if let Some(first) = passed.first_mut() {
            let found = r::resolve(first, &r::streams_dir()).ok_or(format!(
                "no stream {first:?} (path, or a name in {})",
                r::streams_dir().display()
            ))?;
            *first = found.display().to_string();
        }
    }
    let binary = r::binary();
    if !binary.exists() {
        return Err(format!(
            "no {}: run `nv-rs-dev replay build`",
            binary.display()
        ));
    }
    if cmd != "run" {
        let status = std::process::Command::new(&binary)
            .arg(cmd)
            .args(&passed)
            .status()
            .map_err(|e| e.to_string())?;
        return Ok(status.success());
    }
    passed.insert(0, "run".into());
    let code = r::run(
        &r::ReplayRun {
            binary,
            args: passed,
            lock: use_lock.then_some(lock),
            wait_timeout_s: wait,
            timeout_s: 300,
            gpu_sysfs: PathBuf::from("/sys/class/drm"),
            cap,
        },
        &say,
    )?;
    Ok(code == 0)
}

/// `nv-rs-scene smoke-report LOG RC SECONDS`: the checklist of `nv-rs-dev smoke`.
fn smoke_report(args: &[String]) -> Result<bool, String> {
    let [log, rc, secs] = args else {
        return Err("usage: nv-rs-scene smoke-report LOG RC SECONDS".into());
    };
    let text = String::from_utf8_lossy(&std::fs::read(log).map_err(|e| format!("{log}: {e}"))?)
        .into_owned();
    let (out, ok) = fork_scene::smoke::report(&text, log, rc, secs);
    print!("{out}");
    Ok(ok)
}

fn bench_compare(args: &[String]) -> Result<bool, String> {
    let [a, b] = args else {
        return Err("usage: nv-rs-scene bench-compare A.json B.json".into());
    };
    let result = tool::bench_compare(&BenchCompareRequest {
        a: PathBuf::from(a),
        b: PathBuf::from(b),
    })?;
    print!("{}", result.summary());
    Ok(true)
}

/// The arguments of `run`, `soak` and `bench` made ready for the viewer's own start: a relative path is resolved against
/// the current directory (the viewer runs in another one); `--game-lock FILE` names the shared game lock the starts hold
/// (`flock -s FILE`); `--label`, `--who`, `--mem` and `--no-cap` are the memory cap's ([`Cap`]).
fn expand_args(args: &[String], cwd: &Path) -> Result<Vec<String>, String> {
    let absolute = |p: &str| {
        let path = Path::new(p);
        if path.is_absolute() {
            p.to_string()
        } else {
            cwd.join(path).display().to_string()
        }
    };
    let mut out = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |what: &str| it.next().cloned().ok_or(format!("{what} needs a value"));
        match a.as_str() {
            "--viewer" | "--game" | "--places" | "--out" | "--userdata" | "--lock"
            | "--overlay" | "--game-lock" => {
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
            _ => out.push(a.clone()),
        }
    }
    Ok(out)
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(
        args.first().map(String::as_str),
        Some("run" | "soak" | "bench" | "sweep" | "matrix")
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
        Some("light") => light(&args[1..]),
        Some("run") => run_variants(&args[1..]),
        Some("soak") => soak(&args[1..]),
        Some("bench") => bench(&args[1..]),
        Some("sweep") => sweep(&args[1..]),
        Some("bench-compare") => bench_compare(&args[1..]),
        Some("guard") => guard_command(&args[1..]),
        Some("matrix") => matrix_command(&args[1..]),
        Some("smoke-report") => smoke_report(&args[1..]),
        Some("clean") => clean_command(&args[1..]),
        Some("replay") => replay_command(&args[1..]),
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
    fn light_compares_the_ground_of_two_folders_within_a_limit() {
        let t = tmp("light");
        picture(&t.join("a"), "day.png", [100, 100, 100], 100);
        picture(&t.join("same"), "day.png", [100, 100, 100], 100);
        picture(&t.join("bright"), "day.png", [140, 140, 140], 140);
        let (a, same, bright) = (
            t.join("a").display().to_string(),
            t.join("same").display().to_string(),
            t.join("bright").display().to_string(),
        );
        assert_eq!(light(&s(&["--within", "5", &a, &same])), Ok(true));
        assert_eq!(light(&s(&["--within", "5", &a, &bright])), Ok(false));
        // Without a limit only a missing picture fails.
        assert_eq!(light(&[a.clone(), bright.clone()]), Ok(true));
        picture(&t.join("a"), "other.png", [100, 100, 100], 100);
        assert_eq!(light(&[a.clone(), same]), Ok(false));
        assert!(light(&s(&["--within", "x", &a, &bright])).is_err());
    }

    /// nv-rs#772: a picture gone near black fails, though its mean sunlit light is in the limit.
    #[test]
    fn light_fails_a_picture_that_has_gone_near_black() {
        let t = tmp("black");
        let write = |dir: &str, px: [[u8; 3]; 4]| {
            std::fs::create_dir_all(t.join(dir)).unwrap();
            let rgb: Vec<u8> = px.concat();
            std::fs::write(t.join(dir).join("day.png"), encode_rgb(2, 2, &rgb).unwrap()).unwrap();
        };
        write("a", [[60; 3], [60; 3], [200; 3], [200; 3]]);
        write("dark", [[10; 3], [10; 3], [200; 3], [200; 3]]);
        write("read", [[40; 3], [40; 3], [200; 3], [200; 3]]);
        let p = |n: &str| t.join(n).display().to_string();
        assert_eq!(
            light(&s(&["--max-black-gain", "10", &p("a"), &p("dark")])),
            Ok(false)
        );
        assert_eq!(
            light(&s(&["--max-black-gain", "10", &p("a"), &p("read")])),
            Ok(true)
        );
        assert_eq!(light(&[p("a"), p("dark")]), Ok(true));
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
                 [ -n \"$NV_SCENE_OUT\" ] && echo \"$NV_GFX_PRESET\" > \"$NV_SCENE_OUT/../gfx.txt\"\n\
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
            assert!(take_cap(&mut cap, a, &mut iter).unwrap());
        }
        assert!(!take_cap(&mut cap, "--viewer", &mut iter).unwrap());
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
        // The lock stays a flag of the request, made absolute (no wrap words are added).
        assert_eq!(&a[6..8], ["--game-lock", "/work/g.lock"]);
        assert_eq!(&a[8..10], ["--label", "lbl"]);
        assert_eq!(&a[10..12], ["--settle", "50"]);
        assert_eq!(a.len(), 12);
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
            "--variant-env",
            "lod:NV_GFX_PRESET=modern",
        ]);
        assert_eq!(run_variants(&args), Ok(true));
        // A variant's own environment reaches its start only.
        let gfx = |v: &str| std::fs::read_to_string(t.join(format!("out/{v}/gfx.txt"))).unwrap();
        assert_eq!(
            (
                gfx("base").trim().to_string(),
                gfx("lod").trim().to_string()
            ),
            (String::new(), "modern".to_string())
        );
        let mut bad = args.clone();
        bad.extend(s(&["--variant-env", "nope:A=1"]));
        assert!(run_variants(&bad)
            .unwrap_err()
            .contains("no --variant nope"));
        let mut bad = args.clone();
        bad.extend(s(&["--variant-env", "lod"]));
        assert!(run_variants(&bad).unwrap_err().contains("NAME:KEY=VALUE"));
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

    #[test]
    fn the_guard_smoke_matrix_clean_and_replay_commands_refuse_bad_requests_and_report() {
        assert_eq!(guard_command(&s(&["status"])), Ok(true));
        assert!(guard_command(&s(&["bogus"])).unwrap_err().contains("usage"));
        assert!(guard_command(&[]).is_err());
        let t = tmp("cmds");
        let log = t.join("run.log");
        std::fs::write(&log, "is real window\n").unwrap();
        let l = log.display().to_string();
        assert_eq!(smoke_report(&s(&[&l, "0", "5"])), Ok(false));
        assert!(smoke_report(&s(&[&l])).unwrap_err().contains("usage"));
        assert!(smoke_report(&s(&[&t.join("none").display().to_string(), "0", "5"])).is_err());
        assert!(matrix_command(&s(&["--ref", "a"]))
            .unwrap_err()
            .contains("usage"));
        assert!(matrix_command(&s(&["--bogus"]))
            .unwrap_err()
            .contains("unknown"));
        assert!(clean_command(&s(&["--bogus"]))
            .unwrap_err()
            .contains("unknown"));
        assert!(clean_command(&s(&["--repo", &t.display().to_string()])).is_err());
        assert!(replay_command(&[]).unwrap_err().contains("usage"));
        assert!(replay_command(&s(&["bogus"]))
            .unwrap_err()
            .contains("usage"));
        assert_eq!(replay_command(&s(&["streams"])), Ok(true));
        std::fs::remove_dir_all(&t).unwrap();
    }
}
