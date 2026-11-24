// SPDX-License-Identifier: GPL-3.0-only
//! The outside sources the reference lists are read from, each at one commit, and how they are
//! fetched: `git` with a shallow, sparse fetch of the named paths only. Moving a pin is a pull
//! request that changes the commit here; the summary names each pin, so its counts say which
//! version they are against.

use std::path::{Path, PathBuf};
use std::process::Command;

/// One pinned outside source.
#[derive(Debug, Clone, Copy)]
pub struct Pin {
    /// The folder under the sources directory, and the key in the summary.
    pub id: &'static str,
    /// `owner/name` on GitHub.
    pub repo: &'static str,
    pub commit: &'static str,
    /// The release the commit is, as the source names it.
    pub version: &'static str,
    /// The paths read (sparse checkout patterns, from the repository's root).
    pub paths: &'static [&'static str],
    /// The day the files were read at this commit (the coverage catalogue's `read`); a pull request that moves
    /// `commit` sets it to that day.
    pub read: &'static str,
}

pub const PINS: &[Pin] = &[
    Pin {
        id: "xnvse",
        repo: "xNVSE/NVSE",
        // Tag 6.4.9, the version crates/nvse's name table is read from (crates/nvse/src/names.rs).
        commit: "3ff1e21d146e626eb71552af9f1ad381e7f943dd",
        version: "6.4.9",
        read: "2026-11-24",
        paths: &["/nvse/nvse/"],
    },
    Pin {
        id: "jip-ln",
        repo: "jazzisparis/JIP-LN-NVSE",
        // 57.30, the commit of crates/jip/data/source-57.30.tsv.
        commit: "5a30ac4356ea0e93b9ff357b5031b1e420240a4d",
        version: "57.30",
        read: "2026-11-24",
        paths: &["/jip_nvse.cpp"],
    },
    Pin {
        id: "johnnyguitar",
        repo: "carxt/JohnnyGuitarNVSE",
        commit: "6215c7044d7159f0a844e533085efe85ce7e599e",
        version: "5.28",
        read: "2026-11-24",
        paths: &[
            "/JG/internal/JG/JohnnyCommands.cpp",
            "/JG/internal/JG/JohnnyPluginData.hpp",
        ],
    },
    Pin {
        id: "showoff",
        repo: "Demorome/ShowOff-NVSE",
        commit: "f45d069d5357968f01246d87bbdcdee4b030dc90",
        version: "1.84",
        read: "2026-11-24",
        paths: &["/SHOWOFF-NVSE/ShowOffNVSE.cpp"],
    },
    Pin {
        id: "knvse",
        repo: "korri123/kNVSE",
        commit: "0777ffbe91f8d82ce6be7c7587cfcbac8c820657",
        version: "40",
        read: "2026-11-24",
        paths: &[
            "/nvse_plugin_example/main.cpp",
            "/nvse_plugin_example/commands_animation.cpp",
        ],
    },
    Pin {
        id: "d3d9-headers",
        repo: "mingw-w64/mingw-w64",
        // Tag v13.0.0.
        commit: "de62c283f3383bd825c98518cb0a42ba071d4ac2",
        version: "v13.0.0",
        read: "2026-11-24",
        paths: &[
            "/mingw-w64-headers/include/d3d9.h",
            "/mingw-w64-headers/include/d3d9types.h",
        ],
    },
];

pub fn pin(id: &str) -> Pin {
    *PINS.iter().find(|p| p.id == id).expect("a pin of that id")
}

/// Where a pin's checkout lives under `sources`.
pub fn dir(sources: &Path, id: &str) -> PathBuf {
    sources.join(id)
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {} in {}: {}",
            args.join(" "),
            dir.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Fetches every pin into `sources/<id>` (once: a checkout already at the commit is kept).
pub fn fetch(sources: &Path) -> Result<(), String> {
    for p in PINS {
        let d = dir(sources, p.id);
        if git(&d, &["rev-parse", "HEAD"]).ok().as_deref() == Some(p.commit) {
            continue;
        }
        if d.exists() {
            std::fs::remove_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        git(&d, &["init", "-q"])?;
        let mut sparse = vec!["sparse-checkout", "set", "--no-cone"];
        sparse.extend(p.paths.iter().copied());
        git(&d, &sparse)?;
        let url = format!("https://github.com/{}", p.repo);
        git(
            &d,
            &[
                "fetch",
                "-q",
                "--depth",
                "1",
                "--filter=blob:none",
                &url,
                p.commit,
            ],
        )?;
        git(&d, &["checkout", "-q", "FETCH_HEAD"])?;
        let got = git(&d, &["rev-parse", "HEAD"])?;
        if got != p.commit {
            return Err(format!("{}: fetched {got}, pinned {}", p.repo, p.commit));
        }
        eprintln!("progress: fetched {} at {}", p.repo, &p.commit[..12]);
    }
    Ok(())
}

/// A file of a pin's checkout.
pub fn read(sources: &Path, id: &str, path: &str) -> Result<String, String> {
    let f = dir(sources, id).join(path);
    let bytes = std::fs::read(&f).map_err(|e| {
        format!(
            "{}: {e} (fetch the pinned sources first: progress fetch --sources {})",
            f.display(),
            sources.display()
        )
    })?;
    // Some of the sources are in Windows-1252; the names read are ASCII either way.
    Ok(String::from_utf8_lossy(&bytes).replace("\r\n", "\n"))
}

/// The files of a pin's checkout below `path` with one of the extensions, sorted.
pub fn files(sources: &Path, id: &str, path: &str, exts: &[&str]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir(sources, id).join(path))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.extension()
                        .and_then(|x| x.to_str())
                        .is_some_and(|x| exts.contains(&x))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}
