// SPDX-License-Identifier: GPL-3.0-only
//! `nv-rs-scene matrix` and `compare` (docs/fork/LOCAL.md, "Matrix runs"): build many refs (a commit, a branch, a tag, a pull
//! request number or the working tree) into worktrees, bench each with a built-in suite and say which one is better or worse
//! than the baseline. Local only: one machine, one game folder, the exclusive game lock. The Python version's remote hosts, its
//! configuration axes (the old harness's window sessions and modes) and its HTML page are not here.

use crate::bench::Headline;
use crate::tool::{self, BenchRequest, Launcher, Say, Start};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The worktrees this tool makes are named with this prefix, so `clean` never touches the user's own.
pub const WT_PREFIX: &str = "matrix-";

/// `nv-rs-scene matrix`.
#[derive(Clone, Debug)]
pub struct MatrixRequest {
    /// A checkout of the repository (a worktree is fine).
    pub repo: PathBuf,
    /// Refs to compare. The baseline is among them (it is put first when it is not).
    pub refs: Vec<String>,
    pub baseline: Option<String>,
    /// A built-in bench suite (`quick`, `mojave`, `soak`, `capital`).
    pub suite: String,
    pub game: PathBuf,
    /// The shared game lock file (the bench takes it exclusively).
    pub lock: PathBuf,
    /// Where `matrix.md`, `matrix.json` and the runs are written.
    pub out: PathBuf,
    /// Where built viewers are kept per commit (`~/.cache/nv-rs/matrix-bin`).
    pub bin_cache: PathBuf,
    /// `CARGO_TARGET_DIR` of the builds (`~/.cache/nv-rs/target`).
    pub target: PathBuf,
    /// Run the affected components' Linux tests (`nv-rs-dev ci-lite`) in each ref's worktree.
    pub test: bool,
    pub bench: bool,
    pub userdata: Option<PathBuf>,
    pub gpu_sysfs: PathBuf,
}

/// What happened to one ref.
#[derive(Clone, Debug)]
pub struct RefResult {
    pub requested: String,
    /// `fork@1a2b3c4d`, `pr93@1a2b3c4d`, `working@1a2b3c4d`.
    pub label: String,
    pub sha: Option<String>,
    /// `built`, `cached` or `failed` (with the end of the output).
    pub build: String,
    /// `pass`, `fail` or `skipped`.
    pub test: String,
    pub headlines: Vec<Headline>,
    /// The bench failed to run or its checks did not hold.
    pub bench_ok: bool,
    pub bench_note: String,
}

/// One scenario of a candidate against the baseline.
#[derive(Clone, Debug, PartialEq)]
pub struct Judged {
    pub scenario: String,
    pub metric: &'static str,
    pub base: f64,
    pub cand: f64,
    pub change_pct: f64,
    pub verdict: &'static str,
}

#[derive(Clone, Debug)]
pub struct MatrixResult {
    pub ok: bool,
    pub refs: Vec<RefResult>,
    /// Each non-baseline ref (its requested name) and its rows against the baseline.
    pub judged: Vec<(String, Vec<Judged>)>,
    pub markdown: String,
    pub dir: PathBuf,
}

/// The metrics compared, as the old report's: name, higher is better, percent limit, the noise floor below which a change is
/// not one.
const COMPARED: [(&str, bool, f64, f64); 3] = [
    ("avg fps", true, 5.0, 1.0),
    ("1% low fps", true, 10.0, 1.0),
    ("p99 ms", false, 10.0, 0.3),
];

fn metric(h: &Headline, name: &str) -> f64 {
    match name {
        "avg fps" => h.fps,
        "1% low fps" => h.low1_fps,
        _ => h.p99_ms,
    }
}

/// Each scenario the two share: a row per metric. A row regresses when it is worse by more than its limit in percent and by
/// more than its noise floor.
pub fn judge(base: &[Headline], cand: &[Headline]) -> Vec<Judged> {
    let mut rows = Vec::new();
    for b in base {
        let Some(c) = cand.iter().find(|c| c.scenario == b.scenario) else {
            continue;
        };
        for (name, higher, limit, floor) in COMPARED {
            let (va, vb) = (metric(b, name), metric(c, name));
            let worse = if higher { va - vb } else { vb - va };
            let change = if va != 0.0 {
                (vb - va) / va * 100.0
            } else {
                0.0
            };
            let bad = worse > floor && (va == 0.0 || worse / va.abs() * 100.0 > limit);
            let better = -worse > floor && !bad;
            rows.push(Judged {
                scenario: b.scenario.clone(),
                metric: name,
                base: va,
                cand: vb,
                change_pct: change,
                verdict: if bad {
                    "REGRESSION"
                } else if better {
                    "better"
                } else {
                    "ok"
                },
            });
        }
    }
    rows
}

fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let o = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if o.status.success() {
        Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&o.stderr).trim().to_string())
    }
}

/// A name safe in a folder name.
pub fn safe(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// A ref's label and commit (`None` for the working tree). Fetches a pull request, a branch or a tag from `origin` when needed.
pub fn resolve_ref(repo: &Path, r: &str) -> Result<(String, Option<String>), String> {
    if r == "working" || r == "." {
        let head = git(repo, &["rev-parse", "--short=8", "HEAD"])?;
        return Ok((format!("working@{head}"), None));
    }
    let digits = r
        .strip_prefix("pr:")
        .or_else(|| r.strip_prefix('#'))
        .unwrap_or(r);
    if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
        git(
            repo,
            &["fetch", "-q", "origin", &format!("pull/{digits}/head")],
        )
        .map_err(|e| format!("cannot fetch pull request {digits}: {e}"))?;
        let sha = git(repo, &["rev-parse", "FETCH_HEAD"])?;
        return Ok((format!("pr{digits}@{}", &sha[..8]), Some(sha)));
    }
    for cand in [
        r.to_string(),
        format!("origin/{r}"),
        format!("refs/tags/{r}"),
    ] {
        if let Ok(sha) = git(
            repo,
            &["rev-parse", "--verify", "-q", &format!("{cand}^{{commit}}")],
        ) {
            return Ok((format!("{}@{}", safe(r), &sha[..8]), Some(sha)));
        }
    }
    let _ = git(repo, &["fetch", "-q", "origin", r]);
    match git(
        repo,
        &["rev-parse", "--verify", "-q", "FETCH_HEAD^{commit}"],
    ) {
        Ok(sha) => Ok((format!("{}@{}", safe(r), &sha[..8]), Some(sha))),
        Err(_) => Err(format!(
            "unknown ref {r} (not a commit, branch, tag or pull request number)"
        )),
    }
}

/// The main clone behind a checkout.
fn main_clone(repo: &Path) -> Result<PathBuf, String> {
    let common = git(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let g = PathBuf::from(common);
    Ok(if g.file_name().is_some_and(|n| n == ".git") {
        g.parent().map(Path::to_path_buf).unwrap_or(g)
    } else {
        g
    })
}

/// `~/Projects/<owner>/worktrees/<repo>/` for the clone `~/Projects/<owner>/<repo>` (the layout rule).
pub fn worktree_base(repo: &Path) -> Result<PathBuf, String> {
    let clone = main_clone(repo)?;
    let name = clone.file_name().map(PathBuf::from).unwrap_or_default();
    Ok(clone
        .parent()
        .unwrap_or(&clone)
        .join("worktrees")
        .join(name))
}

fn ensure_worktree(repo: &Path, label: &str, sha: &str) -> Result<PathBuf, String> {
    let wt = worktree_base(repo)?.join(format!("{WT_PREFIX}{}", safe(label)));
    let clone = main_clone(repo)?;
    if wt.exists() {
        if git(&wt, &["rev-parse", "HEAD"]).is_ok_and(|h| h == sha) {
            return Ok(wt);
        }
        let _ = git(
            &clone,
            &["worktree", "remove", "--force", &wt.display().to_string()],
        );
    }
    if let Some(p) = wt.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    git(
        &clone,
        &[
            "worktree",
            "add",
            "--detach",
            "-q",
            &wt.display().to_string(),
            sha,
        ],
    )?;
    Ok(wt)
}

/// Builds `nv-viewer` (profile `play`) in `wt` into the shared target folder, behind `capped 10G` when it is installed; the
/// binary is kept under `bin_cache/<sha>`.
fn build(req: &MatrixRequest, wt: &Path, sha: Option<&str>) -> Result<(String, PathBuf), String> {
    let exe = req
        .bin_cache
        .join(sha.unwrap_or("working"))
        .join("nv-viewer");
    if sha.is_some() && exe.exists() {
        return Ok(("cached".into(), exe));
    }
    let capped = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/bin/capped");
    let mut cmd = if capped.exists() {
        let mut c = Command::new(capped);
        c.args(["10G", "cargo"]);
        c
    } else {
        Command::new("cargo")
    };
    let out = cmd
        .args([
            "build",
            "--profile",
            "play",
            "-p",
            "nv-viewer",
            "--bin",
            "nv-viewer",
        ])
        .current_dir(wt)
        .env("CARGO_TARGET_DIR", &req.target)
        .output()
        .map_err(|e| format!("cargo: {e}"))?;
    let built = req.target.join("play").join("nv-viewer");
    if !out.status.success() || !built.exists() {
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let tail: String = text
            .chars()
            .rev()
            .take(300)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        return Err(format!("failed: {}", tail.trim()));
    }
    if let Some(p) = exe.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    std::fs::copy(&built, &exe).map_err(|e| e.to_string())?;
    Ok(("built".into(), exe))
}

/// Builds, tests and benches every ref and compares each with the baseline. Writes `matrix.md` and `matrix.json` in `req.out`.
pub fn matrix(
    req: &MatrixRequest,
    launcher: &dyn Launcher,
    say: Say,
) -> Result<MatrixResult, String> {
    let baseline = req
        .baseline
        .clone()
        .or_else(|| req.refs.first().cloned())
        .ok_or("no refs: give at least one")?;
    let mut refs = req.refs.clone();
    if !refs.contains(&baseline) {
        refs.insert(0, baseline.clone());
    }
    std::fs::create_dir_all(&req.out).map_err(|e| format!("{}: {e}", req.out.display()))?;
    let base_sha = resolve_ref(&req.repo, &baseline)?.1;
    let mut results: Vec<RefResult> = Vec::new();
    for r in &refs {
        let (label, sha) = resolve_ref(&req.repo, r)?;
        say(&format!("== {r} ({label})"));
        let mut res = RefResult {
            requested: r.clone(),
            label: label.clone(),
            sha: sha.clone(),
            build: String::new(),
            test: "skipped".into(),
            headlines: Vec::new(),
            bench_ok: true,
            bench_note: String::new(),
        };
        let wt = match &sha {
            Some(s) => ensure_worktree(&req.repo, &label, s)?,
            None => req.repo.clone(),
        };
        let exe = match build(req, &wt, sha.as_deref()) {
            Ok((how, exe)) => {
                res.build = how;
                Some(exe)
            }
            Err(e) => {
                res.build = e;
                None
            }
        };
        if exe.is_some() && req.test {
            let dev = wt.join("scripts/fork/local/nv-rs-dev");
            let ok = Command::new(&dev)
                .arg("ci-lite")
                .arg(base_sha.as_deref().unwrap_or("origin/fork"))
                .env("NV_DEV_REPO", &wt)
                .env("NV_DEV_TARGET", &req.target)
                .output()
                .is_ok_and(|o| o.status.success());
            res.test = if ok { "pass" } else { "fail" }.into();
        }
        if let (Some(exe), true) = (exe, req.bench) {
            say(&format!("-- bench {} / {r}", req.suite));
            let mut start = Start::new(
                exe,
                req.game.clone(),
                PathBuf::new(),
                req.out.join("runs").join(safe(&label)),
            );
            start.userdata = req.userdata.clone();
            start.gpu_sysfs = req.gpu_sysfs.clone();
            let mut b = BenchRequest::new(start, req.lock.clone());
            b.suite = Some(req.suite.clone());
            b.state = Some(req.out.join("state"));
            match tool::bench(&b, launcher, say) {
                Ok(r) => {
                    res.bench_ok = r.ok;
                    res.headlines = r.headlines;
                }
                Err(e) => {
                    res.bench_ok = false;
                    res.bench_note = e;
                }
            }
        }
        results.push(res);
    }
    let base = results
        .iter()
        .find(|r| r.requested == baseline)
        .map(|r| r.headlines.clone())
        .unwrap_or_default();
    let mut judged = Vec::new();
    let mut ok = true;
    for r in results.iter().filter(|r| r.requested != baseline) {
        let rows = judge(&base, &r.headlines);
        ok &= rows.iter().all(|j| j.verdict != "REGRESSION");
        judged.push((r.requested.clone(), rows));
    }
    for r in &results {
        ok &= !r.build.starts_with("failed") && r.test != "fail" && r.bench_ok;
    }
    let markdown = report(req, &baseline, &results, &judged, ok);
    std::fs::write(req.out.join("matrix.md"), &markdown).map_err(|e| e.to_string())?;
    std::fs::write(
        req.out.join("matrix.json"),
        json(&baseline, &results, &judged, ok),
    )
    .map_err(|e| e.to_string())?;
    Ok(MatrixResult {
        ok,
        refs: results,
        judged,
        markdown,
        dir: req.out.clone(),
    })
}

fn report(
    req: &MatrixRequest,
    baseline: &str,
    results: &[RefResult],
    judged: &[(String, Vec<Judged>)],
    ok: bool,
) -> String {
    let mut s = format!(
        "# Matrix: {} on suite {}\n\nBaseline: {baseline}. Result: **{}**\n\n| ref | build | tests | bench |\n| --- | --- | --- | --- |\n",
        results.iter().map(|r| r.label.as_str()).collect::<Vec<_>>().join(", "),
        req.suite,
        if ok { "OK" } else { "FAILED" }
    );
    for r in results {
        let bench = if !req.bench {
            "skipped".to_string()
        } else if r.bench_note.is_empty() {
            if r.bench_ok {
                "ok".into()
            } else {
                "checks failed".into()
            }
        } else {
            format!("error: {}", r.bench_note)
        };
        s.push_str(&format!(
            "| {} | {} | {} | {bench} |\n",
            r.label, r.build, r.test
        ));
    }
    for (name, rows) in judged {
        s.push_str(&format!("\n## {name} against {baseline}\n\n| scenario | metric | baseline | candidate | change | verdict |\n| --- | --- | --- | --- | --- | --- |\n"));
        for j in rows {
            s.push_str(&format!(
                "| {} | {} | {:.2} | {:.2} | {:+.1}% | {} |\n",
                j.scenario, j.metric, j.base, j.cand, j.change_pct, j.verdict
            ));
        }
    }
    s
}

fn json(
    baseline: &str,
    results: &[RefResult],
    judged: &[(String, Vec<Judged>)],
    ok: bool,
) -> String {
    use crate::bench::esc;
    let refs: Vec<String> = results
        .iter()
        .map(|r| {
            format!(
                "{{\"ref\":{},\"label\":{},\"build\":{},\"test\":{},\"bench_ok\":{}}}",
                esc(&r.requested),
                esc(&r.label),
                esc(&r.build),
                esc(&r.test),
                r.bench_ok
            )
        })
        .collect();
    let rows: Vec<String> = judged
        .iter()
        .flat_map(|(name, rows)| {
            rows.iter().map(move |j| {
                format!(
                    "{{\"ref\":{},\"scenario\":{},\"metric\":{},\"base\":{},\"cand\":{},\"change_pct\":{:.3},\"verdict\":{}}}",
                    esc(name),
                    esc(&j.scenario),
                    esc(j.metric),
                    j.base,
                    j.cand,
                    j.change_pct,
                    esc(j.verdict)
                )
            })
        })
        .collect();
    format!(
        "{{\"schema\":\"nv-rs-scene-matrix/1\",\"baseline\":{},\"ok\":{ok},\"refs\":[{}],\"rows\":[{}]}}\n",
        esc(baseline),
        refs.join(","),
        rows.join(",")
    )
}

/// The size of a folder in bytes (symlinks are not followed).
fn du(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_dir() {
        std::fs::read_dir(path)
            .map(|d| d.flatten().map(|e| du(&e.path())).sum())
            .unwrap_or(0)
    } else {
        meta.len()
    }
}

/// `nv-rs-scene clean`: removes the `matrix-` worktrees and the matrix binary cache (with `all`: the shared target folder and
/// the matrix reports too). Returns one line per thing, with its size; nothing is removed unless `apply`. Never touches a
/// worktree without the prefix.
pub fn clean(
    repo: &Path,
    bin_cache: &Path,
    target: &Path,
    reports: &Path,
    all: bool,
    apply: bool,
) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();
    let clone = main_clone(repo)?;
    let base = worktree_base(repo)?;
    if let Ok(dir) = std::fs::read_dir(&base) {
        for e in dir.flatten() {
            if e.file_name().to_string_lossy().starts_with(WT_PREFIX) {
                let size = du(&e.path());
                if apply {
                    let _ = git(
                        &clone,
                        &[
                            "worktree",
                            "remove",
                            "--force",
                            &e.path().display().to_string(),
                        ],
                    );
                }
                lines.push(format!(
                    "worktree {}: {} MiB",
                    e.path().display(),
                    size >> 20
                ));
            }
        }
    }
    if apply {
        let _ = git(&clone, &["worktree", "prune"]);
    }
    let mut dirs = vec![bin_cache];
    if all {
        dirs.push(target);
        dirs.push(reports);
    }
    for d in dirs {
        if d.exists() {
            lines.push(format!("{}: {} MiB", d.display(), du(d) >> 20));
            if apply {
                std::fs::remove_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
            }
        }
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(scenario: &str, fps: f64, low: f64, p99: f64) -> Headline {
        Headline {
            scenario: scenario.into(),
            run: "r".into(),
            fps,
            p50_ms: 1.0,
            p99_ms: p99,
            low1_fps: low,
            low01_fps: low,
            capped: None,
            wall_s: 1.0,
            verdict: "ok",
            checks_ok: true,
            noisy: false,
        }
    }

    #[test]
    fn a_candidate_regresses_past_the_limit_and_the_floor_and_not_below_them() {
        let base = [head("a", 100.0, 60.0, 10.0)];
        // 4% slower: under the 5% limit. 1% low 20% worse: over its 10%.
        let rows = judge(&base, &[head("a", 96.0, 48.0, 10.2)]);
        let verdict = |m: &str| rows.iter().find(|r| r.metric == m).unwrap().verdict;
        assert_eq!(verdict("avg fps"), "ok");
        assert_eq!(verdict("1% low fps"), "REGRESSION");
        // p99 up 2% (0.2 ms): under the limit and under the 0.3 ms floor.
        assert_eq!(verdict("p99 ms"), "ok");
        let better = judge(&base, &[head("a", 120.0, 60.0, 8.0)]);
        assert_eq!(
            better
                .iter()
                .find(|r| r.metric == "avg fps")
                .unwrap()
                .verdict,
            "better"
        );
        // A scenario only one has is not compared.
        assert!(judge(&base, &[head("b", 1.0, 1.0, 1.0)]).is_empty());
    }

    #[test]
    fn a_report_names_the_baseline_and_the_verdict_of_every_row() {
        let base = [head("a", 100.0, 60.0, 10.0)];
        let rows = judge(&base, &[head("a", 80.0, 60.0, 10.0)]);
        let req = MatrixRequest {
            repo: PathBuf::new(),
            refs: vec![],
            baseline: None,
            suite: "quick".into(),
            game: PathBuf::new(),
            lock: PathBuf::new(),
            out: PathBuf::new(),
            bin_cache: PathBuf::new(),
            target: PathBuf::new(),
            test: false,
            bench: true,
            userdata: None,
            gpu_sysfs: PathBuf::new(),
        };
        let res = |name: &str| RefResult {
            requested: name.into(),
            label: format!("{name}@1"),
            sha: None,
            build: "built".into(),
            test: "skipped".into(),
            headlines: vec![],
            bench_ok: true,
            bench_note: String::new(),
        };
        let md = report(
            &req,
            "fork",
            &[res("fork"), res("pr9")],
            &[("pr9".into(), rows.clone())],
            false,
        );
        assert!(md.contains("Baseline: fork. Result: **FAILED**"), "{md}");
        assert!(
            md.contains("| a | avg fps | 100.00 | 80.00 | -20.0% | REGRESSION |"),
            "{md}"
        );
        let j = json("fork", &[res("fork")], &[("pr9".into(), rows)], false);
        assert!(
            j.contains("\"ok\":false") && j.contains("\"verdict\":\"REGRESSION\""),
            "{j}"
        );
    }

    #[test]
    fn clean_lists_without_removing_unless_applied_and_keeps_worktrees_without_the_prefix() {
        let t = std::env::temp_dir().join(format!("fork-scene-clean-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let repo = t.join("owner/repo");
        std::fs::create_dir_all(&repo).unwrap();
        let g = |a: &[&str]| {
            assert!(Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(a)
                .output()
                .unwrap()
                .status
                .success())
        };
        g(&["init", "-q", "-b", "main"]);
        g(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "x",
        ]);
        let wts = t.join("owner/worktrees/repo");
        g(&[
            "worktree",
            "add",
            "-q",
            "--detach",
            &wts.join("matrix-a@1").display().to_string(),
        ]);
        g(&[
            "worktree",
            "add",
            "-q",
            "--detach",
            &wts.join("mine").display().to_string(),
        ]);
        let cache = t.join("bin");
        std::fs::create_dir_all(&cache).unwrap();
        let dry = clean(
            &repo,
            &cache,
            &t.join("target"),
            &t.join("reports"),
            false,
            false,
        )
        .unwrap();
        assert_eq!(dry.len(), 2, "{dry:?}");
        assert!(wts.join("matrix-a@1").exists() && cache.exists());
        let done = clean(
            &repo,
            &cache,
            &t.join("target"),
            &t.join("reports"),
            false,
            true,
        )
        .unwrap();
        assert_eq!(done.len(), 2);
        assert!(!wts.join("matrix-a@1").exists() && !cache.exists());
        assert!(
            wts.join("mine").exists(),
            "a worktree without the prefix is never touched"
        );
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn labels_are_safe_names_and_a_pull_request_number_is_recognised() {
        assert_eq!(safe("feat/x y"), "feat-x-y");
        let t = std::env::temp_dir().join(format!("fork-scene-matrix-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        let g = |a: &[&str]| {
            assert!(Command::new("git")
                .arg("-C")
                .arg(&t)
                .args(a)
                .output()
                .unwrap()
                .status
                .success())
        };
        g(&["init", "-q", "-b", "main"]);
        g(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "x",
        ]);
        let (label, sha) = resolve_ref(&t, "main").unwrap();
        assert!(label.starts_with("main@") && sha.is_some());
        assert!(resolve_ref(&t, "working")
            .unwrap()
            .0
            .starts_with("working@"));
        assert!(resolve_ref(&t, "nonesuch")
            .unwrap_err()
            .contains("unknown ref"));
        assert!(resolve_ref(&t, "#99999")
            .unwrap_err()
            .contains("pull request 99999"));
        std::fs::remove_dir_all(&t).unwrap();
    }
}
