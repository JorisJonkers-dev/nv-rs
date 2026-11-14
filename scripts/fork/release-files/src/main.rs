//! `release-files capabilities [--out FILE] [--check FILE]`
//! `release-files manifest --version V --upstream-base SHA --source-commit SHA --source-tree SHA [--out FILE]`
//!
//! `release-files validate --schema SCHEMA FILE`: exit 1 when FILE does not validate against SCHEMA.
//!
//! With neither `--out` nor `--check`, a file is printed to standard output. `--check FILE` exits 1
//! when FILE differs from what the tables generate (regenerate with `--out`).

use release_files::{capabilities, manifest, schema_errors, to_text, ManifestInput};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("release-files: {e}");
            ExitCode::from(2)
        }
    }
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn need<'a>(args: &'a [String], name: &str) -> Result<&'a str, String> {
    flag(args, name).ok_or_else(|| format!("{name} is required"))
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    if args.first().map(String::as_str) == Some("validate") {
        return validate(args);
    }
    let text = match args.first().map(String::as_str) {
        Some("capabilities") => to_text(&capabilities()),
        Some("manifest") => {
            let sha = |name: &str| -> Result<String, String> {
                let v = need(args, name)?;
                if v.len() == 40 && v.bytes().all(|b| b.is_ascii_hexdigit()) {
                    Ok(v.to_ascii_lowercase())
                } else {
                    Err(format!("{name} is not a 40-digit hexadecimal id"))
                }
            };
            to_text(&manifest(&ManifestInput {
                version: need(args, "--version")?.to_string(),
                upstream_base_commit: sha("--upstream-base")?,
                source_commit: sha("--source-commit")?,
                source_tree_hash: sha("--source-tree")?,
            }))
        }
        _ => {
            return Err(
                "usage: release-files capabilities|manifest [options] (see src/main.rs)".into(),
            )
        }
    };
    if let Some(file) = flag(args, "--check") {
        let have = std::fs::read_to_string(file)
            .map_err(|e| format!("{file}: {e}"))?
            .replace("\r\n", "\n");
        if have != text {
            eprintln!("{file} is out of date with the command tables; regenerate it with --out");
            return Ok(ExitCode::from(1));
        }
        return Ok(ExitCode::SUCCESS);
    }
    match flag(args, "--out") {
        Some(file) => std::fs::write(file, text).map_err(|e| format!("{file}: {e}"))?,
        None => print!("{text}"),
    }
    Ok(ExitCode::SUCCESS)
}

fn validate(args: &[String]) -> Result<ExitCode, String> {
    let read = |file: &str| -> Result<serde_json::Value, String> {
        let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
        serde_json::from_str(&text).map_err(|e| format!("{file}: {e}"))
    };
    let file = args.last().ok_or("validate needs a FILE")?;
    let errors = schema_errors(&read(need(args, "--schema")?)?, &read(file)?)?;
    for e in &errors {
        eprintln!("{file}: {e}");
    }
    Ok(if errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}
