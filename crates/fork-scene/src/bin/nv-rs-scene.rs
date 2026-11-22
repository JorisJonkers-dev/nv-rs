// SPDX-License-Identifier: GPL-3.0-only
//! `nv-rs-scene compare A B`: the difference between two pictures, or between the same-named PNGs
//! of two folders (a determinism check, or two variants of the same places). Exit 0 when every
//! pair was compared; with `--max-mean N` exit 1 when a pair's mean difference is above N.

use fork_scene::diff::{compare, decode_png, Difference};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

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
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--max-mean" {
            max_mean = Some(
                it.next()
                    .and_then(|v| v.parse::<f64>().ok())
                    .ok_or("--max-mean needs a number")?,
            );
        } else {
            paths.push(PathBuf::from(a));
        }
    }
    let [a, b] = paths.as_slice() else {
        return Err(
            "usage: nv-rs-scene compare [--max-mean N] A B  (two PNGs or two folders)".into(),
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
            check(&name, compare(&load(&pa)?, &load(&pb)?)?, &mut ok);
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

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("compare") {
        eprintln!("usage: nv-rs-scene compare [--max-mean N] A B");
        return ExitCode::from(2);
    }
    match run(&args[1..]) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}
