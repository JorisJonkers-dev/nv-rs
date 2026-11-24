// SPDX-License-Identifier: GPL-3.0-only
//! The scene tool as a library (docs/fork/SCENE.md, "Using this from another program").
//!
//! One public function per command of `nv-rs-scene`, each taking a typed request and returning a typed result:
//!
//! | command | function | request | result |
//! | --- | --- | --- | --- |
//! | `compare` | [`compare`] | [`CompareRequest`] | [`CompareResult`] |
//! | `run` | [`run`] | [`RunRequest`] | [`RunResult`] |
//! | `soak` | [`soak`] | [`SoakRequest`] | [`SoakResult`] |
//! | `sweep` | [`sweep`] | [`SweepRequest`] | [`SweepResult`] |
//! | `bench` | [`bench`] | [`BenchRequest`] | [`BenchResult`] |
//! | `bench-compare` | [`bench_compare`] | [`BenchCompareRequest`] | [`BenchCompareResult`] |
//!
//! Nothing here prints or ends the process. What the command-line tool prints while it works goes to the `say`
//! closure the caller gives (a line at a time; pass `&|_| {}` to ignore it); what it prints at the end is a method of
//! the result (`summary`). The engine start is a hook: the caller gives a [`Launcher`] that turns a [`StartSpec`] (the
//! program, the folder, the environment, the lock to hold) into a running child. [`CappedLauncher`] is the
//! command-line tool's own (a `gamerun` scope with a memory limit, optionally behind `flock`); a program that starts
//! the game as its own child with its own label and memory cap writes its own.
//!
//! ```no_run
//! use fork_scene::tool::{self, CappedLauncher, SoakRequest, Start};
//! use std::path::PathBuf;
//!
//! let start = Start::new(
//!     PathBuf::from("/opt/nv-rs/nv-viewer"),
//!     PathBuf::from("/games/Fallout New Vegas"),
//!     PathBuf::from("places.toml"),
//!     PathBuf::from("/tmp/soak-out"),
//! );
//! let mut request = SoakRequest::new(start);
//! request.frames = 300;
//! let launcher = CappedLauncher::default(); // a gamerun scope, 4 to 6 GiB
//! let result = tool::soak(&request, &launcher, &|line| eprintln!("{line}")).unwrap();
//! assert!(result.ok, "{}", result.summary());
//! ```

use crate::diff::{
    compare as compare_pictures, compare_without_noise, decode_png, Difference, Picture,
};
use std::path::{Path, PathBuf};

/// Where a command says what it is doing, a line at a time. Called from several threads at once by `run`.
pub type Say<'a> = &'a (dyn Fn(&str) + Sync);

// ---------------------------------------------------------------------------------------------------------------------
// compare

/// `compare`: the difference between two pictures, or between the same-named PNGs of two folders.
#[derive(Clone, Debug, Default)]
pub struct CompareRequest {
    /// Two PNG files, or two folders.
    pub a: PathBuf,
    pub b: PathBuf,
    /// The folder of a second run of `a`: pixels that differ between the two runs are left out of the numbers.
    pub noise: Option<PathBuf>,
    /// A pair whose mean difference is above this does not pass.
    pub max_mean: Option<f64>,
    /// A folder to write a difference picture per pair into (two folders, no noise run).
    pub diff_out: Option<PathBuf>,
}

/// How one pair came out.
#[derive(Clone, Debug, PartialEq)]
pub enum RowOutcome {
    Compared(Difference),
    /// The picture is in `a` but not in `b`.
    MissingInB(PathBuf),
}

/// One picture (or pair of pictures) of a comparison.
#[derive(Clone, Debug, PartialEq)]
pub struct CompareRow {
    /// The file name (or the path, for two single files).
    pub name: String,
    /// With a noise run: the share of pixels (percent) that differs between the two runs of `a` and was left out.
    pub masked_pct: Option<f64>,
    pub outcome: RowOutcome,
    /// False when the pair is missing or its mean is above the request's `max_mean`.
    pub pass: bool,
}

impl CompareRow {
    /// The line the command-line tool prints for this row.
    pub fn line(&self) -> String {
        match (&self.outcome, self.masked_pct) {
            (RowOutcome::MissingInB(dir), _) => {
                format!("{}: missing in {}", self.name, dir.display())
            }
            (RowOutcome::Compared(d), masked) => {
                let name = match masked {
                    Some(m) => format!("{} (without the {m:.2}% that differs between the two runs of the first folder)", self.name),
                    None => self.name.clone(),
                };
                format!(
                    "{name}: mean {:.3}, max {}, changed {:.3}% of pixels (channel difference above {})",
                    d.mean,
                    d.max,
                    d.changed_pct,
                    crate::diff::THRESHOLD
                )
            }
        }
    }
}

/// The rows of a comparison.
#[derive(Clone, Debug, PartialEq)]
pub struct CompareResult {
    /// Every pair was there and under `max_mean` (when given).
    pub ok: bool,
    pub rows: Vec<CompareRow>,
}

impl CompareResult {
    /// The rows' lines, one per row.
    pub fn summary(&self) -> String {
        self.rows
            .iter()
            .map(CompareRow::line)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn load(p: &Path) -> Result<Picture, String> {
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

/// Compares two pictures or two folders of pictures. An unreadable picture is an `Err`; a missing one is a failing row.
pub fn compare(req: &CompareRequest) -> Result<CompareResult, String> {
    let mut rows = Vec::new();
    let pass = |d: &Difference| req.max_mean.is_none_or(|m| d.mean <= m);
    if req.a.is_dir() && req.b.is_dir() {
        for pa in pngs(&req.a)? {
            let name = pa
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let pb = req.b.join(&name);
            if !pb.exists() {
                rows.push(CompareRow {
                    name,
                    masked_pct: None,
                    outcome: RowOutcome::MissingInB(req.b.clone()),
                    pass: false,
                });
                continue;
            }
            let (pic_a, pic_b) = (load(&pa)?, load(&pb)?);
            if let Some(n) = &req.noise {
                let again = load(&n.join(&name))?;
                let (d, masked) = compare_without_noise(&pic_a, &again, &pic_b)?;
                rows.push(CompareRow {
                    name,
                    masked_pct: Some(masked),
                    pass: pass(&d),
                    outcome: RowOutcome::Compared(d),
                });
                continue;
            }
            let d = compare_pictures(&pic_a, &pic_b)?;
            if let Some(dir) = &req.diff_out {
                let (w, h, rgb) = crate::diff::difference_picture(&pic_a, &pic_b)?;
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                let png = crate::png::encode_rgb(w, h, &rgb)?;
                std::fs::write(dir.join(&name), png).map_err(|e| e.to_string())?;
            }
            rows.push(CompareRow {
                name,
                masked_pct: None,
                pass: pass(&d),
                outcome: RowOutcome::Compared(d),
            });
        }
    } else {
        let d = compare_pictures(&load(&req.a)?, &load(&req.b)?)?;
        rows.push(CompareRow {
            name: req.a.display().to_string(),
            masked_pct: None,
            pass: pass(&d),
            outcome: RowOutcome::Compared(d),
        });
    }
    Ok(CompareResult {
        ok: rows.iter().all(|r| r.pass),
        rows,
    })
}

// ---------------------------------------------------------------------------------------------------------------------
// light

/// `light`: how the light on the ground of the same-named pictures of two folders compares (the light balance's
/// picture case, nv-rs#768): per picture the ratios B / A of [`crate::diff::Light`], and their means.
#[derive(Clone, Debug, Default)]
pub struct LightRequest {
    pub a: PathBuf,
    pub b: PathBuf,
    /// The mean sunlit ratio passes when it is within this many percent of 1.
    pub within_pct: Option<f64>,
    /// The detail guard (nv-rs#772): a picture fails when B has more than this many percentage
    /// points more blocks that show no visible detail than A ([`crate::diff::no_detail_pct`]). A
    /// dark picture passes as long as shapes and texture can still be seen.
    pub max_lost_detail: Option<f64>,
}

/// One picture's ratios B / A.
#[derive(Clone, Debug, PartialEq)]
pub struct LightRow {
    pub name: String,
    pub sunlit: f64,
    pub shade: f64,
    pub band: f64,
    /// Blocks without visible detail, percent, in A and in B.
    pub no_detail: (f64, f64),
    /// False when the gain in such blocks is above the request's limit.
    pub readable: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LightResult {
    pub rows: Vec<LightRow>,
    /// The means of the rows' ratios.
    pub sunlit: f64,
    pub shade: f64,
    pub band: f64,
    /// Every picture of A was in B and the mean sunlit ratio is within the request's limit (when given).
    pub ok: bool,
}

impl LightResult {
    pub fn summary(&self) -> String {
        let mut lines: Vec<String> = self
            .rows
            .iter()
            .map(|r| {
                format!(
                    "{}: sunlit {:.3}, shade {:.3}, ground {:.3}, no detail {:.1}% -> {:.1}%{}",
                    r.name,
                    r.sunlit,
                    r.shade,
                    r.band,
                    r.no_detail.0,
                    r.no_detail.1,
                    if r.readable { "" } else { " DETAIL LOST" }
                )
            })
            .collect();
        lines.push(format!(
            "mean of {} pictures (B / A, linear light of the lower 45%): sunlit {:.3}, shade {:.3}, ground {:.3}",
            self.rows.len(),
            self.sunlit,
            self.shade,
            self.band
        ));
        lines.join("\n")
    }
}

/// Compares the light of two folders' pictures. A picture of A missing in B fails the result.
pub fn light(req: &LightRequest) -> Result<LightResult, String> {
    let ratio = |b: f64, a: f64| if a > 0.0 { b / a } else { f64::NAN };
    let mut rows = Vec::new();
    let mut missing = false;
    for pa in pngs(&req.a)? {
        let name = pa
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let pb = req.b.join(&name);
        if !pb.exists() {
            missing = true;
            continue;
        }
        let (pic_a, pic_b) = (load(&pa)?, load(&pb)?);
        let (la, lb) = (crate::diff::light(&pic_a)?, crate::diff::light(&pic_b)?);
        let no_detail = (
            crate::diff::no_detail_pct(&pic_a),
            crate::diff::no_detail_pct(&pic_b),
        );
        rows.push(LightRow {
            name,
            sunlit: ratio(lb.sunlit, la.sunlit),
            shade: ratio(lb.shade, la.shade),
            band: ratio(lb.band, la.band),
            no_detail,
            readable: req
                .max_lost_detail
                .is_none_or(|gain| no_detail.1 - no_detail.0 <= gain),
        });
    }
    if rows.is_empty() {
        return Err(format!(
            "no picture of {} is in {}",
            req.a.display(),
            req.b.display()
        ));
    }
    let mean = |f: fn(&LightRow) -> f64| {
        let v: Vec<f64> = rows.iter().map(f).filter(|x| x.is_finite()).collect();
        v.iter().sum::<f64>() / v.len().max(1) as f64
    };
    let (sunlit, shade, band) = (mean(|r| r.sunlit), mean(|r| r.shade), mean(|r| r.band));
    let within = req
        .within_pct
        .is_none_or(|pct| (sunlit - 1.0).abs() * 100.0 <= pct);
    Ok(LightResult {
        ok: !missing && within && rows.iter().all(|r| r.readable),
        rows,
        sunlit,
        shade,
        band,
    })
}

// ---------------------------------------------------------------------------------------------------------------------
// the engine start: the hook

/// The shared (pictures, soaks, sweeps) or exclusive (bench) lock a start must hold while the engine runs.
#[derive(Clone, Debug, PartialEq)]
pub struct GameLock {
    pub path: PathBuf,
    pub exclusive: bool,
}

/// Everything a launcher needs to start one engine: the library has staged the game folder and decided the environment.
#[derive(Clone, Debug)]
pub struct StartSpec {
    /// `run`, `soak` or `bench`: the default label of a start.
    pub kind: String,
    /// The variant's name (`soak` for a soak or bench).
    pub name: String,
    /// The viewer program.
    pub viewer: PathBuf,
    /// Its one argument: the staged game folder, also the working directory.
    pub game_dir: PathBuf,
    /// The environment to set (later entries replace earlier ones of the same name; none repeats).
    pub env: Vec<(String, String)>,
    /// Engine starts running at once in this invocation (a launcher that caps memory shares its budget by it).
    pub concurrent: usize,
    /// The lock to hold while the engine runs.
    pub lock: Option<GameLock>,
}

/// A started engine.
pub struct Launched {
    pub child: std::process::Child,
    /// The start of the name of the systemd scope the engine runs in (`nvgame-<label>-`), when it runs in one: the
    /// library reads the scope's memory while the engine runs and kills the scope when the engine hangs.
    pub scope_prefix: Option<String>,
}

/// Starts engines. `log` is the file the engine's standard output and error go to.
pub trait Launcher: Sync {
    /// One line for the start of a run: what limits the engine's memory.
    fn describe(&self, concurrent: usize) -> String;
    /// Starts the engine of `spec` in `spec.game_dir` with `spec.env`, holding `spec.lock`.
    fn launch(&self, spec: &StartSpec, log: std::fs::File) -> Result<Launched, String>;
}

/// The memory cap of every engine start: each start is a `gamerun` scope (a systemd scope in `nv-games.slice` with a
/// hard `MemoryMax` and no swap, labelled in the monitor) unless `off`. The default limit is a share of a total budget
/// ([`TOTAL_BUDGET_MIB`]), `mem` overrides it.
#[derive(Clone, Debug, Default)]
pub struct Cap {
    /// The base of the scope's name (a run adds its variant's name); empty: `scene-<kind>-<pid>`.
    pub label: String,
    pub who: String,
    /// `gamerun --mem` value such as `10G`; empty: the default share.
    pub mem: String,
    pub off: bool,
}

/// The default memory budget of one invocation (MiB), shared by the starts that run together: the owner's desktop once
/// failed from about 12 GiB in one process, so no run is allowed more in all by default. `mem` overrides it per start.
pub const TOTAL_BUDGET_MIB: u64 = 12288;
/// What one start needs at least (MiB): the cap never goes below it. Measured peaks are about 1.7 GiB (the plain game) and
/// 1.9 GiB (a Tale of Two Wastelands folder), docs/fork/SCENE.md: twice that.
pub const ONE_START_MIB: u64 = 4096;
/// The most one start gets by default (MiB): three times the measured peak, so a start that grows out of bounds is
/// stopped long before the desktop notices.
pub const ONE_START_MAX_MIB: u64 = 6144;

impl Cap {
    /// The words that put a command in the scope, for the start named `name` of a run of `kind`; empty when the
    /// cap is off or the wrap words already hold `gamerun`.
    pub fn words(
        &self,
        kind: &str,
        name: &str,
        wrap: &[String],
        concurrent: usize,
    ) -> Result<Vec<String>, String> {
        if self.off || wrap.iter().any(|w| w.ends_with("gamerun")) {
            return Ok(Vec::new());
        }
        let home = std::env::var("HOME").unwrap_or_default();
        let gamerun = format!("{home}/.local/bin/gamerun");
        if !Path::new(&gamerun).exists() {
            return Err(format!(
                "{gamerun} is not there: a game run is memory-capped by default; install it or give --no-cap"
            ));
        }
        let clean = |t: &str| {
            t.chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                        c
                    } else {
                        '-'
                    }
                })
                .collect::<String>()
        };
        let label = if self.label.is_empty() {
            format!("scene-{kind}-{}-{}", clean(name), std::process::id())
        } else if name == kind {
            clean(&self.label)
        } else {
            format!("{}-{}", clean(&self.label), clean(name))
        };
        let mut w = vec![gamerun, label, "--who".into()];
        w.push(if self.who.is_empty() {
            "engine".into()
        } else {
            self.who.clone()
        });
        w.push("--mem".into());
        w.push(self.limit(concurrent));
        w.push("--".into());
        Ok(w)
    }

    /// The limit of each start (gamerun's `--mem` value): `mem` as given, else the total budget shared by the starts
    /// that run together, never below what one start needs.
    pub fn limit(&self, concurrent: usize) -> String {
        if !self.mem.is_empty() {
            return self.mem.clone();
        }
        let n = concurrent.max(1) as u64;
        format!(
            "{}M",
            (TOTAL_BUDGET_MIB / n).clamp(ONE_START_MIB, ONE_START_MAX_MIB)
        )
    }

    /// The line printed at the start of a run.
    pub fn describe(&self, wrap: &[String], concurrent: usize) -> String {
        if self.off {
            "memory cap: OFF (--no-cap): the engine can use all the memory of the machine".into()
        } else if wrap.iter().any(|w| w.ends_with("gamerun")) {
            "memory cap: the --wrap words start gamerun, which holds the cap".into()
        } else {
            let per = self.limit(concurrent);
            let how = if self.mem.is_empty() {
                format!(
                    "the budget of {TOTAL_BUDGET_MIB} MiB for the whole invocation shared by {} start(s) at once, between {ONE_START_MIB} and {ONE_START_MAX_MIB} MiB each",
                    concurrent.max(1)
                )
            } else {
                "from --mem".to_string()
            };
            format!(
                "memory cap: {per} per engine start ({how}; a gamerun scope in nv-games.slice, hard limit, no swap)"
            )
        }
    }
}

/// The command-line tool's launcher: `flock` for the lock, then `gamerun` for the memory cap, then the viewer.
#[derive(Clone, Debug, Default)]
pub struct CappedLauncher {
    pub cap: Cap,
    /// Words put in front of the command (for example `flock -s FILE`), before the cap's.
    pub wrap: Vec<String>,
}

impl Launcher for CappedLauncher {
    fn describe(&self, concurrent: usize) -> String {
        self.cap.describe(&self.wrap, concurrent)
    }

    fn launch(&self, spec: &StartSpec, log: std::fs::File) -> Result<Launched, String> {
        let mut words: Vec<std::ffi::OsString> = Vec::new();
        if let Some(lock) = &spec.lock {
            words.push("flock".into());
            words.push(if lock.exclusive { "-x" } else { "-s" }.into());
            words.push((&lock.path).into());
        }
        words.extend(self.wrap.iter().map(Into::into));
        let cap_words = self
            .cap
            .words(&spec.kind, &spec.name, &self.wrap, spec.concurrent)?;
        words.extend(cap_words.iter().cloned().map(std::ffi::OsString::from));
        words.push((&spec.viewer).into());
        words.push((&spec.game_dir).into());
        let mut cmd = std::process::Command::new(&words[0]);
        cmd.args(&words[1..])
            .current_dir(&spec.game_dir)
            .stdout(log.try_clone().map_err(|e| e.to_string())?)
            .stderr(log);
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        let child = cmd
            .spawn()
            .map_err(|e| format!("{}: {e}", words[0].to_string_lossy()))?;
        Ok(Launched {
            child,
            scope_prefix: cap_words.get(1).map(|l| format!("nvgame-{l}-")),
        })
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// what every command that starts an engine takes

/// What every engine start of `run`, `soak`, `sweep` and `bench` shares.
#[derive(Clone, Debug)]
pub struct Start {
    /// The viewer binary (`nv-viewer`).
    pub viewer: PathBuf,
    /// The game's install folder (never changed: each start works on a staged copy of its structure).
    pub game: PathBuf,
    /// The place file.
    pub places: PathBuf,
    /// Where everything is written: `<out>/<variant>/` for a run, `<out>/soak/` for the others.
    pub out: PathBuf,
    /// A folder to copy the user data from (settings, saves).
    pub userdata: Option<PathBuf>,
    /// The plugin list, in load order; empty: the plugins found at the top of the first overlay, sorted.
    pub plugins: Vec<String>,
    /// Top-level entries of an overlay left out (such as `meshes`, `textures`, `lodsettings`).
    pub overlay_skip: Vec<String>,
    /// Environment of the engine, after the tool's own (a later entry replaces an earlier one).
    pub env: Vec<(String, String)>,
    /// Ini lines rewritten in the staged copy before the start (`uGridsToLoad=7`), in `Fallout.ini` and
    /// `FalloutPrefs.ini`, whichever has the key (`crate::stage::set_ini`).
    pub ini: Vec<(String, String)>,
    /// Where the card's memory is read for the start check (default `/sys/class/drm`).
    pub gpu_sysfs: PathBuf,
    /// Seconds the whole scene may take inside the engine.
    pub timeout_s: u32,
    /// Only these places of the file (empty: all).
    pub only: Vec<String>,
    /// The lock to hold while the engine runs (shared; a bench takes it exclusively).
    pub game_lock: Option<PathBuf>,
}

impl Start {
    pub fn new(viewer: PathBuf, game: PathBuf, places: PathBuf, out: PathBuf) -> Start {
        Start {
            viewer,
            game,
            places,
            out,
            userdata: None,
            plugins: Vec::new(),
            overlay_skip: Vec::new(),
            env: Vec::new(),
            ini: Vec::new(),
            gpu_sysfs: PathBuf::from("/sys/class/drm"),
            timeout_s: 120,
            only: Vec::new(),
            game_lock: None,
        }
    }
}

/// A named set of data laid over the game folder.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Variant {
    pub name: String,
    /// Overlay folders, the first staged with the game, later ones laid over it.
    pub overlays: Vec<PathBuf>,
    /// Environment of this variant's start only, over the request's (`--variant-env`): the same
    /// places drawn with other engine settings, such as a graphics preset (`NV_GFX_PRESET`).
    pub env: Vec<(String, String)>,
}

/// The exit status [`SoakResult::exit_status`] holds for a start the tool killed because its log went silent.
pub const HUNG: i32 = -99;

/// The exit status [`SoakResult::exit_status`] holds for a start the memory guard stopped.
pub const GUARD_STOPPED: i32 = -98;

/// How an engine start ended.
#[derive(Clone, Debug, PartialEq)]
pub enum StartEnd {
    Exit(i32),
    /// The engine's log stayed silent for the stall limit and the start was killed.
    Hung,
    /// The memory guard stopped the start (the card's use neared its cap, the session was in trouble, or the engine
    /// refused an allocation past its budget): the reason. The bench runs are halted until `guard clear-halt`.
    Stopped(String),
}

struct Opts<'a> {
    kind: &'static str,
    concurrent: usize,
    start: &'a Start,
    settle: u32,
    resolution: &'a str,
    env: Vec<(String, String)>,
    stall_s: u64,
    progress: Option<&'a Path>,
    pictures: bool,
    lock: Option<GameLock>,
}

/// Sets `key` in an ordered environment list, replacing an earlier entry.
fn put(env: &mut Vec<(String, String)>, key: &str, value: &str) {
    env.retain(|(k, _)| k != key);
    env.push((key.to_string(), value.to_string()));
}

fn env_value(env: &[(String, String)], key: &str, default: u32) -> u32 {
    env.iter()
        .rev()
        .find(|(k, _)| k == key)
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(default)
}

/// Appends a table row to the sweep report for each place the engine has soaked since `offset` (the log's bytes already
/// read); lines not yet complete are left for the next call.
fn progress_poll(report: &Path, log: &Path, offset: &mut u64) {
    use std::io::{Read, Seek, SeekFrom, Write};
    let Ok(mut f) = std::fs::File::open(log) else {
        return;
    };
    if f.seek(SeekFrom::Start(*offset)).is_err() {
        return;
    }
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        return;
    }
    let Some(end) = buf.iter().rposition(|b| *b == b'\n') else {
        return;
    };
    *offset += end as u64 + 1;
    let mut rows = String::new();
    for line in String::from_utf8_lossy(&buf[..end]).lines() {
        let Ok(v) = crate::json_lite::fields(line, &["message"]) else {
            continue;
        };
        if let Some(r) = v[0].strip_prefix("scene: soak ") {
            let name = r.split_whitespace().next().unwrap_or("");
            rows.push_str(&format!("| {name} | ok: {r} |\n"));
        }
    }
    if !rows.is_empty() {
        if let Ok(mut out) = std::fs::OpenOptions::new().append(true).open(report) {
            let _ = out.write_all(rows.as_bytes());
        }
    }
}

/// The cache folder a start runs with: one folder kept between starts (`~/.cache/nv-rs/scene-cache`), so the drawing layer's
/// pipeline cache and the driver's shader cache are warm from the second start on (1.4 s of the start, the same pictures);
/// the engine writes its pipeline cache file under a temporary name and renames it, so starts at the same time share it.
/// `NV_RS_SCENE_COLD_CACHE=1` gives each start its own empty folder (`own`), as an `XDG_CACHE_HOME` entry of the request's
/// environment gives it that folder.
fn cache_home(own: &Path) -> PathBuf {
    if std::env::var_os("NV_RS_SCENE_COLD_CACHE").is_some() {
        return own.to_path_buf();
    }
    match std::env::var_os("HOME") {
        Some(home) => {
            let dir = PathBuf::from(home).join(".cache/nv-rs/scene-cache");
            if std::fs::create_dir_all(&dir).is_ok() {
                return dir;
            }
            own.to_path_buf()
        }
        None => own.to_path_buf(),
    }
}

/// The highest memory of a scope seen while it ran: the cgroup's `memory.peak` (it counts the page cache the run read,
/// which the kernel gives back under pressure) and the largest anonymous memory (`memory.stat`, what the process itself holds).
#[derive(Default)]
struct MemSeen {
    peak: u64,
    anon: u64,
    dir: Option<PathBuf>,
}

impl MemSeen {
    fn find(prefix: &str) -> Option<PathBuf> {
        let uid = std::fs::read_to_string("/proc/self/loginuid")
            .ok()?
            .trim()
            .to_string();
        let root = PathBuf::from(format!(
            "/sys/fs/cgroup/user.slice/user-{uid}.slice/user@{uid}.service"
        ));
        let mut stack = vec![(root, 0)];
        while let Some((d, depth)) = stack.pop() {
            for e in std::fs::read_dir(&d).ok()?.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.starts_with(prefix) && n.ends_with(".scope") {
                    return Some(e.path());
                }
                if depth < 3 && n.ends_with(".slice") {
                    stack.push((e.path(), depth + 1));
                }
            }
        }
        None
    }

    fn poll(&mut self, prefix: &str) {
        if self.dir.is_none() {
            self.dir = Self::find(prefix);
        }
        let Some(d) = &self.dir else { return };
        let read = |f: &str| std::fs::read_to_string(d.join(f)).ok();
        if let Some(p) = read("memory.peak").and_then(|t| t.trim().parse::<u64>().ok()) {
            self.peak = self.peak.max(p);
        }
        if let Some(a) = read("memory.stat").and_then(|t| {
            t.lines().find_map(|l| {
                l.strip_prefix("anon ")
                    .and_then(|v| v.trim().parse::<u64>().ok())
            })
        }) {
            self.anon = self.anon.max(a);
        }
    }

    fn describe(&self, name: &str) -> String {
        format!(
            "memory {name}: peak {} MiB of the scope (with the page cache the run read), {} MiB anonymous at most",
            self.peak >> 20,
            self.anon >> 20
        )
    }
}

/// Kills an engine start: its whole scope when it runs in one (so the game process goes too), then the child.
fn kill_start(mem: &MemSeen, child: &mut std::process::Child) {
    if let Some(scope) = mem.dir.as_ref().and_then(|d| d.file_name()) {
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "kill", "--signal=SIGKILL"])
            .arg(scope)
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn start_one(
    name: &str,
    overlays: &[PathBuf],
    variant_env: &[(String, String)],
    o: &Opts,
    launcher: &dyn Launcher,
    say: Say,
) -> Result<StartEnd, String> {
    let start = o.start;
    // The card's part of the memory guard: refused when the runs are halted or the card has no room for the budget.
    let halt_file = crate::guard::halt_path();
    let guarded = crate::guard::start_check(&start.gpu_sysfs, &halt_file)
        .map_err(|e| format!("refused by the memory guard: {e}"))?;
    let gpu = guarded.as_ref().map(|g| g.caps);
    let dir = start.out.join(name);
    // A packed overlay names its plugins: every plugin file at its top.
    let mut plugins = Vec::new();
    for ov in overlays {
        for e in std::fs::read_dir(ov)
            .map_err(|e| format!("{}: {e}", ov.display()))?
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
    if !start.plugins.is_empty() {
        plugins = start.plugins.clone();
    }
    let staged = crate::stage::stage(
        &start.game,
        overlays.first().map(PathBuf::as_path),
        &start.overlay_skip,
        &plugins,
        start.userdata.as_deref(),
        &dir,
    )
    .map_err(|e| format!("staging: {e}"))?;
    for (key, value) in &start.ini {
        crate::stage::set_ini(&staged, key, value).map_err(|e| format!("staging: {e}"))?;
    }
    // Later overlays (a generated LOD folder over a load order's data folder) are laid over the first, all of
    // their files, their plugins after it.
    for later in overlays.iter().skip(1) {
        crate::stage::merge_over(later, &staged.game.join("Data"), &[])
            .map_err(|e| format!("staging {}: {e}", later.display()))?;
    }
    let log = std::fs::File::create(dir.join("run.log")).map_err(|e| e.to_string())?;
    let log_path = dir.join("log.jsonl");
    let mut env: Vec<(String, String)> = Vec::new();
    let mut set = |k: &str, v: &str| put(&mut env, k, v);
    set("XDG_DATA_HOME", &staged.xdg[0].display().to_string());
    set("XDG_CONFIG_HOME", &staged.xdg[1].display().to_string());
    set(
        "XDG_CACHE_HOME",
        &cache_home(&staged.xdg[2]).display().to_string(),
    );
    set("XDG_STATE_HOME", &staged.xdg[3].display().to_string());
    set("NV_D3D9", "wgpu");
    set("NV_D3D9_OFFSCREEN", "1");
    set("NV_SOUND", "out");
    set("NV_SOUND_BACKEND", "null");
    set("NV_RESOLUTION", o.resolution);
    set("NV_THREADS", "real");
    set("NV_LOG_JSON", &log_path.display().to_string());
    set("NV_LOG_TERM", "warn");
    set("NV_SCENE", &start.places.display().to_string());
    set("NV_SCENE_SETTLE_FRAMES", &o.settle.to_string());
    set("NV_SCENE_TIMEOUT_S", &start.timeout_s.to_string());
    if let Some(caps) = &gpu {
        set("NV_GPU_BUDGET_MB", &caps.vram_mib.to_string());
    }
    if !start.only.is_empty() {
        set("NV_SCENE_PLACES", &start.only.join(","));
    }
    for (k, v) in o.env.iter().chain(&start.env).chain(variant_env) {
        set(k, v);
    }
    if o.pictures {
        set("NV_SCENE_OUT", &dir.join("pictures").display().to_string());
    }
    let spec = StartSpec {
        kind: o.kind.to_string(),
        name: name.to_string(),
        viewer: start.viewer.clone(),
        game_dir: staged.game.clone(),
        env,
        concurrent: o.concurrent,
        lock: o.lock.clone(),
    };
    let Launched {
        mut child,
        scope_prefix,
    } = launcher.launch(&spec, log)?;
    // The scope's memory, read while the engine runs (the scope is gone when it ends).
    let mut mem = MemSeen::default();
    let mut watch = guarded.map(crate::guard::Watch::new);
    let mut progress_at = 0u64;
    let mut progress_clock = std::time::Instant::now();
    let mut last_moved = std::time::Instant::now();
    let mut last_len = 0u64;
    let status = loop {
        if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
            break st;
        }
        if let Some(prefix) = &scope_prefix {
            mem.poll(prefix);
        }
        if let Some(w) = watch.as_mut() {
            if let Some(why) = w.step(&dir.join("run.log"), &[child.id()]) {
                kill_start(&mem, &mut child);
                let _ = crate::guard::halt(&halt_file, &why);
                say(&format!(
                    "{name}: memory guard: {why}: the start was stopped; later runs are halted (`nv-rs-scene guard clear-halt`)"
                ));
                return Ok(StartEnd::Stopped(why));
            }
        }
        if let Some(report) = o.progress {
            if progress_clock.elapsed().as_secs() >= 3 {
                progress_clock = std::time::Instant::now();
                progress_poll(report, &log_path, &mut progress_at);
            }
        }
        if o.stall_s > 0 {
            // A hung engine writes nothing: kill the start (its scope, so the game process goes too) when the log has not
            // grown for the stall limit.
            let len = std::fs::metadata(&log_path).map_or(0, |m| m.len());
            if len != last_len {
                last_len = len;
                last_moved = std::time::Instant::now();
            } else if last_moved.elapsed().as_secs() >= o.stall_s {
                kill_start(&mem, &mut child);
                say(&format!(
                    "{name}: hung: the log was silent for {} s, the start was killed",
                    o.stall_s
                ));
                return Ok(StartEnd::Hung);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    if let Some(report) = o.progress {
        progress_poll(report, &log_path, &mut progress_at);
    }
    if mem.peak > 0 {
        let line = mem.describe(name);
        say(&line);
        let _ = std::fs::write(
            dir.join("memory.txt"),
            format!(
                "{line}\npeak_mib {}\nanon_mib {}\n",
                mem.peak >> 20,
                mem.anon >> 20
            ),
        );
    }
    Ok(StartEnd::Exit(status.code().unwrap_or(-1)))
}

// ---------------------------------------------------------------------------------------------------------------------
// run

/// `run`: every variant of the game's data on one place file, one engine start each (in parallel up to `jobs`), then
/// the first variant's pictures against every other's.
#[derive(Clone, Debug)]
pub struct RunRequest {
    pub start: Start,
    /// Empty: one variant named `base` with no overlay.
    pub variants: Vec<Variant>,
    /// Steady frames to wait before each picture.
    pub settle_frames: u32,
    /// `1280x720`.
    pub resolution: String,
    /// Starts at once.
    pub jobs: usize,
    /// Small pictures and a short settle: a look at what a change did, not a picture to judge.
    pub quick: bool,
}

impl RunRequest {
    pub fn new(start: Start) -> RunRequest {
        RunRequest {
            start,
            variants: Vec::new(),
            settle_frames: 120,
            resolution: "1280x720".into(),
            jobs: 3,
            quick: false,
        }
    }
}

/// One variant's start.
#[derive(Debug)]
pub struct VariantResult {
    pub name: String,
    /// How the engine ended, or why it did not start.
    pub end: Result<StartEnd, String>,
    pub seconds: f64,
    /// `<out>/<name>/pictures`.
    pub pictures: PathBuf,
    /// `<out>/<name>/run.log`.
    pub log: PathBuf,
}

/// The result of [`run`].
#[derive(Debug)]
pub struct RunResult {
    /// Every engine ended with status 0 and every picture pair was compared.
    pub ok: bool,
    pub seconds: f64,
    pub variants: Vec<VariantResult>,
    /// The first variant against each other one (only when every start was ok): the other's name and the rows.
    pub comparisons: Vec<(String, CompareResult)>,
}

impl RunResult {
    /// What the command-line tool prints at the end.
    pub fn summary(&self) -> String {
        let mut s = String::new();
        for v in &self.variants {
            match &v.end {
                Ok(StartEnd::Exit(0)) => s.push_str(&format!(
                    "{}: ok, {:.0} s, pictures in {}\n",
                    v.name,
                    v.seconds,
                    v.pictures.display()
                )),
                Ok(StartEnd::Exit(code)) => s.push_str(&format!(
                    "{}: the engine ended with status {code} after {:.0} s (see {})\n",
                    v.name,
                    v.seconds,
                    v.log.display()
                )),
                Ok(StartEnd::Hung) => {
                    s.push_str(&format!("{}: hung, see {}\n", v.name, v.log.display()))
                }
                Ok(StartEnd::Stopped(why)) => {
                    s.push_str(&format!("{}: stopped by the memory guard: {why}\n", v.name))
                }
                Err(e) => s.push_str(&format!("{}: {e}\n", v.name)),
            }
        }
        s.push_str(&format!("all variants: {:.0} s\n", self.seconds));
        let first = self.variants.first().map_or("", |v| v.name.as_str());
        for (name, c) in &self.comparisons {
            s.push_str(&format!("--- {name} against {first}\n{}\n", c.summary()));
        }
        s
    }
}

/// Stages and starts every variant and compares the pictures. A start that fails is a failing variant in the result,
/// not an `Err`; an `Err` is a request that cannot be carried out (a picture that cannot be read, say).
pub fn run(req: &RunRequest, launcher: &dyn Launcher, say: Say) -> Result<RunResult, String> {
    let mut variants = req.variants.clone();
    if variants.is_empty() {
        variants.push(Variant {
            name: "base".into(),
            overlays: Vec::new(),
            env: Vec::new(),
        });
    }
    let (resolution, settle) = if req.quick {
        ("640x360", 100)
    } else {
        (req.resolution.as_str(), req.settle_frames)
    };
    let jobs = req.jobs.max(1);
    let opts = Opts {
        kind: "run",
        concurrent: variants.len().min(jobs),
        start: &req.start,
        settle,
        resolution,
        env: Vec::new(),
        stall_s: 0,
        progress: None,
        pictures: true,
        lock: req.start.game_lock.clone().map(|path| GameLock {
            path,
            exclusive: false,
        }),
    };
    say(&launcher.describe(opts.concurrent));
    let started = std::time::Instant::now();
    let mut results: Vec<VariantResult> = Vec::new();
    for chunk in variants.chunks(jobs) {
        std::thread::scope(|scope| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|v| {
                    let opts = &opts;
                    scope.spawn(move || {
                        let t = std::time::Instant::now();
                        let end = start_one(&v.name, &v.overlays, &v.env, opts, launcher, say);
                        let dir = opts.start.out.join(&v.name);
                        VariantResult {
                            name: v.name.clone(),
                            end,
                            seconds: t.elapsed().as_secs_f64(),
                            pictures: dir.join("pictures"),
                            log: dir.join("run.log"),
                        }
                    })
                })
                .collect();
            for h in handles {
                results.push(h.join().expect("a variant's thread"));
            }
        });
    }
    let ok = results
        .iter()
        .all(|r| matches!(r.end, Ok(StartEnd::Exit(0))));
    let mut comparisons = Vec::new();
    if ok && variants.len() > 1 {
        // The comparison's own verdict is not the run's: a difference is a number to read, not a failure.
        let base = req.start.out.join(&variants[0].name).join("pictures");
        for v in &variants[1..] {
            let c = compare(&CompareRequest {
                a: base.clone(),
                b: req.start.out.join(&v.name).join("pictures"),
                ..CompareRequest::default()
            })?;
            comparisons.push((v.name.clone(), c));
        }
    }
    Ok(RunResult {
        ok,
        seconds: started.elapsed().as_secs_f64(),
        variants: results,
        comparisons,
    })
}

// ---------------------------------------------------------------------------------------------------------------------
// soak

/// `soak`: one engine start visits every place of the file and runs `frames` frames at each.
#[derive(Clone, Debug)]
pub struct SoakRequest {
    pub start: Start,
    /// Frames at each place.
    pub frames: u32,
    /// Frames left out of the numbers at each place (default 120).
    pub warmup: Option<u32>,
    /// Passes whose median is taken (default 1).
    pub passes: Option<u32>,
    /// The real clock instead of the engine's fixed step (a bench).
    pub realtime: bool,
    /// The machine is not shared: the timing rows are valid (a bench).
    pub exclusive: bool,
    /// Seconds the engine's log may stay silent before the start counts as hung and is killed (0: no watch).
    pub stall_s: u64,
    /// A sweep's report: rows are appended as the engine finishes places.
    pub progress: Option<PathBuf>,
    /// An overlay folder laid over the game's data.
    pub overlay: Option<PathBuf>,
}

impl SoakRequest {
    pub fn new(mut start: Start) -> SoakRequest {
        start.timeout_s = 300;
        SoakRequest {
            start,
            frames: 600,
            warmup: None,
            passes: None,
            realtime: false,
            exclusive: false,
            stall_s: 0,
            progress: None,
            overlay: None,
        }
    }
}

/// The soak's log, read: the frame-time rows and the counts the verdict uses.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LogScan {
    /// `<place> frames N avg_ms X p99_ms Y ...`, a line per place that finished its frames.
    pub rows: Vec<String>,
    pub errors: u32,
    pub warnings: u32,
    /// Times the engine logged that its main loop stopped.
    pub stops: u32,
    /// The place the player was last sent to.
    pub last_place: String,
}

/// Reads an engine log (`log.jsonl`) into a [`LogScan`]; a missing file is an empty scan.
pub fn scan_log(path: &Path) -> LogScan {
    let log = std::fs::read_to_string(path).unwrap_or_default();
    let mut s = LogScan::default();
    for line in log.lines() {
        let Ok(v) = crate::json_lite::fields(line, &["level", "message"]) else {
            continue;
        };
        let (level, msg) = (&v[0], &v[1]);
        match level.as_str() {
            "error" | "fatal" => s.errors += 1,
            "warn" => s.warnings += 1,
            _ => {}
        }
        if msg.contains("Game's main stopped") {
            s.stops += 1;
        }
        if let Some(r) = msg.strip_prefix("scene: sending the player to ") {
            s.last_place = r.split_whitespace().next().unwrap_or("").to_string();
        }
        if let Some(r) = msg.strip_prefix("scene: soak ") {
            s.rows.push(r.to_string());
        }
    }
    s
}

/// The result of [`soak`].
#[derive(Clone, Debug)]
pub struct SoakResult {
    /// The engine ended with status 0, no stop, no error line, and every place finished its frames.
    pub ok: bool,
    /// The engine's exit status ([`HUNG`] when the tool killed it for a silent log).
    pub exit_status: i32,
    pub hung: bool,
    /// Why the memory guard stopped the start ([`GUARD_STOPPED`] is the exit status then).
    pub stopped: Option<String>,
    pub seconds: f64,
    pub scan: LogScan,
    /// `scan.rows` parsed.
    pub timings: Vec<crate::bench::PlaceRow>,
    pub places_expected: usize,
    pub frames: u32,
    pub warmup: u32,
    pub passes: u32,
    /// `<out>/soak`: `run.log`, `log.jsonl`, `bench.json`.
    pub dir: PathBuf,
    /// The machine is not shared, so the timing rows are valid.
    pub exclusive: bool,
    pub realtime: bool,
}

impl SoakResult {
    /// What the command-line tool prints at the end.
    pub fn summary(&self) -> String {
        let mut s = String::new();
        if !self.exclusive {
            s.push_str("timing rows not valid, machine shared (a bench is run with `bench`, which takes the lock exclusively)\n");
        }
        s.push_str("place, frames, frame times (ms):\n");
        for r in &self.scan.rows {
            s.push_str(&format!("  {r}\n"));
        }
        s.push_str(&format!(
            "soak: {} of {} places soaked for {} frames, exit status {}, {} stops, {} error lines, {} warnings, {:.0} s{}\n",
            self.scan.rows.len(),
            self.places_expected,
            self.frames,
            self.exit_status,
            self.scan.stops,
            self.scan.errors,
            self.scan.warnings,
            self.seconds,
            if self.exit_status != 0 {
                format!("; ended at place {}, see {}", self.scan.last_place, self.dir.join("run.log").display())
            } else {
                String::new()
            }
        ));
        s
    }
}

/// What the machine was doing: the CPU frequency governor, the load average and the CPU model (a bench on a machine
/// that was busy, or on another governor, says so in its own file).
pub fn machine_fields() -> (String, String, String) {
    let read = |p: &str| {
        std::fs::read_to_string(p)
            .map(|t| t.trim().to_string())
            .unwrap_or_default()
    };
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
    (governor, load, model)
}

fn machine_notes() -> String {
    let (g, l, c) = machine_fields();
    format!(
        "{{\"governor\":{},\"loadavg\":{},\"cpu\":{}}}",
        crate::bench::esc(&g),
        crate::bench::esc(&l),
        crate::bench::esc(&c)
    )
}

/// Starts one engine that visits the places and soaks each; writes `<out>/soak/bench.json`.
pub fn soak(req: &SoakRequest, launcher: &dyn Launcher, say: Say) -> Result<SoakResult, String> {
    let t = std::time::Instant::now();
    let mut env = vec![("NV_SCENE_SOAK_FRAMES".to_string(), req.frames.to_string())];
    if let Some(w) = req.warmup {
        put(&mut env, "NV_SCENE_SOAK_WARMUP", &w.to_string());
    }
    if let Some(p) = req.passes {
        put(&mut env, "NV_SCENE_SOAK_PASSES", &p.to_string());
    }
    if req.realtime {
        put(&mut env, "NV_SCENE_REALTIME", "1");
    }
    let mut all_env = env.clone();
    all_env.extend(req.start.env.iter().cloned());
    let opts = Opts {
        kind: if req.exclusive { "bench" } else { "soak" },
        concurrent: 1,
        start: &req.start,
        settle: 60,
        resolution: "1280x720",
        env,
        stall_s: req.stall_s,
        progress: req.progress.as_deref(),
        pictures: false,
        lock: req.start.game_lock.clone().map(|path| GameLock {
            path,
            exclusive: req.exclusive,
        }),
    };
    say(&launcher.describe(opts.concurrent));
    let overlays: Vec<PathBuf> = req.overlay.iter().cloned().collect();
    let end = start_one("soak", &overlays, &[], &opts, launcher, say)?;
    let (code, hung, stopped) = match end {
        StartEnd::Exit(c) => (c, false, None),
        StartEnd::Hung => (HUNG, true, None),
        StartEnd::Stopped(why) => (GUARD_STOPPED, false, Some(why)),
    };
    let secs = t.elapsed().as_secs_f64();
    let dir = req.start.out.join("soak");
    let scan = scan_log(&dir.join("log.jsonl"));
    // `bench.json`: machine-readable numbers per place, for `bench_compare`.
    let mut json = String::from("{\"schema\":\"nv-rs-scene-bench/1\",\"places\":[");
    for (i, r) in scan.rows.iter().enumerate() {
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
        "],\"real_clock\":{},\"exclusive_lock\":{},\"machine\":{}}}\n",
        req.realtime,
        req.exclusive,
        machine_notes()
    ));
    let _ = std::fs::write(dir.join("bench.json"), &json);
    let places_expected = if req.start.only.is_empty() {
        std::fs::read_to_string(&req.start.places).map_or(0, |t| t.matches("[[place]]").count())
    } else {
        req.start.only.len()
    };
    let ok = code == 0 && scan.stops == 0 && scan.errors == 0 && scan.rows.len() == places_expected;
    let timings = scan
        .rows
        .iter()
        .filter_map(|l| crate::bench::PlaceRow::parse(l))
        .collect();
    Ok(SoakResult {
        ok,
        exit_status: code,
        hung,
        stopped,
        seconds: secs,
        timings,
        places_expected,
        frames: req.frames,
        warmup: env_value(&all_env, "NV_SCENE_SOAK_WARMUP", 120),
        passes: env_value(&all_env, "NV_SCENE_SOAK_PASSES", 1),
        dir,
        exclusive: req.exclusive,
        realtime: req.realtime,
        scan,
    })
}

// ---------------------------------------------------------------------------------------------------------------------
// sweep

/// `sweep`: the stability sweep. Like a soak, but a place that stops, hangs or is put back does not end it: the failure
/// is recorded with the end of the log, and the engine is started again at the places after it. The report is
/// `<out>/sweep.md`. It runs in the calling process: the command-line tool runs it in a child of its own so that a
/// killed sweep is reported (a program that needs that does the same).
#[derive(Clone, Debug)]
pub struct SweepRequest {
    /// `frames` 300, `timeout_s` 120 and `stall_s` 120 unless changed; `progress` is set by the sweep.
    pub soak: SoakRequest,
}

impl SweepRequest {
    pub fn new(start: Start) -> SweepRequest {
        let mut soak = SoakRequest::new(start);
        soak.frames = 300;
        soak.start.timeout_s = 120;
        soak.stall_s = 120;
        SweepRequest { soak }
    }
}

/// The result of [`sweep`].
#[derive(Clone, Debug)]
pub struct SweepResult {
    /// Every place loaded, held its frames, and the engines left cleanly.
    pub ok: bool,
    /// `<out>/sweep.md`.
    pub report: PathBuf,
    /// The text of the report.
    pub markdown: String,
    /// Place and its frame-time row.
    pub passed: Vec<(String, String)>,
    /// Place (or `(after the last place)`) and why.
    pub failed: Vec<(String, String)>,
    pub places: usize,
    pub warnings: u32,
    pub starts: usize,
    pub seconds: f64,
}

/// Runs the sweep and writes `<out>/sweep.md` as it goes (the report says "in progress" until the summary is written).
pub fn sweep(req: &SweepRequest, launcher: &dyn Launcher, say: Say) -> Result<SweepResult, String> {
    let start = &req.soak.start;
    let all: Vec<String> = crate::PlaceFile::parse(
        &std::fs::read_to_string(&start.places)
            .map_err(|e| format!("{}: {e}", start.places.display()))?,
    )?
    .places
    .into_iter()
    .map(|p| p.name)
    .collect();
    let mut remaining: Vec<String> = if start.only.is_empty() {
        all
    } else {
        all.iter()
            .filter(|n| start.only.contains(n))
            .cloned()
            .collect()
    };
    let out = start.out.clone();
    let total = remaining.len();
    let report = out.join("sweep.md");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    std::fs::write(
        &report,
        format!(
            "# Sweep of {} (in progress: a report without a summary line below the title is one that did not finish)\n\n| place | result |\n| --- | --- |\n",
            start.places.display()
        ),
    )
    .map_err(|e| e.to_string())?;
    let mut passed: Vec<(String, String)> = Vec::new();
    let mut failed: Vec<(String, String)> = Vec::new();
    let mut warnings = 0;
    let mut starts = 0;
    let t = std::time::Instant::now();
    while !remaining.is_empty() {
        starts += 1;
        if starts > total + 1 {
            return Err("the sweep started more engines than it has places: stopped".into());
        }
        let mut one = req.soak.clone();
        one.progress = Some(report.clone());
        one.start.out = out.join(format!("run{starts}"));
        one.start.only = remaining.clone();
        let r = soak(&one, launcher, say)?;
        warnings += r.scan.warnings;
        let done = r.scan.rows.len().min(remaining.len());
        for (name, row) in remaining.iter().zip(&r.scan.rows) {
            passed.push((name.clone(), row.clone()));
        }
        remaining.drain(..done);
        if r.ok || remaining.is_empty() {
            if !r.ok {
                // Every place held its frames, but the engine did not leave cleanly or logged errors.
                failed.push((
                    "(after the last place)".into(),
                    format!(
                        "exit status {}, {} stops, {} error lines",
                        r.exit_status, r.scan.stops, r.scan.errors
                    ),
                ));
            }
            break;
        }
        let bad = remaining.remove(0);
        let log = r.dir.join("run.log");
        // What the engine said: its error lines (the log's), else the last lines of its output.
        let said = |path: &Path, keep: &dyn Fn(&str) -> bool, n: usize| -> Vec<String> {
            let mut v: Vec<String> = std::fs::read_to_string(path)
                .unwrap_or_default()
                .lines()
                .filter(|l| keep(l))
                .map(|l| l.chars().take(300).collect())
                .collect();
            let skip = v.len().saturating_sub(n);
            v.drain(..skip);
            v
        };
        let jsonl = std::fs::read_to_string(r.dir.join("log.jsonl")).unwrap_or_default();
        let mut tail: Vec<String> = jsonl
            .lines()
            .filter_map(|l| crate::json_lite::fields(l, &["level", "message"]).ok())
            .filter(|v| v[0] == "error" || v[0] == "fatal")
            .map(|v| v[1].chars().take(300).collect())
            .collect();
        let skip = tail.len().saturating_sub(3);
        tail.drain(..skip);
        if tail.is_empty() {
            tail = said(&log, &|l| !l.starts_with("hitch:"), 4);
        }
        let why = if r.hung {
            "hung (the engine's log went silent past the stall limit)".to_string()
        } else if let Some(g) = &r.stopped {
            format!("stopped by the memory guard: {g}")
        } else {
            format!(
                "exit status {}, {} stops, {} error lines",
                r.exit_status, r.scan.stops, r.scan.errors
            )
        };
        failed.push((
            bad,
            format!(
                "{why}; log {}; last lines: {}",
                log.display(),
                tail.join(" | ")
            ),
        ));
    }
    let seconds = t.elapsed().as_secs_f64();
    let mut md = format!(
        "# Sweep of {}\n\n{} of {total} places loaded and held {} frames; {} failed; {warnings} warnings; {starts} engine starts, {seconds:.0} s.\n\n",
        start.places.display(),
        passed.len(),
        req.soak.frames,
        failed.len(),
    );
    md.push_str("| place | result |\n| --- | --- |\n");
    for (n, row) in &passed {
        md.push_str(&format!("| {n} | ok: {row} |\n"));
    }
    for (n, why) in &failed {
        md.push_str(&format!("| {n} | FAILED: {why} |\n"));
    }
    std::fs::write(&report, &md).map_err(|e| e.to_string())?;
    Ok(SweepResult {
        ok: failed.is_empty(),
        report,
        markdown: md,
        passed,
        failed,
        places: total,
        warnings,
        starts,
        seconds,
    })
}

// ---------------------------------------------------------------------------------------------------------------------
// bench

/// The built-in suites: a name and the place file it stands for (the files are in `docs/fork/scenes`).
pub const SUITES: [(&str, &str); 4] = [
    (
        "quick",
        include_str!("../../../docs/fork/scenes/bench-quick.toml"),
    ),
    (
        "mojave",
        include_str!("../../../docs/fork/scenes/mojave-far.toml"),
    ),
    (
        "soak",
        include_str!("../../../docs/fork/scenes/soak-far-moves.toml"),
    ),
    (
        "capital",
        include_str!("../../../docs/fork/scenes/ttw-capital.toml"),
    ),
];

/// Other `nv-viewer` processes running now (a bench that shared the machine is marked noisy).
pub fn other_viewers() -> u32 {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return 0;
    };
    dir.flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .chars()
                .all(|c| c.is_ascii_digit())
        })
        .filter(|e| {
            std::fs::read_to_string(e.path().join("comm"))
                .is_ok_and(|c| c.trim().starts_with("nv-viewer"))
        })
        .count() as u32
}

/// `bench`: a soak on the real clock under the exclusive game lock, with the numbers written to `<out>/soak/bench.json`
/// and, in the old harness's shape, `<out>/soak/summary.json`, `suite.md` and the suite files in the state folder.
#[derive(Clone, Debug)]
pub struct BenchRequest {
    /// Real clock, exclusive, 5 passes: set by [`BenchRequest::new`]. `start.game_lock` is the lock file (`lock`).
    pub soak: SoakRequest,
    /// The shared game lock file; the bench takes it exclusively.
    pub lock: PathBuf,
    /// A built-in suite (`quick`, `mojave`, `soak`, `capital`) instead of `soak.start.places`: its place file is written
    /// to `<out>/suite-<name>.toml`.
    pub suite: Option<String>,
    /// Where the suites and baselines are kept (default `~/.cache/nv-rs/scene-bench`).
    pub state: Option<PathBuf>,
    /// Store this run's rows as a baseline of this name.
    pub save_baseline: Option<String>,
    /// Show the change against the stored baseline of this name.
    pub baseline: Option<String>,
}

impl BenchRequest {
    pub fn new(start: Start, lock: PathBuf) -> BenchRequest {
        let mut soak = SoakRequest::new(start);
        soak.realtime = true;
        soak.exclusive = true;
        soak.passes = Some(5);
        BenchRequest {
            soak,
            lock,
            suite: None,
            state: None,
            save_baseline: None,
            baseline: None,
        }
    }
}

/// The result of [`bench`].
#[derive(Clone, Debug)]
pub struct BenchResult {
    /// The soak was ok and every headline's checks hold.
    pub ok: bool,
    pub soak: SoakResult,
    pub name: String,
    pub headlines: Vec<crate::bench::Headline>,
    /// The suite table (markdown), also in `<out>/soak/suite.md`.
    pub markdown: String,
    /// `<out>/soak/summary.json`.
    pub summary: PathBuf,
}

/// `YYYYMMDDTHHMMSS` (UTC) for a run's name.
pub fn stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (days, rem) = (secs / 86400, secs % 86400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// The rows of the newest suite in `state` older than `name`, or `None`.
fn previous_suite(state: &Path, name: &str) -> Option<crate::bench::Known> {
    let prefix = name.split('-').next()?.to_string();
    let mut dirs: Vec<String> = std::fs::read_dir(state)
        .ok()?
        .flatten()
        .filter(|e| e.path().join("suite.json").exists())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(&format!("{prefix}-")) && n.as_str() < name)
        .collect();
    dirs.sort();
    let last = dirs.pop()?;
    let text = std::fs::read_to_string(state.join(last).join("suite.json")).ok()?;
    Some(crate::bench::known_from(&text))
}

/// Runs the bench and writes its files.
pub fn bench(req: &BenchRequest, launcher: &dyn Launcher, say: Say) -> Result<BenchResult, String> {
    let mut soak_req = req.soak.clone();
    soak_req.start.game_lock = Some(req.lock.clone());
    soak_req.exclusive = true;
    soak_req.realtime = true;
    let out = soak_req.start.out.clone();
    let suite_name = req.suite.clone().unwrap_or_else(|| "places".into());
    if let Some(name) = &req.suite {
        let (_, text) = SUITES.iter().find(|(n, _)| n == name).ok_or_else(|| {
            let known: Vec<&str> = SUITES.iter().map(|(n, _)| *n).collect();
            format!(
                "no suite `{name}` (the built-in ones: {})",
                known.join(", ")
            )
        })?;
        std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
        let file = out.join(format!("suite-{name}.toml"));
        std::fs::write(&file, text).map_err(|e| format!("{}: {e}", file.display()))?;
        soak_req.start.places = file;
    }
    let noisy_before = other_viewers() > 0;
    let r = soak(&soak_req, launcher, say)?;
    let noisy = noisy_before || other_viewers() > 0;
    let name = format!("{suite_name}-{}", stamp());
    let rows = r.timings.clone();
    let notes = machine_fields();
    let facts = crate::bench::RunFacts {
        name: name.clone(),
        scenario: suite_name.clone(),
        resolution: "1280x720".into(),
        limiters: crate::bench::limiters_in(|k| std::env::var(k).ok()),
        wall_s: r.seconds,
        noisy,
        exit_status: r.exit_status,
        stops: r.scan.stops,
        errors: r.scan.errors,
        warnings: r.scan.warnings,
        governor: notes.0,
        loadavg: notes.1,
        cpu: notes.2,
        warmup: r.warmup,
        passes: r.passes,
    };
    let summary = crate::bench::summary_json(&suite_name, &rows, &facts);
    let write = |p: PathBuf, text: &str| {
        std::fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))
    };
    let summary_path = out.join("soak").join("summary.json");
    write(summary_path.clone(), &summary)?;
    let heads: Vec<_> = rows
        .iter()
        .map(|row| crate::bench::headline(&suite_name, row, &facts))
        .collect();
    let state = req.state.clone().unwrap_or_else(|| {
        PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache/nv-rs/scene-bench")
    });
    let previous = previous_suite(&state, &name);
    let base = req.baseline.as_ref().and_then(|b| {
        std::fs::read_to_string(state.join("baselines").join(format!("{b}.json")))
            .ok()
            .map(|t| crate::bench::known_from(&t))
    });
    let md = crate::bench::suite_markdown(
        &name,
        &heads,
        previous.as_ref(),
        base.as_ref(),
        req.baseline.as_deref(),
        "1280x720",
    );
    write(out.join("soak").join("suite.md"), &md)?;
    let suite_dir = state.join(&name);
    std::fs::create_dir_all(&suite_dir).map_err(|e| format!("{}: {e}", suite_dir.display()))?;
    write(
        suite_dir.join("suite.json"),
        &crate::bench::suite_json(&name, &heads),
    )?;
    write(suite_dir.join("suite.md"), &md)?;
    if let Some(b) = &req.save_baseline {
        let dir = state.join("baselines");
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        write(
            dir.join(format!("{b}.json")),
            &crate::bench::baseline_json(&heads),
        )?;
    }
    Ok(BenchResult {
        ok: r.ok && !heads.iter().any(|h| !h.checks_ok),
        soak: r,
        name,
        headlines: heads,
        markdown: md,
        summary: summary_path,
    })
}

// ---------------------------------------------------------------------------------------------------------------------
// bench-compare

/// `bench-compare`: two `bench.json` files.
#[derive(Clone, Debug, Default)]
pub struct BenchCompareRequest {
    pub a: PathBuf,
    pub b: PathBuf,
}

/// One place's numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BenchNumbers {
    pub avg_ms: f64,
    pub low1_fps: f64,
    pub max_ms: f64,
}

/// One place of A, and B's numbers for it (`None`: not in B).
#[derive(Clone, Debug, PartialEq)]
pub struct BenchChange {
    pub place: String,
    pub a: BenchNumbers,
    pub b: Option<BenchNumbers>,
}

impl BenchChange {
    /// The change of the average frame time from A to B in percent (positive: B is slower).
    pub fn change_pct(&self) -> Option<f64> {
        self.b
            .map(|b| 100.0 * (b.avg_ms - self.a.avg_ms) / self.a.avg_ms.max(1e-9))
    }

    pub fn line(&self) -> String {
        match (&self.b, self.change_pct()) {
            (Some(b), Some(pct)) => format!(
                "{}: {:.2} -> {:.2} ({pct:+.1}%), {:.1} -> {:.1}, {:.1} -> {:.1}",
                self.place,
                self.a.avg_ms,
                b.avg_ms,
                self.a.low1_fps,
                b.low1_fps,
                self.a.max_ms,
                b.max_ms
            ),
            _ => format!("{}: not in B", self.place),
        }
    }
}

/// The result of [`bench_compare`].
#[derive(Clone, Debug, PartialEq)]
pub struct BenchCompareResult {
    pub rows: Vec<BenchChange>,
}

impl BenchCompareResult {
    pub fn summary(&self) -> String {
        let mut s = String::from(
            "place: avg ms A -> B (change), 1% low fps A -> B, worst frame ms A -> B\n",
        );
        for r in &self.rows {
            s.push_str(&r.line());
            s.push('\n');
        }
        s
    }
}

/// Per place, the average frame time, the 1% low and the largest frame of both and the change from A to B.
pub fn bench_compare(req: &BenchCompareRequest) -> Result<BenchCompareResult, String> {
    let rows = |p: &Path| -> Result<Vec<(String, BenchNumbers)>, String> {
        let text = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
        let mut out = Vec::new();
        for chunk in text.split("{\"place\":\"").skip(1) {
            let name = chunk.split('"').next().unwrap_or("").to_string();
            let num = |key: &str| -> Result<f64, String> {
                let at = chunk
                    .find(&format!("\"{key}\":"))
                    .ok_or(format!("{}: no {key} for {name}", p.display()))?;
                let rest = &chunk[at + key.len() + 3..];
                let end = rest.find([',', '}']).unwrap_or(rest.len());
                rest[..end]
                    .parse::<f64>()
                    .map_err(|e| format!("{}: {key}: {e}", p.display()))
            };
            out.push((
                name.clone(),
                BenchNumbers {
                    avg_ms: num("avg_ms")?,
                    low1_fps: num("low1_fps")?,
                    max_ms: num("max_ms")?,
                },
            ));
        }
        Ok(out)
    };
    let (ra, rb) = (rows(&req.a)?, rows(&req.b)?);
    Ok(BenchCompareResult {
        rows: ra
            .into_iter()
            .map(|(place, a)| BenchChange {
                b: rb.iter().find(|r| r.0 == place).map(|r| r.1),
                place,
                a,
            })
            .collect(),
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// A launcher that starts a shell script in place of the engine: what a caller's own start looks like.
    struct Script(&'static str);

    impl Launcher for Script {
        fn describe(&self, _: usize) -> String {
            "no cap (a test)".into()
        }

        fn launch(&self, spec: &StartSpec, log: std::fs::File) -> Result<Launched, String> {
            let mut cmd = std::process::Command::new("sh");
            cmd.arg("-c")
                .arg(self.0)
                .current_dir(&spec.game_dir)
                .stdout(log.try_clone().unwrap())
                .stderr(log);
            for (k, v) in &spec.env {
                cmd.env(k, v);
            }
            Ok(Launched {
                child: cmd.spawn().map_err(|e| e.to_string())?,
                scope_prefix: None,
            })
        }
    }

    fn setup(name: &str, place_names: &[&str]) -> (PathBuf, Start) {
        let t = std::env::temp_dir().join(format!("fork-scene-tool-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let game = t.join("install");
        std::fs::create_dir_all(game.join("Data")).unwrap();
        std::fs::write(game.join("FalloutNV.exe"), "x").unwrap();
        std::fs::write(game.join("Data/FalloutNV.esm"), "x").unwrap();
        let mut text = String::from("version = 1\n");
        for n in place_names {
            text.push_str(&format!(
                "[[place]]\nname = \"{n}\"\nworld = \"W\"\nx = 1\ny = 2\nz = 3\n"
            ));
        }
        std::fs::write(t.join("places.toml"), text).unwrap();
        let mut start = Start::new(
            PathBuf::from("unused"),
            game,
            t.join("places.toml"),
            t.join("out"),
        );
        start.gpu_sysfs = PathBuf::from("/nonexistent");
        (t, start)
    }

    const GOOD: &str = r#"for p in $(echo "$NV_SCENE_PLACES" | tr ',' ' '); do
  echo '{"level":"info","message":"scene: sending the player to '$p' from the Menu screen"}' >> "$NV_LOG_JSON"
  echo '{"level":"info","message":"scene: soak '$p' frames 10 avg_ms 5.00 p99_ms 6.00 max_ms 7.00 low1_fps 150.0"}' >> "$NV_LOG_JSON"
done"#;

    #[test]
    fn a_soak_through_the_library_returns_typed_rows_and_prints_nothing_itself() {
        let (t, mut start) = setup("soak", &["a", "b"]);
        start.only = vec!["a".into(), "b".into()];
        let mut req = SoakRequest::new(start);
        req.frames = 10;
        let lines = std::sync::Mutex::new(Vec::<String>::new());
        let say = |l: &str| lines.lock().unwrap().push(l.to_string());
        let r = soak(&req, &Script(GOOD), &say).unwrap();
        assert!(r.ok && !r.hung, "{}", r.summary());
        assert_eq!(r.timings.len(), 2);
        assert_eq!(r.timings[0].place, "a");
        assert!((r.timings[0].avg_ms - 5.0).abs() < 1e-9);
        assert_eq!(lines.lock().unwrap()[0], "no cap (a test)");
        assert!(r.dir.join("bench.json").exists());
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn a_sweep_through_the_library_goes_on_after_a_failing_place() {
        let (t, mut start) = setup("sweep", &["a", "bad", "c"]);
        start.only = Vec::new();
        let mut req = SweepRequest::new(start);
        req.soak.frames = 10;
        let engine = r#"for p in $(echo "$NV_SCENE_PLACES" | tr ',' ' '); do
  echo '{"level":"info","message":"scene: sending the player to '$p' from the Menu screen"}' >> "$NV_LOG_JSON"
  if [ "$p" = bad ]; then echo '{"level":"error","message":"scene: place bad: no cell"}' >> "$NV_LOG_JSON"; exit 70; fi
  echo '{"level":"info","message":"scene: soak '$p' frames 10 avg_ms 5.00 p99_ms 6.00 max_ms 7.00 low1_fps 150.0"}' >> "$NV_LOG_JSON"
done"#;
        let r = sweep(&req, &Script(engine), &|_| {}).unwrap();
        assert!(!r.ok);
        assert_eq!((r.places, r.starts), (3, 2));
        assert_eq!(
            r.passed.iter().map(|p| p.0.as_str()).collect::<Vec<_>>(),
            ["a", "c"]
        );
        assert_eq!(r.failed.len(), 1);
        assert!(r.failed[0].1.contains("exit status 70") && r.failed[0].1.contains("no cell"));
        assert!(std::fs::read_to_string(&r.report)
            .unwrap()
            .contains("| bad | FAILED"));
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn a_run_through_the_library_reports_each_variant_and_a_start_that_is_refused() {
        let (t, start) = setup("run", &["a"]);
        let mut req = RunRequest::new(start);
        req.variants = vec![Variant {
            name: "v".into(),
            overlays: Vec::new(),
            env: Vec::new(),
        }];
        let engine = r#"mkdir -p "$NV_SCENE_OUT"; exit 3"#;
        let r = run(&req, &Script(engine), &|_| {}).unwrap();
        assert!(!r.ok);
        assert_eq!(r.variants[0].end, Ok(StartEnd::Exit(3)));
        assert!(r.summary().contains("the engine ended with status 3"));
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn bench_without_a_known_suite_is_an_error_and_a_known_one_is_listed() {
        let (t, start) = setup("bench", &["a"]);
        let mut req = BenchRequest::new(start, t.join("game.lock"));
        req.suite = Some("nope".into());
        let e = bench(&req, &Script(GOOD), &|_| {}).unwrap_err();
        assert!(e.contains("no suite `nope`") && e.contains("quick"), "{e}");
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn bench_compare_gives_the_change_per_place() {
        let t = std::env::temp_dir().join(format!("fork-scene-tool-bc-{}", std::process::id()));
        std::fs::create_dir_all(&t).unwrap();
        let row = |p: &str, avg: f64| {
            format!("{{\"place\":\"{p}\",\"frames\":10,\"avg_ms\":{avg},\"p99_ms\":9,\"max_ms\":10,\"low1_fps\":100}}")
        };
        std::fs::write(
            t.join("a.json"),
            format!("{{\"places\":[{},{}]}}", row("x", 5.0), row("y", 6.0)),
        )
        .unwrap();
        std::fs::write(
            t.join("b.json"),
            format!("{{\"places\":[{}]}}", row("x", 5.5)),
        )
        .unwrap();
        let r = bench_compare(&BenchCompareRequest {
            a: t.join("a.json"),
            b: t.join("b.json"),
        })
        .unwrap();
        assert!((r.rows[0].change_pct().unwrap() - 10.0).abs() < 1e-9);
        assert_eq!(r.rows[1].b, None);
        assert!(r.summary().contains("y: not in B"));
        std::fs::remove_dir_all(&t).unwrap();
    }
}
