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
//! `FILE` is the ledger's per-function table:
//! `cargo run --release --manifest-path scripts/ledger/Cargo.toml -- --tsv FILE`.

use progress::{
    baseline_counts, counts, drops, measure, pins, render, render_summary, report, Inputs,
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
                    Ok(t) => baseline_counts(&t)?,
                    // A first run has no baseline.
                    Err(_) => Vec::new(),
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
        _ => Err("commands: fetch, generate, check, draw, paths (see the source's head)".into()),
    }
}
