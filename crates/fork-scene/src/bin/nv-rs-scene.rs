// SPDX-License-Identifier: GPL-3.0-only
//! `nv-rs-scene compare A B`: the difference between two pictures, or between the same-named PNGs
//! of two folders (a determinism check, or two variants of the same places). Exit 0 when every
//! pair was compared; with `--max-mean N` exit 1 when a pair's mean difference is above N.

use fork_scene::diff::{compare, decode_png, Difference};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// A variant's run: the engine's exit status and the seconds it took, or why it did not start.
type RunResult = Result<(i32, f64), String>;

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
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--max-mean" {
            max_mean = Some(
                it.next()
                    .and_then(|v| v.parse::<f64>().ok())
                    .ok_or("--max-mean needs a number")?,
            );
        } else if a == "--diff-out" {
            diff_out = Some(PathBuf::from(it.next().ok_or("--diff-out needs a folder")?));
        } else {
            paths.push(PathBuf::from(a));
        }
    }
    let [a, b] = paths.as_slice() else {
        return Err(
            "usage: nv-rs-scene compare [--max-mean N] [--diff-out DIR] A B  (two PNGs or two folders)".into(),
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

/// `nv-rs-scene run`: every variant of the game's data on one place file, one engine start each (in
/// parallel up to `--jobs`), then the first variant's pictures against every other's.
fn run_variants(args: &[String]) -> Result<bool, String> {
    let (mut viewer, mut game, mut places, mut out, mut userdata) = (None, None, None, None, None);
    let (mut variants, mut wrap, mut settle, mut timeout, mut jobs) = (
        Vec::new(),
        Vec::new(),
        "200".to_string(),
        "120".to_string(),
        3usize,
    );
    let mut resolution = "1280x720".to_string();
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
            "--jobs" => jobs = value(a)?.parse().map_err(|_| "--jobs is a number")?,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let usage = "usage: nv-rs-scene run --viewer BIN --game DIR --places FILE --out DIR [--userdata TEMPLATE] [--variant NAME[=OVERLAY]]... [--settle N] [--timeout S] [--resolution WxH] [--jobs N] [--wrap WORD]...";
    let (Some(viewer), Some(game), Some(places), Some(out)) = (viewer, game, places, out) else {
        return Err(usage.into());
    };
    if variants.is_empty() {
        variants.push("base".into());
    }
    let variants: Vec<(String, Option<PathBuf>)> = variants
        .iter()
        .map(|v| match v.split_once('=') {
            Some((n, o)) => (n.to_string(), Some(PathBuf::from(o))),
            None => (v.clone(), None),
        })
        .collect();
    let started = std::time::Instant::now();
    let mut results: Vec<(String, RunResult)> = Vec::new();
    for chunk in variants.chunks(jobs.max(1)) {
        std::thread::scope(|scope| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|(name, overlay)| {
                    let (viewer, game, places, out, userdata, wrap, settle, timeout, resolution) = (
                        &viewer,
                        &game,
                        &places,
                        &out,
                        &userdata,
                        &wrap,
                        &settle,
                        &timeout,
                        &resolution,
                    );
                    scope.spawn(move || {
                        let t = std::time::Instant::now();
                        let r = run_one(
                            name,
                            overlay.as_deref(),
                            viewer,
                            game,
                            places,
                            out,
                            userdata.as_deref(),
                            wrap,
                            settle,
                            timeout,
                            resolution,
                            &[],
                        );
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

#[allow(clippy::too_many_arguments)]
fn run_one(
    name: &str,
    overlay: Option<&Path>,
    viewer: &Path,
    game: &Path,
    places: &Path,
    out: &Path,
    userdata: Option<&Path>,
    wrap: &[String],
    settle: &str,
    timeout: &str,
    resolution: &str,
    extra_env: &[(String, String)],
) -> Result<i32, String> {
    let dir = out.join(name);
    // A packed overlay names its plugins: every plugin file at its top.
    let mut plugins = Vec::new();
    if let Some(o) = overlay {
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
    let staged = fork_scene::stage::stage(game, overlay, &plugins, userdata, &dir)
        .map_err(|e| format!("staging: {e}"))?;
    let log = std::fs::File::create(dir.join("run.log")).map_err(|e| e.to_string())?;
    let mut cmd_words: Vec<std::ffi::OsString> = wrap.iter().map(Into::into).collect();
    cmd_words.push(viewer.into());
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
        .env("NV_RESOLUTION", resolution)
        .env("NV_THREADS", "real")
        .env("NV_LOG_JSON", dir.join("log.jsonl"))
        .env("NV_LOG_TERM", "warn")
        .env("NV_SCENE", places)
        .env("NV_SCENE_SETTLE_FRAMES", settle)
        .env("NV_SCENE_TIMEOUT_S", timeout)
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log);
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    if !extra_env.iter().any(|(k, _)| k == "NV_SCENE_SOAK_FRAMES") {
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
    let (mut frames, mut wrap, mut realtime, mut timeout) =
        ("600".to_string(), Vec::new(), false, "300".to_string());
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
            "--timeout" => timeout = value(a)?,
            "--wrap" => wrap.push(value(a)?),
            "--realtime" => realtime = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let (Some(viewer), Some(game), Some(places), Some(out)) = (viewer, game, places, out) else {
        return Err("usage: nv-rs-scene soak --viewer BIN --game DIR --places FILE --out DIR [--frames N] [--timeout S] [--realtime] [--userdata TPL] [--wrap WORD]...".into());
    };
    let mut extra = vec![("NV_SCENE_SOAK_FRAMES".to_string(), frames.clone())];
    if realtime {
        extra.push(("NV_SCENE_REALTIME".into(), "1".into()));
    }
    let t = std::time::Instant::now();
    let code = run_one(
        "soak",
        None,
        &viewer,
        &game,
        &places,
        &out,
        userdata.as_deref(),
        &wrap,
        "60",
        &timeout,
        "1280x720",
        &extra,
    )?;
    let secs = t.elapsed().as_secs_f64();
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
    let n_places = std::fs::read_to_string(&places).map_or(0, |t| t.matches("[[place]]").count());
    println!(
        "soak: {} of {n_places} places soaked for {frames} frames, exit status {code}, {stops} stops, {errors} error lines, {warnings} warnings, {secs:.0} s{}",
        rows.len(),
        if code != 0 { format!("; ended at place {last_place}, see {}", out.join("soak").join("run.log").display()) } else { String::new() }
    );
    Ok(code == 0 && stops == 0 && errors == 0 && rows.len() == n_places)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("compare") => run(&args[1..]),
        Some("run") => run_variants(&args[1..]),
        Some("soak") => soak(&args[1..]),
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
