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

/// What every variant's run shares.
struct Opts {
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
}

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
    let mut quick = false;
    let mut extra_env: Vec<(String, String)> = Vec::new();
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
            "--plugins" => plugin_list = value(a)?.split(',').map(String::from).collect(),
            "--overlay-skip" => overlay_skip = value(a)?.split(',').map(String::from).collect(),
            "--jobs" => jobs = value(a)?.parse().map_err(|_| "--jobs is a number")?,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if quick {
        // A quick look: small pictures and a short settle (the picture is not the one to judge; it shows what a change did).
        resolution = "640x360".to_string();
        settle = "100".to_string();
    }
    let usage = "usage: nv-rs-scene run --viewer BIN --game DIR --places FILE --out DIR [--userdata TEMPLATE] [--variant NAME[=OVERLAY]]... [--settle N] [--timeout S] [--resolution WxH] [--only a,b] [--quick] [--jobs N] [--game-lock FILE --label NAME [--who NAME]] [--wrap WORD]...";
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
    };
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
    // Later overlays (a generated LOD folder over a load order's data folder) are laid over the first, all of
    // their files, their plugins after it.
    for later in overlays.iter().skip(1) {
        fork_scene::stage::merge_over(later, &staged.game.join("Data"), &[])
            .map_err(|e| format!("staging {}: {e}", later.display()))?;
    }
    let log = std::fs::File::create(dir.join("run.log")).map_err(|e| e.to_string())?;
    let mut cmd_words: Vec<std::ffi::OsString> = o.wrap.iter().map(Into::into).collect();
    cmd_words.push((&o.viewer).into());
    cmd_words.push((&staged.game).into());
    let mut cmd = std::process::Command::new(&cmd_words[0]);
    cmd.args(&cmd_words[1..])
        .current_dir(&staged.game)
        .env("XDG_DATA_HOME", &staged.xdg[0])
        .env("XDG_CONFIG_HOME", &staged.xdg[1])
        .env("XDG_CACHE_HOME", &staged.xdg[2])
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
    for (k, v) in &o.extra_env {
        cmd.env(k, v);
    }
    if !o.extra_env.iter().any(|(k, _)| k == "NV_SCENE_SOAK_FRAMES") {
        cmd.env("NV_SCENE_OUT", dir.join("pictures"));
    }
    let status = cmd
        .status()
        .map_err(|e| format!("{}: {e}", cmd_words[0].to_string_lossy()))?;
    Ok(status.code().unwrap_or(-1))
}

/// `nv-rs-scene soak`: one engine start visits every place of the file and runs `--frames` frames at
/// each; the verdict counts stops, error lines and warnings and prints the frame times of each place.
fn soak(args: &[String]) -> Result<bool, String> {
    let (mut viewer, mut game, mut places, mut out, mut userdata) = (None, None, None, None, None);
    let (mut plugin_list, mut overlay_skip): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    let mut overlay: Option<PathBuf> = None;
    let mut extra_env: Vec<(String, String)> = Vec::new();
    let (mut frames, mut wrap, mut realtime, mut timeout) =
        ("600".to_string(), Vec::new(), false, "300".to_string());
    let mut exclusive = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |what: &str| it.next().cloned().ok_or(format!("{what} needs a value"));
        match a.as_str() {
            "--viewer" => viewer = Some(PathBuf::from(value(a)?)),
            "--game" => game = Some(PathBuf::from(value(a)?)),
            "--places" => places = Some(PathBuf::from(value(a)?)),
            "--out" => out = Some(PathBuf::from(value(a)?)),
            "--userdata" => userdata = Some(PathBuf::from(value(a)?)),
            "--frames" => frames = value(a)?,
            "--warmup" => extra_env.push(("NV_SCENE_SOAK_WARMUP".into(), value(a)?)),
            "--passes" => extra_env.push(("NV_SCENE_SOAK_PASSES".into(), value(a)?)),
            "--env" => extra_env.push(env_pair(&value(a)?)?),
            "--plugins" => plugin_list = value(a)?.split(',').map(String::from).collect(),
            "--overlay" => overlay = Some(PathBuf::from(value(a)?)),
            "--overlay-skip" => overlay_skip = value(a)?.split(',').map(String::from).collect(),
            "--timeout" => timeout = value(a)?,
            "--wrap" => wrap.push(value(a)?),
            "--realtime" => realtime = true,
            "--exclusive" => exclusive = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let (Some(viewer), Some(game), Some(places), Some(out)) = (viewer, game, places, out) else {
        return Err("usage: nv-rs-scene soak --viewer BIN --game DIR --places FILE --out DIR [--frames N] [--warmup N] [--passes N] [--timeout S] [--realtime] [--userdata TPL] [--wrap WORD]...".into());
    };
    let mut extra = vec![("NV_SCENE_SOAK_FRAMES".to_string(), frames.clone())];
    extra.extend(extra_env);
    if realtime {
        extra.push(("NV_SCENE_REALTIME".into(), "1".into()));
    }
    let t = std::time::Instant::now();
    let opts = Opts {
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
    };
    let overlays: Vec<PathBuf> = overlay.into_iter().collect();
    let code = run_one("soak", &overlays, &opts)?;
    let secs = t.elapsed().as_secs_f64();
    if !exclusive {
        println!("timing rows not valid, machine shared (a bench is run with `bench`, which takes the lock exclusively)");
    }
    let log = std::fs::read_to_string(out.join("soak").join("log.jsonl")).unwrap_or_default();
    let (mut errors, mut warnings, mut stops) = (0, 0, 0);
    let mut rows = Vec::new();
    let mut last_place = String::new();
    for line in log.lines() {
        let Ok(v) = fork_scene::json_lite::fields(line, &["level", "message"]) else {
            continue;
        };
        let (level, msg) = (&v[0], &v[1]);
        match level.as_str() {
            "error" | "fatal" => errors += 1,
            "warn" => warnings += 1,
            _ => {}
        }
        if msg.contains("Game's main stopped") {
            stops += 1;
        }
        if let Some(r) = msg.strip_prefix("scene: sending the player to ") {
            last_place = r.split_whitespace().next().unwrap_or("").to_string();
        }
        if let Some(r) = msg.strip_prefix("scene: soak ") {
            rows.push(r.to_string());
        }
    }
    println!("place, frames, frame times (ms):");
    for r in &rows {
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
    let n_places = std::fs::read_to_string(&places).map_or(0, |t| t.matches("[[place]]").count());
    println!(
        "soak: {} of {n_places} places soaked for {frames} frames, exit status {code}, {stops} stops, {errors} error lines, {warnings} warnings, {secs:.0} s{}",
        rows.len(),
        if code != 0 { format!("; ended at place {last_place}, see {}", out.join("soak").join("run.log").display()) } else { String::new() }
    );
    Ok(code == 0 && stops == 0 && errors == 0 && rows.len() == n_places)
}

/// What the machine was doing, as a JSON object for `bench.json`: the CPU frequency governor, the load average, the CPU
/// model (a bench on a machine that was busy, or on another governor, says so in its own file).
fn machine_notes() -> String {
    let read = |p: &str| {
        std::fs::read_to_string(p)
            .map(|t| t.trim().to_string())
            .unwrap_or_default()
    };
    let esc = |t: String| t.replace('\\', "\\\\").replace('"', "\\\"");
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
    format!(
        "{{\"governor\":\"{}\",\"loadavg\":\"{}\",\"cpu\":\"{}\"}}",
        esc(governor),
        esc(load),
        esc(model)
    )
}

/// `nv-rs-scene bench`: a soak on the real clock under the exclusive game lock (`--lock FILE`), with the numbers written
/// to `<out>/soak/bench.json`. Takes the soak's arguments, plus `--lock`.
fn bench(args: &[String]) -> Result<bool, String> {
    let mut rest = Vec::new();
    let mut lock = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--lock" {
            lock = Some(it.next().ok_or("--lock needs the game.lock file")?.clone());
        } else {
            rest.push(a.clone());
        }
    }
    let lock = lock.ok_or(
        "bench needs --lock FILE (the shared game lock file: the bench takes it exclusively)",
    )?;
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
    soak(&full)
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
/// the current directory (the viewer runs in another one), and `--game-lock FILE`, `--label NAME` and `--who NAME`
/// become the wrap words that take the shared game lock and label the start (`flock -s FILE gamerun NAME --who WHO --`).
fn expand_args(args: &[String], cwd: &Path) -> Result<Vec<String>, String> {
    let absolute = |p: &str| {
        let path = Path::new(p);
        if path.is_absolute() {
            p.to_string()
        } else {
            cwd.join(path).display().to_string()
        }
    };
    let (mut out, mut lock, mut label, mut who) = (Vec::new(), None, None, None);
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
            "--label" => label = Some(value(a)?),
            "--who" => who = Some(value(a)?),
            _ => out.push(a.clone()),
        }
    }
    if who.is_some() && label.is_none() {
        return Err("--who needs --label".into());
    }
    let mut words: Vec<&str> = Vec::new();
    if let Some(l) = &lock {
        words.extend(["flock", "-s", l]);
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let gamerun = format!("{home}/.local/bin/gamerun");
    if let Some(l) = &label {
        words.extend([
            gamerun.as_str(),
            l,
            "--who",
            who.as_deref().unwrap_or("engine"),
            "--",
        ]);
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
        Some("run" | "soak" | "bench")
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
        assert_eq!(&a[6..8], ["--settle", "50"]);
        let wraps: Vec<&str> = a[8..]
            .iter()
            .filter(|w| *w != "--wrap")
            .map(String::as_str)
            .collect();
        assert_eq!(&wraps[..3], ["flock", "-s", "/work/g.lock"]);
        assert!(wraps[3].ends_with("/.local/bin/gamerun"));
        assert_eq!(&wraps[4..], ["lbl", "--who", "engine", "--"]);
        assert!(expand_args(&s(&["--who", "x"]), Path::new("/")).is_err());
    }

    #[test]
    fn run_stages_each_variant_starts_the_viewer_and_compares_their_pictures() {
        let t = tmp("run");
        let g = install(&t);
        let overlay = t.join("overlay");
        std::fs::create_dir_all(overlay.join("meshes")).unwrap();
        std::fs::write(overlay.join("Lod.esp"), "x").unwrap();
        let viewer = fake_viewer(&t, 0);
        let args = s(&[
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

    #[cfg(unix)]
    #[test]
    fn a_failing_viewer_makes_the_run_fail_and_a_missing_one_is_an_error() {
        let t = tmp("fail");
        let g = install(&t);
        let common = |viewer: &Path| {
            s(&[
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
}
