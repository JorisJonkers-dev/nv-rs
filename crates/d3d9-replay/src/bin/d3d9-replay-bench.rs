//! `d3d9-replay-bench`: the replay-bench of the recorded Direct3D 9 stream (docs/fork/REPLAY.md).
//!
//! ```text
//! d3d9-replay-bench info  FILE                        what a recording or container holds
//! d3d9-replay-bench pack  RAW OUT --scene NAME --from F --frames N
//!                                                     a window of a recording as a container
//! d3d9-replay-bench synth OUT [--frames N] [--draws N] [--textures N] [--size WxH]
//!                                                     a made-up scene as a container
//! d3d9-replay-bench run   FILE [--backend null|wgpu|vulkan|all] [--size WxH] [--warmup N]
//!                         [--passes N] [--runs N] [--split] [--gpu]
//! ```
//!
//! Measurement runs go through `nv-rs-dev replay run`, which takes the bench lock.

use d3d9_replay::container::{self, Meta};
use d3d9_replay::replay::{self, Options, Replay, Target, WgpuTarget};
use d3d9_replay::stats::{summarize, timer_overhead_ns};
use d3d9_replay::{null, rescale, synthetic, window};
use d3d9_stream::{Cmd, Data};
use std::path::{Path, PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("info") if args.len() >= 2 => info(Path::new(&args[1])),
        Some("pack") if args.len() >= 3 => pack(&args[1..]),
        Some("synth") if args.len() >= 2 => synth(&args[1..]),
        Some("run") if args.len() >= 2 => run(&args[1..]),
        _ => Err(include_str!("d3d9-replay-bench.rs")
            .lines()
            .skip(2)
            .take_while(|l| l.starts_with("//!"))
            .map(|l| l.trim_start_matches("//!").trim_start_matches(' '))
            .collect::<Vec<_>>()
            .join("\n")),
    };
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(2);
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn number(args: &[String], name: &str, default: usize) -> Result<usize, String> {
    flag(args, name).map_or(Ok(default), |v| {
        v.parse()
            .map_err(|_| format!("{name}: {v} is not a number"))
    })
}

fn size(args: &[String]) -> Result<Option<(u32, u32)>, String> {
    flag(args, "--size")
        .map(|v| {
            let (w, h) = v.split_once('x').ok_or("--size WxH")?;
            Ok((
                w.parse().map_err(|_| "--size WxH")?,
                h.parse().map_err(|_| "--size WxH")?,
            ))
        })
        .transpose()
}

fn parse_all(bytes: &[u8]) -> Result<Vec<Cmd<Data<'_>>>, String> {
    d3d9_stream::wire::commands(bytes)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn info(path: &Path) -> Result<(), String> {
    let (meta, bytes) = container::load(path).map_err(|e| e.to_string())?;
    let cmds = parse_all(&bytes)?;
    println!("file: {}", path.display());
    println!("stream: {} bytes, {} commands", bytes.len(), cmds.len());
    if let Some(m) = &meta {
        println!(
            "container: scene={} frames={} setup_cmds={} draws={} size={}x{}",
            m.scene, m.frames, m.setup_cmds, m.draws, m.width, m.height
        );
    }
    let mut counts: std::collections::BTreeMap<&str, usize> = Default::default();
    for c in &cmds {
        *counts.entry(c.name()).or_default() += 1;
    }
    let mut by: Vec<_> = counts.into_iter().collect();
    by.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
    for (name, n) in by.iter().take(14) {
        println!("  {name}: {n}");
    }
    let mut per_frame = Vec::new();
    let mut n = 0u32;
    for c in &cmds {
        match c {
            Cmd::DrawPrimitive { .. }
            | Cmd::DrawIndexedPrimitive { .. }
            | Cmd::DrawPrimitiveUp { .. } => n += 1,
            Cmd::Present { .. } => {
                per_frame.push(n);
                n = 0;
            }
            _ => {}
        }
    }
    // The targets and the textures' formats, so a window's shape is known before it is replayed.
    let mut made: std::collections::BTreeMap<String, usize> = Default::default();
    for c in &cmds {
        let key = match c {
            Cmd::CreateSurface {
                width,
                height,
                format,
                multisample,
                depth,
                ..
            } => {
                format!(
                    "surface {width}x{height} fmt {format:#x} ms {multisample}{}",
                    if *depth { " depth" } else { "" }
                )
            }
            Cmd::CreateTexture {
                width,
                height,
                format,
                usage,
                levels,
                ..
            } if usage & 3 != 0 => {
                format!(
                    "target texture {width}x{height} fmt {format:#x} levels {}",
                    levels.len() / 4
                )
            }
            Cmd::CreateTexture { format, .. } => format!("texture fmt {format:#x}"),
            Cmd::CreateCubeTexture { format, .. } => format!("cube fmt {format:#x}"),
            _ => continue,
        };
        *made.entry(key).or_default() += 1;
    }
    for (k, n) in &made {
        println!("  made: {k} x{n}");
    }
    println!("frames: {}", per_frame.len());
    if let Some(s) = size_of(&cmds) {
        println!("back buffer: {}x{}", s.0, s.1);
    }
    // Draws a frame, in runs of 100 frames, so a window with many draws is easy to pick.
    for (i, chunk) in per_frame.chunks(100).enumerate() {
        let mean = chunk.iter().map(|&d| f64::from(d)).sum::<f64>() / chunk.len() as f64;
        let max = chunk.iter().max().copied().unwrap_or(0);
        println!(
            "  frames {:>6}..{:<6} draws/frame mean {:>7.1} max {}",
            i * 100,
            i * 100 + chunk.len(),
            mean,
            max
        );
    }
    Ok(())
}

fn size_of(cmds: &[Cmd<Data<'_>>]) -> Option<(u32, u32)> {
    rescale::back_buffer_size(cmds)
}

fn pack(args: &[String]) -> Result<(), String> {
    let (raw, out) = (&args[0], &args[1]);
    let scene = flag(args, "--scene").ok_or("--scene NAME")?;
    let from = number(args, "--from", 1)?;
    let frames = number(args, "--frames", 60)?;
    let (_, bytes) = container::load(Path::new(raw)).map_err(|e| e.to_string())?;
    let all = parse_all(&bytes)?;
    let (w, h) = size_of(&all).unwrap_or((0, 0));
    let cut = window::cut(&all, from, frames)?;
    let wire = window::encode(&cut.cmds);
    let draws = cut.cmds[cut.setup..]
        .iter()
        .filter(|c| {
            matches!(
                c,
                Cmd::DrawPrimitive { .. }
                    | Cmd::DrawIndexedPrimitive { .. }
                    | Cmd::DrawPrimitiveUp { .. }
            )
        })
        .count() as u64;
    let meta = Meta {
        scene,
        frames: frames as u32,
        setup_cmds: cut.setup as u64,
        draws,
        width: w,
        height: h,
        packed_at: now(),
        extra: vec![("from_frame".into(), from.to_string())],
    };
    write_container(out, &meta, &wire)?;
    println!(
        "packed frames {from}..{} of {raw}: {} setup commands, {draws} draws, {} bytes",
        from + frames,
        cut.setup,
        std::fs::metadata(out).map_or(0, |m| m.len())
    );
    Ok(())
}

fn write_container(out: &str, meta: &Meta, wire: &[u8]) -> Result<(), String> {
    if let Some(dir) = Path::new(out).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut f =
        std::io::BufWriter::new(std::fs::File::create(out).map_err(|e| format!("{out}: {e}"))?);
    container::write(&mut f, meta, wire).map_err(|e| e.to_string())
}

fn synth(args: &[String]) -> Result<(), String> {
    let (w, h) = size(args)?.unwrap_or((640, 480));
    let scene = synthetic::Scene {
        frames: number(args, "--frames", 60)? as u32,
        draws: number(args, "--draws", 1000)? as u32,
        textures: number(args, "--textures", 32)? as u32,
        width: w,
        height: h,
    };
    let wire = synthetic::stream(&scene);
    let meta = Meta {
        scene: "synthetic".into(),
        frames: scene.frames,
        setup_cmds: synthetic::setup_cmds(&scene),
        draws: u64::from(scene.frames) * u64::from(scene.draws),
        width: w,
        height: h,
        packed_at: now(),
        extra: Vec::new(),
    };
    write_container(&args[0], &meta, &wire)?;
    println!("synthetic scene: {scene:?}");
    Ok(())
}

fn run(args: &[String]) -> Result<(), String> {
    let path = PathBuf::from(&args[0]);
    let (meta, mut bytes) = container::load(&path).map_err(|e| e.to_string())?;
    let setup = meta.as_ref().map_or(0, |m| m.setup_cmds);
    let scene = meta.as_ref().map_or("raw".to_string(), |m| m.scene.clone());
    if let Some(to) = size(args)? {
        let all = parse_all(&bytes)?;
        let from = size_of(&all).ok_or("no CreateDevice in the stream")?;
        bytes = rescale::rescale(&all, &rescale::Scale::new(from, to));
    }
    let replay = Replay::parse(&bytes, setup)?;
    let back = size_of(&replay.cmds[..replay.setup.max(1)]).unwrap_or((0, 0));
    let all = matches!(flag(args, "--backend").as_deref(), None | Some("all"));
    let backends: Vec<String> = match flag(args, "--backend").as_deref() {
        None | Some("all") => vec!["null".into(), "wgpu".into(), "vulkan".into()],
        Some(b) => b.split(',').map(str::to_string).collect(),
    };
    let o = Options {
        warmup: number(args, "--warmup", 3)?,
        passes: number(args, "--passes", 10)?,
    };
    let runs = number(args, "--runs", 3)?;
    let overhead = timer_overhead_ns();
    println!(
        "scene={scene} backbuffer={}x{} frames={} draws_per_pass={} setup_cmds={} warmup={} passes={} runs={runs} timer_ns={overhead:.0}",
        back.0,
        back.1,
        replay.frames.len(),
        replay.draws_per_pass(),
        replay.setup,
        o.warmup,
        o.passes,
    );
    let total_frames = replay.frames.len() * (o.warmup + o.passes);
    if has(args, "--gpu") {
        // Read when the device is made (d3d9-wgpu's `open`).
        std::env::set_var("NV_REPLAY_GPU_TIMING", "1");
    }
    for name in &backends {
        for r in 1..=runs {
            let mut target: Box<dyn Target> = match name.as_str() {
                "null" => Box::new(null::Null::new()),
                "wgpu" => Box::new(WgpuTarget::new(has(args, "--gpu").then_some(total_frames))),
                "vulkan" => match d3d9_replay::vulkan::VulkanTarget::new(
                    has(args, "--gpu").then_some(total_frames),
                ) {
                    Ok(t) => Box::new(t),
                    // With every backend asked for, a machine without Vulkan skips this one.
                    Err(e) if all => {
                        println!("vulkan: skipped ({e})");
                        continue;
                    }
                    Err(e) => return Err(e),
                },
                other => return Err(format!("unknown backend {other} (null, wgpu, vulkan)")),
            };
            let res = replay::run(target.as_mut(), &replay, o)?;
            report(&res, r);
            let notes = target.notes();
            if !notes.is_empty() {
                println!("  notes: {notes}");
            }
            if has(args, "--split") {
                let s = replay::split(target.as_mut(), &replay, overhead);
                print_split(name, &s);
            }
        }
    }
    Ok(())
}

fn ms(ns: &[f64]) -> Vec<f64> {
    ns.iter().map(|n| n / 1e6).collect()
}

fn report(r: &replay::Run, run: usize) {
    let mut cmd = ms(&r.frames.iter().map(|f| f.cmd_ns as f64).collect::<Vec<_>>());
    let mut present = ms(&r
        .frames
        .iter()
        .map(|f| f.present_ns as f64)
        .collect::<Vec<_>>());
    let mut total = ms(&r
        .frames
        .iter()
        .map(|f| (f.cmd_ns + f.present_ns) as f64)
        .collect::<Vec<_>>());
    let mut thread_cpu = ms(&r
        .frames
        .iter()
        .map(|f| f.thread_cpu_ns as f64)
        .collect::<Vec<_>>());
    let mut process_cpu = ms(&r
        .frames
        .iter()
        .map(|f| f.process_cpu_ns as f64)
        .collect::<Vec<_>>());
    let draws: u64 = r.frames.iter().map(|f| u64::from(f.draws)).sum();
    let cmd_ns: u64 = r.frames.iter().map(|f| f.cmd_ns).sum();
    let present_ns: u64 = r.frames.iter().map(|f| f.present_ns).sum();
    let thread_ns: u64 = r.frames.iter().map(|f| f.thread_cpu_ns).sum();
    let process_ns: u64 = r.frames.iter().map(|f| f.process_cpu_ns).sum();
    let per = |ns: u64| {
        if draws == 0 {
            0.0
        } else {
            ns as f64 / draws as f64 / 1e3
        }
    };
    let (c, p, t, tc, pc) = (
        summarize(&mut cmd),
        summarize(&mut present),
        summarize(&mut total),
        summarize(&mut thread_cpu),
        summarize(&mut process_cpu),
    );
    println!(
        "{} run={run} setup_ms={:.0} warmup_ms={:.0} measured_ms={:.0} frames={} draws={draws}",
        r.target, r.setup_ms, r.warmup_ms, r.wall_ms, c.n
    );
    println!(
        "  us/draw: record {:.3} | record+present {:.3} | thread cpu {:.3} | process cpu {:.3}",
        per(cmd_ns),
        per(cmd_ns + present_ns),
        per(thread_ns),
        per(process_ns),
    );
    let row = |name: &str, s: &d3d9_replay::stats::Summary| {
        println!(
            "  {name:<12} ms/frame mean {:.3} p50 {:.3} p90 {:.3} p99 {:.3} max {:.3}",
            s.mean, s.p50, s.p90, s.p99, s.max
        );
    };
    row("record", &c);
    row("present", &p);
    row("frame", &t);
    row("thread cpu", &tc);
    row("process cpu", &pc);
    println!(
        "  draws/s: {:.0} (wall {:.0} ms over the measured passes)",
        draws as f64 / (r.wall_ms / 1e3).max(1e-9),
        r.wall_ms
    );
    let top: Vec<String> = r
        .threads
        .iter()
        .take(6)
        .filter(|(_, ns)| per(*ns) >= 0.01)
        .map(|(n, ns)| format!("{n} {:.3}", per(*ns)))
        .collect();
    if !top.is_empty() {
        println!("  threads, cpu us/draw: {}", top.join(", "));
    }
    if let Some(g) = &r.gpu_ns {
        let mut g = ms(g);
        let s = summarize(&mut g);
        println!(
            "  {:<12} ms/frame mean {:.3} p50 {:.3} p90 {:.3} p99 {:.3} max {:.3}",
            "gpu span", s.mean, s.p50, s.p90, s.p99, s.max
        );
    }
}

fn print_split(name: &str, s: &replay::Split) {
    let us = |ns: u64, n: u64| {
        if n == 0 {
            0.0
        } else {
            ns as f64 / n as f64 / 1e3
        }
    };
    println!(
        "  split {name} (one pass, each command timed): draw {:.3} us x{} | state {:.3} x{} | bind {:.3} x{} | constants {:.3} x{} | upload {:.3} x{} | other {:.3} x{} | present {:.1} us",
        us(s.draw_ns, s.draws),
        s.draws,
        us(s.state_ns, s.state),
        s.state,
        us(s.bind_ns, s.binds),
        s.binds,
        us(s.constants_ns, s.constants),
        s.constants,
        us(s.upload_ns, s.uploads),
        s.uploads,
        us(s.other_ns, s.other),
        s.other,
        s.present_ns as f64 / 1e3,
    );
}
