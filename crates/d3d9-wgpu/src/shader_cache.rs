//! The converted-shader cache: Direct3D 9 bytecode, linked as the device links it, converted to
//! WGSL once and kept on disk between runs (layer (a) of docs/fork/SHADER-CACHE.md; the driver's
//! own pipeline cache, `cache.rs`, is layer (b), the pipeline list, `prepare.rs`, layer (c)).
//!
//! **What an entry is keyed by**: a 128-bit hash of the converter's stamp, the bytecode's bytes
//! and the link (varyings, inputs, clip, flags). Never a file name, a mod, an object, a texture,
//! a path or the load order: a shader a mod ships hits the cache from the second run on whatever
//! file it came from, two mods that ship the same shader share one entry, and adding, removing or
//! reordering mods invalidates nothing but the entries of bytecode that is no longer used (those
//! age out by the size cap, least recently used first).
//!
//! **The stamp** is the source fingerprint of the crate (`build.rs`: every converter and device
//! source), so a build whose converter changed has its own folder, `modules/<stamp>/`; the
//! folders of other stamps are removed when the cache is opened.
//!
//! **Safety**: an entry is a small envelope (magic, version, the key again, the length and a
//! hash of the text). A missing, short, altered or foreign file is a miss (a damaged one is
//! removed and counted); nothing in the cache can fail a start. Writes go to a temporary of the
//! writer's own and are renamed ([`crate::cache::write_replacing`]), so another process reads
//! the old entry or the new one, never half of one, and two processes writing the same key write
//! the same bytes. A hit refreshes the file's modified time, which is the order the size cap
//! evicts in.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

const MAGIC: &[u8; 8] = b"NVSHMOD1";
/// Bump when the envelope changes.
const VERSION: u32 = 1;
/// magic, version, key (16), length, text hash.
const HEADER: usize = 8 + 4 + 16 + 8 + 8;
/// An entry larger than this is never written or read (the largest converted shader is a few
/// hundred KB).
/// A hit refreshes the entry's modified time when it is older than this.
const TOUCH_AFTER_SECS: u64 = 3600;
const MAX_ENTRY: u64 = 16 << 20;
/// The default size cap of the whole store.
pub const DEFAULT_MAX_BYTES: u64 = 1 << 30;

/// Where the store lives and how large it may grow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub root: PathBuf,
    pub max_bytes: u64,
}

impl Settings {
    /// From `NV_SHADER_CACHE` (`off`, `0`, `none` or `false` turns the store off, any other
    /// value is the folder) and `NV_SHADER_CACHE_MAX_MB`; unset, `default_root`. `None` when off
    /// or without a folder.
    pub fn from_env(default_root: Option<&Path>) -> Option<Settings> {
        let var = std::env::var("NV_SHADER_CACHE").ok();
        let mb = std::env::var("NV_SHADER_CACHE_MAX_MB")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok());
        Self::resolve(var.as_deref(), mb, default_root)
    }

    pub(crate) fn resolve(
        var: Option<&str>,
        max_mb: Option<u64>,
        default_root: Option<&Path>,
    ) -> Option<Settings> {
        let root = match var.map(str::trim) {
            Some(v)
                if matches!(
                    v.to_ascii_lowercase().as_str(),
                    "off" | "0" | "none" | "false"
                ) =>
            {
                return None
            }
            Some(v) if !v.is_empty() => PathBuf::from(v),
            _ => default_root?.to_path_buf(),
        };
        Some(Settings {
            root,
            max_bytes: max_mb.map_or(DEFAULT_MAX_BYTES, |m| m << 20),
        })
    }
}

/// The default folder: `$XDG_CACHE_HOME/nv-rs/shaders`, else `~/.cache/nv-rs/shaders`; on
/// Windows `%LOCALAPPDATA%\nv-rs\shaders`.
pub fn default_root() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        match std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from) {
            Some(p) if p.is_absolute() => Some(p),
            _ => std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")),
        }
    }?;
    Some(base.join("nv-rs").join("shaders"))
}

/// What a host gives the backend: the converted-shader store's settings (`NV_SHADER_CACHE`,
/// default [`default_root`]) and the folder of the driver's pipeline cache and pipeline list: the
/// host's own (`NV_WGPU_CACHE`) when it has one, else `<root>/pipelines`. `NV_SHADER_CACHE=off`
/// turns the default off (an explicit `NV_WGPU_CACHE` stays). `default_on` false: only an explicit
/// folder or `NV_SHADER_CACHE=PATH` turns anything on (tests).
pub fn plan(
    explicit_pipelines: Option<PathBuf>,
    default_on: bool,
) -> (Option<Settings>, Option<PathBuf>) {
    let root = if default_on { default_root() } else { None };
    let settings = Settings::from_env(root.as_deref());
    let pipelines =
        explicit_pipelines.or_else(|| settings.as_ref().map(|s| s.root.join("pipelines")));
    (settings, pipelines)
}

/// 128-bit FNV-1a (the published offset basis and prime), fixed by its definition so the keys
/// never move with a standard-library change. Each part is length-framed.
pub fn hash128(parts: &[&[u8]]) -> u128 {
    const BASIS: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;
    let mut h = BASIS;
    for p in parts {
        for b in (p.len() as u64).to_le_bytes().iter().chain(p.iter()) {
            h ^= u128::from(*b);
            h = h.wrapping_mul(PRIME);
        }
    }
    h
}

/// The key of one conversion: stamp, bytecode, link description.
pub fn key(stamp: &str, tokens: &[u32], link: &str) -> u128 {
    let bytes: Vec<u8> = tokens.iter().flat_map(|t| t.to_le_bytes()).collect();
    hash128(&[stamp.as_bytes(), &bytes, link.as_bytes()])
}

#[derive(Debug, Default)]
struct Counters {
    hits: AtomicU64,
    misses: AtomicU64,
    stored: AtomicU64,
    corrupt: AtomicU64,
    evicted: AtomicU64,
    bytes_read: AtomicU64,
    bytes_written: AtomicU64,
    /// Nanoseconds spent reading hits and writing entries.
    io_ns: AtomicU64,
}

/// A snapshot of the counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub hits: u64,
    pub misses: u64,
    pub stored: u64,
    /// Entries found damaged (removed, counted as misses too).
    pub corrupt: u64,
    pub evicted: u64,
    pub bytes_read: u64,
    pub bytes_written: u64,
}

/// The on-disk store of converted modules.
pub struct ShaderCache {
    dir: PathBuf,
    root: PathBuf,
    stamp: String,
    max_bytes: u64,
    counters: Counters,
    /// A write failed (read-only folder): no more this run, said once.
    failed: std::sync::atomic::AtomicBool,
}

/// Every open store, for the exit summary.
static STORES: Mutex<Vec<Weak<ShaderCache>>> = Mutex::new(Vec::new());

impl ShaderCache {
    /// Opens the store under `settings.root` for converter `stamp` (sixteen hex digits): makes
    /// the folder, removes the folders of other stamps and stale temporaries, and evicts down to
    /// the cap when over it. `None` (one warn line) when the folder cannot be made: never a
    /// failed start.
    pub fn open(settings: &Settings, stamp: &str) -> Option<Arc<ShaderCache>> {
        let modules = settings.root.join("modules");
        let dir = modules.join(stamp);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            fork_log::warn!(target: "d3d9",
                "NV_D3D9 wgpu: shader cache {}: {e}: no shader cache this run",
                settings.root.display());
            return None;
        }
        remove_other_stamps(&modules, stamp);
        remove_stale_temporaries(&dir);
        let c = Arc::new(ShaderCache {
            dir,
            root: settings.root.clone(),
            stamp: stamp.to_string(),
            max_bytes: settings.max_bytes,
            counters: Counters::default(),
            failed: std::sync::atomic::AtomicBool::new(false),
        });
        c.evict_to_cap();
        STORES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Arc::downgrade(&c));
        Some(c)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn stamp(&self) -> &str {
        &self.stamp
    }

    fn path(&self, key: u128) -> PathBuf {
        let hex = format!("{key:032x}");
        self.dir.join(&hex[..2]).join(format!("{}.wgsl", &hex[2..]))
    }

    /// The converted text for `key`, or none. A damaged entry is removed.
    pub fn get(&self, key: u128) -> Option<String> {
        let t = std::time::Instant::now();
        let path = self.path(key);
        let got = match read_entry(&path, key) {
            Ok(text) => Some(text),
            Err(Miss::Absent) => None,
            Err(Miss::Damaged(why)) => {
                self.counters.corrupt.fetch_add(1, Ordering::Relaxed);
                fork_log::warn!(target: "d3d9",
                    "NV_D3D9 wgpu: shader cache entry {}: {why}: discarded", path.display());
                let _ = std::fs::remove_file(&path);
                None
            }
        };
        match &got {
            Some(text) => {
                self.counters.hits.fetch_add(1, Ordering::Relaxed);
                self.counters
                    .bytes_read
                    .fetch_add(text.len() as u64, Ordering::Relaxed);
                // The least-recently-used order the cap evicts in: refreshed when the entry's
                // time is over an hour old (a touch per hit costs more than the hit).
                let stale = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .map_or(true, |age| age.as_secs() > TOUCH_AFTER_SECS);
                if stale {
                    if let Ok(f) = std::fs::File::options().write(true).open(&path) {
                        let _ = f.set_modified(std::time::SystemTime::now());
                    }
                }
            }
            None => {
                self.counters.misses.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.counters
            .io_ns
            .fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
        got
    }

    /// Keeps `text` under `key` (atomically); a failure is said once and stops writes.
    pub fn put(&self, key: u128, text: &str) {
        if self.failed.load(Ordering::Relaxed) || text.len() as u64 > MAX_ENTRY {
            return;
        }
        let t = std::time::Instant::now();
        let bytes = wrap(key, text);
        match crate::cache::write_replacing(&self.path(key), &bytes) {
            Ok(()) => {
                self.counters.stored.fetch_add(1, Ordering::Relaxed);
                self.counters
                    .bytes_written
                    .fetch_add(bytes.len() as u64, Ordering::Relaxed);
            }
            Err(e) => {
                if !self.failed.swap(true, Ordering::Relaxed) {
                    fork_log::warn!(target: "d3d9",
                        "NV_D3D9 wgpu: shader cache {}: not written: {e}: none written this run",
                        self.dir.display());
                }
            }
        }
        self.counters
            .io_ns
            .fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
    }

    /// Whether an entry for `key` exists and is whole (no counters, no touch): the precompile
    /// pass skips these.
    pub fn contains(&self, key: u128) -> bool {
        read_entry(&self.path(key), key).is_ok()
    }

    pub fn stats(&self) -> Stats {
        let c = &self.counters;
        Stats {
            hits: c.hits.load(Ordering::Relaxed),
            misses: c.misses.load(Ordering::Relaxed),
            stored: c.stored.load(Ordering::Relaxed),
            corrupt: c.corrupt.load(Ordering::Relaxed),
            evicted: c.evicted.load(Ordering::Relaxed),
            bytes_read: c.bytes_read.load(Ordering::Relaxed),
            bytes_written: c.bytes_written.load(Ordering::Relaxed),
        }
    }

    /// Removes the least recently used entries until the store is at most nine tenths of its
    /// cap, when it is over the cap. Returns how many went.
    pub fn evict_to_cap(&self) -> usize {
        let mut files = Vec::new();
        collect_entries(&self.dir, &mut files);
        let total: u64 = files.iter().map(|f| f.1).sum();
        let n = evict(&mut files, total, self.max_bytes);
        self.counters.evicted.fetch_add(n as u64, Ordering::Relaxed);
        n
    }

    /// The exit summary's line.
    pub fn summary(&self) -> String {
        let s = self.stats();
        let total = s.hits + s.misses;
        format!(
            "shader cache {}: {} hits, {} misses ({:.0}% hit), {} stored ({} KB), {} damaged, {} evicted, {:.0} ms of I/O",
            self.root.display(),
            s.hits,
            s.misses,
            if total == 0 {
                0.0
            } else {
                s.hits as f64 * 100.0 / total as f64
            },
            s.stored,
            s.bytes_written / 1024,
            s.corrupt,
            s.evicted,
            self.counters.io_ns.load(Ordering::Relaxed) as f64 / 1e6
        )
    }
}

/// Logs every open store's summary (the exit hooks call this through
/// [`crate::save_pipeline_caches`]), once per store.
pub fn log_summaries() {
    let all: Vec<Arc<ShaderCache>> = STORES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter_map(Weak::upgrade)
        .collect();
    for c in all {
        eprintln!("NV_D3D9 wgpu: {}", c.summary());
    }
}

/// The stamp of this build's converter (`build.rs`: the sources of `src/sm`).
pub const CONVERTER_STAMP: &str = env!("D3D9_WGPU_CONVERTER_SOURCES");

/// `wgsl::convert`, through `cache` when there is one: the text of a hit is returned as it was
/// stored, a miss converts and stores. An error is never stored.
pub(crate) fn convert(
    cache: Option<&ShaderCache>,
    tokens: &[u32],
    program: &crate::sm::Program,
    link: &crate::sm::wgsl::Link,
) -> Result<String, crate::sm::Error> {
    let Some(cache) = cache else {
        return crate::sm::wgsl::convert(program, link);
    };
    let k = key(cache.stamp(), tokens, &format!("{link:?}"));
    if let Some(text) = cache.get(k) {
        return Ok(text);
    }
    let text = crate::sm::wgsl::convert(program, link)?;
    cache.put(k, &text);
    Ok(text)
}

enum Miss {
    Absent,
    Damaged(&'static str),
}

fn wrap(key: u128, text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER + text.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&key.to_le_bytes());
    out.extend_from_slice(&(text.len() as u64).to_le_bytes());
    out.extend_from_slice(&crate::cache::hash(text.as_bytes()).to_le_bytes());
    out.extend_from_slice(text.as_bytes());
    out
}

fn unwrap(file: &[u8], key: u128) -> Result<String, &'static str> {
    if file.len() < HEADER || &file[..8] != MAGIC {
        return Err("not a shader cache entry");
    }
    if u32::from_le_bytes(file[8..12].try_into().expect("4")) != VERSION {
        return Err("another version of the entry");
    }
    if u128::from_le_bytes(file[12..28].try_into().expect("16")) != key {
        return Err("another key inside");
    }
    let len = u64::from_le_bytes(file[28..36].try_into().expect("8"));
    let sum = u64::from_le_bytes(file[36..44].try_into().expect("8"));
    let data = &file[HEADER..];
    if len != data.len() as u64 {
        return Err("truncated or too long");
    }
    if sum != crate::cache::hash(data) {
        return Err("its text does not match its hash");
    }
    String::from_utf8(data.to_vec()).map_err(|_| "not text")
}

fn read_entry(path: &Path, key: u128) -> Result<String, Miss> {
    match std::fs::metadata(path) {
        Ok(m) if m.len() > MAX_ENTRY + HEADER as u64 => return Err(Miss::Damaged("too large")),
        Ok(_) => {}
        Err(_) => return Err(Miss::Absent),
    }
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Miss::Absent),
        Err(_) => return Err(Miss::Damaged("unreadable")),
    };
    unwrap(&bytes, key).map_err(Miss::Damaged)
}

/// Removes `modules/<other>` folders, the stamps of other builds.
fn remove_other_stamps(modules: &Path, current: &str) {
    let Ok(entries) = std::fs::read_dir(modules) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name();
        let Some(name) = name.to_str() else { continue };
        // Only folders named like a stamp (hex): nothing else in a shared folder is ours.
        if name != current && name.len() == 16 && name.bytes().all(|b| b.is_ascii_hexdigit()) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// Removes the temporaries of writes that never finished (`*.PID-N.tmp`) older than ten minutes
/// (another process's write in progress is younger) under `dir`.
fn remove_stale_temporaries(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        let Ok(m) = e.metadata() else { continue };
        if m.is_dir() {
            remove_stale_temporaries(&path);
        } else if path.extension().is_some_and(|x| x == "tmp")
            && m.modified()
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age.as_secs() > 600)
        {
            let _ = std::fs::remove_file(&path);
        }
    }
}

type Found = (PathBuf, u64, std::time::SystemTime);

/// `(path, size, modified)` of every entry file under `dir`.
fn collect_entries(dir: &Path, out: &mut Vec<Found>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        let Ok(m) = e.metadata() else { continue };
        if m.is_dir() {
            collect_entries(&path, out);
        } else if path.extension().is_some_and(|x| x == "wgsl") {
            out.push((
                path,
                m.len(),
                m.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH),
            ));
        }
    }
}

/// Removes the oldest of `files` (total size `total`) while `total` is over `cap`, down to nine
/// tenths of it. Returns how many were removed.
fn evict(files: &mut [Found], mut total: u64, cap: u64) -> usize {
    if total <= cap {
        return 0;
    }
    files.sort_by_key(|f| f.2);
    let goal = cap / 10 * 9;
    let mut n = 0;
    for (path, size, _) in files.iter() {
        if total <= goal {
            break;
        }
        if std::fs::remove_file(path).is_ok() {
            total -= size;
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nv-shcache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn settings(d: &Path, max: u64) -> Settings {
        Settings {
            root: d.to_path_buf(),
            max_bytes: max,
        }
    }

    fn open(d: &Path, max: u64) -> Arc<ShaderCache> {
        ShaderCache::open(&settings(d, max), "0123456789abcdef").expect("opens")
    }

    #[test]
    fn the_hash_is_deterministic_and_length_framed() {
        assert_eq!(hash128(&[b"ab", b"c"]), hash128(&[b"ab", b"c"]));
        assert_ne!(hash128(&[b"ab", b"c"]), hash128(&[b"a", b"bc"]));
        assert_ne!(hash128(&[b""]), hash128(&[]));
    }

    #[test]
    fn a_key_follows_the_bytecode_link_and_stamp_only() {
        let a = key("s1", &[1, 2, 3], "link");
        assert_eq!(a, key("s1", &[1, 2, 3], "link"));
        assert_ne!(a, key("s2", &[1, 2, 3], "link"));
        assert_ne!(a, key("s1", &[1, 2, 4], "link"));
        assert_ne!(a, key("s1", &[1, 2, 3], "link2"));
    }

    #[test]
    fn an_entry_round_trips_and_counts() {
        let d = dir("round");
        let c = open(&d, DEFAULT_MAX_BYTES);
        let k = key("s", &[9], "l");
        assert!(c.get(k).is_none());
        c.put(k, "fn main() {}");
        assert!(c.contains(k));
        assert_eq!(c.get(k).as_deref(), Some("fn main() {}"));
        let s = c.stats();
        assert_eq!((s.hits, s.misses, s.stored, s.corrupt), (1, 1, 1, 0));
        assert!(c.summary().contains("1 hits, 1 misses"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn damaged_entries_are_misses_that_are_removed_never_a_failure() {
        let d = dir("damaged");
        let c = open(&d, DEFAULT_MAX_BYTES);
        let k = key("s", &[1], "l");
        let good = wrap(k, "text");
        let cases: Vec<Vec<u8>> = vec![
            Vec::new(),
            b"garbage".to_vec(),
            good[..good.len() - 1].to_vec(),
            {
                let mut b = good.clone();
                *b.last_mut().unwrap() ^= 1;
                b
            },
            {
                let mut b = good.clone();
                b[8] = 99;
                b
            },
            wrap(key("s", &[2], "l"), "text"),
        ];
        let n = cases.len() as u64;
        for bytes in cases {
            let p = c.path(k);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, &bytes).unwrap();
            assert!(c.get(k).is_none());
            assert!(!p.exists(), "a damaged entry is removed");
        }
        assert_eq!(c.stats().corrupt, n);
        c.put(k, "text");
        assert_eq!(c.get(k).as_deref(), Some("text"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_cap_evicts_the_least_recently_used_first() {
        let d = dir("lru");
        let c = open(&d, DEFAULT_MAX_BYTES);
        let text = "x".repeat(1000);
        let keys: Vec<u128> = (0..10).map(|n| key("s", &[n], "l")).collect();
        for &k in &keys {
            c.put(k, &text);
        }
        // Age them in order, then use the first: it is now the newest.
        let now = std::time::SystemTime::now();
        for (n, &k) in keys.iter().enumerate() {
            let f = std::fs::File::options()
                .write(true)
                .open(c.path(k))
                .unwrap();
            f.set_modified(now - std::time::Duration::from_secs(100_000 - n as u64 * 10))
                .unwrap();
        }
        assert!(c.get(keys[0]).is_some());
        let one = (HEADER + 1000) as u64;
        // A cap of five entries' worth: down to nine tenths of it, four whole entries.
        let small = ShaderCache {
            max_bytes: one * 5,
            dir: c.dir.clone(),
            root: c.root.clone(),
            stamp: c.stamp.clone(),
            counters: Counters::default(),
            failed: Default::default(),
        };
        assert_eq!(small.evict_to_cap(), 6);
        assert!(c.contains(keys[0]), "the one just used stays");
        assert!(!c.contains(keys[1]) && !c.contains(keys[6]));
        assert!(c.contains(keys[9]));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn another_stamp_has_its_own_folder_and_the_old_ones_go() {
        let d = dir("stamp");
        let a = ShaderCache::open(&settings(&d, DEFAULT_MAX_BYTES), "aaaaaaaaaaaaaaaa").unwrap();
        a.put(1, "old");
        std::fs::create_dir_all(d.join("modules").join("not-a-stamp")).unwrap();
        let b = ShaderCache::open(&settings(&d, DEFAULT_MAX_BYTES), "bbbbbbbbbbbbbbbb").unwrap();
        assert!(b.get(1).is_none());
        assert!(!d.join("modules").join("aaaaaaaaaaaaaaaa").exists());
        assert!(d.join("modules").join("not-a-stamp").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_switch_takes_off_a_path_or_the_default() {
        let def = PathBuf::from("/d");
        assert_eq!(Settings::resolve(None, None, Some(&def)).unwrap().root, def);
        assert!(Settings::resolve(Some("off"), None, Some(&def)).is_none());
        assert!(Settings::resolve(Some("OFF"), None, Some(&def)).is_none());
        assert!(Settings::resolve(None, None, None).is_none());
        let s = Settings::resolve(Some("/x"), Some(5), Some(&def)).unwrap();
        assert_eq!((s.root, s.max_bytes), (PathBuf::from("/x"), 5 << 20));
    }

    #[test]
    fn a_folder_that_cannot_be_made_is_no_cache_not_a_failure() {
        let d = dir("blocked");
        std::fs::create_dir_all(&d).unwrap();
        let file = d.join("file");
        std::fs::write(&file, b"x").unwrap();
        assert!(
            ShaderCache::open(&settings(&file, DEFAULT_MAX_BYTES), "0123456789abcdef").is_none()
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn concurrent_writers_of_one_key_leave_one_whole_entry() {
        let d = dir("conc");
        let c = open(&d, DEFAULT_MAX_BYTES);
        let k = key("s", &[7], "l");
        let hs: Vec<_> = (0..8)
            .map(|_| {
                let c = c.clone();
                std::thread::spawn(move || {
                    for _ in 0..20 {
                        c.put(k, "same bytes");
                        assert_eq!(c.get(k).as_deref(), Some("same bytes"));
                    }
                })
            })
            .collect();
        for h in hs {
            h.join().unwrap();
        }
        assert_eq!(c.stats().corrupt, 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A vs_2_0: `dcl_position v0; mov oPos, v0; mov oD0.xyz, c0`.
    const VS: [u32; 11] = [
        0xFFFE_0200,
        0x0200_001F,
        0x8000_0000,
        0x900F_0000,
        0x0200_0001,
        0xC00F_0000,
        0x90E4_0000,
        0x0200_0001,
        0xD007_0000,
        0xA0E4_0000,
        crate::sm::END,
    ];

    fn link() -> crate::sm::wgsl::Link {
        use crate::sm::wgsl::{centroid_everywhere, Input, Link, Semantic};
        let varyings: Vec<Semantic> = Vec::new();
        Link {
            inputs: vec![Input::Float(4)],
            centroid: centroid_everywhere(varyings.len()),
            varyings,
            ..Link::default()
        }
    }

    /// The same bytecode and link convert to the same text every time, and the cache hands back
    /// exactly that text.
    #[test]
    fn conversion_is_deterministic_and_the_cache_returns_it_unchanged() {
        let p = crate::sm::parse(&VS).unwrap();
        let first = crate::sm::wgsl::convert(&p, &link()).unwrap();
        for _ in 0..5 {
            assert_eq!(crate::sm::wgsl::convert(&p, &link()).unwrap(), first);
        }
        let d = dir("convert");
        let c = open(&d, DEFAULT_MAX_BYTES);
        let miss = convert(Some(&c), &VS, &p, &link()).unwrap();
        let hit = convert(Some(&c), &VS, &p, &link()).unwrap();
        assert_eq!(
            (miss.as_str(), hit.as_str()),
            (first.as_str(), first.as_str())
        );
        let s = c.stats();
        assert_eq!((s.hits, s.misses, s.stored), (1, 1, 1));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Two mods' shader sets overlap in some shaders: the cache is keyed by content, so the shared
    /// bytecode is one entry whichever set stored it, and each set adds only what it alone has.
    #[test]
    fn two_shader_sets_share_entries_by_content() {
        let d = dir("sets");
        let c = open(&d, DEFAULT_MAX_BYTES);
        let mut other = VS;
        other[9] = 0xA0E4_0001; // reads c1: other bytecode
        let shared = crate::sm::parse(&VS).unwrap();
        let only_b = crate::sm::parse(&other);
        // Set A: the shared shader under one name; set B: the same bytecode under another name
        // (nothing about a name reaches the key) and one of its own.
        convert(Some(&c), &VS, &shared, &link()).unwrap();
        let before = c.stats();
        convert(Some(&c), &VS, &shared, &link()).unwrap();
        let after = c.stats();
        assert_eq!(after.hits, before.hits + 1);
        assert_eq!(
            after.stored, before.stored,
            "the shared shader is not stored again"
        );
        let p = only_b.unwrap();
        convert(Some(&c), &other, &p, &link()).unwrap();
        assert_eq!(
            c.stats().stored,
            before.stored + 1,
            "only its own shader is new"
        );
        assert_ne!(key("s", &VS, "l"), key("s", &other, "l"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
