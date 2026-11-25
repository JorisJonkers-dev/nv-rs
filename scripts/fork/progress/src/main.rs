// SPDX-License-Identifier: GPL-3.0-only
//! `progress fetch --sources DIR`: fetches the pinned outside sources (git, shallow and sparse).
//!
//! `progress generate --sources DIR --ledger FILE [--root DIR]`: writes the pictures and the
//! summary (`progress::OUTPUTS`).
//!
//! `progress check --sources DIR --ledger FILE [--root DIR] [--baseline FILE] [--allow-drop]`:
//! writes nothing; counts twice and fails (exit 1) when the two results differ or a reading
//! fails; prints the report (Markdown); exits 3 when a count is lower than in the baseline
//! summary, unless `--allow-drop`.
//!
//! `progress paths`: the files `generate` writes, one per line (the only files the workflow that
//! commits them may commit).
//!
//! `progress draw --summary FILE [--root DIR]`: draws the pictures again from a summary alone.
//!
//! `progress catalogue --sources DIR --ledger FILE [--root DIR] [--check] [--evidence FILE]`:
//! writes the coverage catalogue's reference lists and their pins (`.fork/catalogue/ref/*.tsv`,
//! `.fork/catalogue/sources.toml`), or with `--check` writes nothing and fails (exit 1) when one
//! of them differs; `--evidence FILE` also writes the implementation evidence `tally check`
//! compares the Direct3D 9 and graphics groups with (docs/fork/CATALOGUE.md).
//!
//! `FILE` is the ledger's per-function table:
//! `cargo run --release --manifest-path scripts/ledger/Cargo.toml -- --tsv FILE`.

use progress::{
    baseline_counts, catalogue, counts, drops, engine_features, measure, pins, render,
    render_summary, report, Inputs,
};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn need<'a>(args: &'a [String], name: &str) -> Result<&'a str, String> {
    flag(args, name).ok_or_else(|| format!("{name} is required"))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("progress: {e}");
            ExitCode::from(1)
        }
    }
}

fn root(args: &[String]) -> PathBuf {
    flag(args, "--root").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."),
        PathBuf::from,
    )
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    if args.first().map(String::as_str) == Some("paths") {
        for p in progress::OUTPUTS {
            println!("{p}");
        }
        return Ok(ExitCode::SUCCESS);
    }
    if args.first().map(String::as_str) == Some("draw") {
        let f = need(args, "--summary")?;
        let text = std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?;
        let root = root(args);
        for (rel, svg) in render_summary(&text)? {
            std::fs::write(root.join(rel), svg).map_err(|e| format!("{rel}: {e}"))?;
        }
        return Ok(ExitCode::SUCCESS);
    }
    let sources = PathBuf::from(need(args, "--sources")?);
    let cmd = args.first().map(String::as_str);
    if cmd == Some("fetch") {
        pins::fetch(&sources)?;
        return Ok(ExitCode::SUCCESS);
    }
    let root = root(args);
    let ledger = need(args, "--ledger")?;
    let tsv = std::fs::read_to_string(ledger).map_err(|e| format!("{ledger}: {e}"))?;
    let inp = Inputs {
        root: &root,
        sources: &sources,
        ledger_tsv: &tsv,
    };
    match cmd {
        Some("catalogue") => {
            let p = measure(&inp)?;
            let lists = catalogue::ref_lists(&root, &sources, &p, &tsv)?;
            let mut files: Vec<(String, String)> = vec![(
                ".fork/catalogue/sources.toml".into(),
                catalogue::sources_toml(&sources, &lists)?,
            )];
            files.extend(
                lists
                    .into_iter()
                    .map(|l| (format!(".fork/catalogue/{}", l.file), l.text)),
            );
            let check = args.iter().any(|a| a == "--check");
            let mut stale = Vec::new();
            for (rel, text) in &files {
                let path = root.join(rel);
                let old = std::fs::read_to_string(&path).unwrap_or_default();
                if old == *text {
                    continue;
                }
                if check {
                    stale.push(rel.clone());
                } else {
                    std::fs::write(&path, text).map_err(|e| format!("{rel}: {e}"))?;
                    println!("progress: wrote {rel}");
                }
            }
            if let Some(f) = flag(args, "--evidence") {
                let (_, features) = engine_features::read(&root, &tsv)?;
                std::fs::write(f, catalogue::evidence(&p, &features))
                    .map_err(|e| format!("{f}: {e}"))?;
            }
            if !stale.is_empty() {
                for s in &stale {
                    println!("stale: {s}");
                }
                println!("The catalogue's reference lists differ from the sources: run `task catalogue` and commit the files.");
                return Ok(ExitCode::from(1));
            }
            Ok(ExitCode::SUCCESS)
        }
        Some("generate") => {
            let p = measure(&inp)?;
            for (rel, text) in render(&p) {
                let path = root.join(rel);
                let old = std::fs::read_to_string(&path).unwrap_or_default();
                if old != text {
                    std::fs::write(&path, &text).map_err(|e| format!("{rel}: {e}"))?;
                    println!("progress: wrote {rel}");
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Some("check") => {
            let (a, b) = (measure(&inp)?, measure(&inp)?);
            if render(&a) != render(&b) {
                return Err("two runs over the same tree wrote different files".into());
            }
            let before = match flag(args, "--baseline") {
                Some(f) => match std::fs::read_to_string(f) {
                    Ok(t) => baseline_counts(&t).map_err(|e| format!("{f}: {e}"))?,
                    // A first run has no baseline; any other failure to read it stops the check.
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                    Err(e) => return Err(format!("{f}: {e}")),
                },
                None => Vec::new(),
            };
            print!("{}", report(&a, &before));
            let down = drops(&before, &counts(&a));
            if down.is_empty() {
                println!("\nNo count went down.");
                return Ok(ExitCode::SUCCESS);
            }
            println!("\n### Counts that went down\n");
            for (k, b, n) in &down {
                println!("- `{k}`: {b} to {n}");
            }
            if args.iter().any(|a| a == "--allow-drop") {
                println!("\nAllowed: the pull request says why (a `Progress-Drop:` line).");
                return Ok(ExitCode::SUCCESS);
            }
            println!("\nA count went down. Say why in the pull request description with a line `Progress-Drop: <reason>`, or keep the count.");
            Ok(ExitCode::from(3))
        }
        _ => Err(
            "commands: fetch, generate, check, catalogue, draw, paths (see the source's head)"
                .into(),
        ),
    }
}
