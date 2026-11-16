//! Precompiling the shaders of a load order without the game (`d3d9-precompile`,
//! `nv-viewer --precompile-shaders`, docs/fork/SHADER-CACHE.md).
//!
//! The roots are Data folders (or shader package files) in load order, lowest priority first.
//! A package is found the way the game finds it: `Shaders/*.sdp` (or `*.sdp` at the top of the
//! folder), and a package of the same name in a later root replaces the earlier one (a mod's
//! loose replacement). What is precompiled is what the game would load: the winning packages'
//! shaders. The bytecodes are deduplicated by content, so two packages, or two mods, that carry
//! the same shader make it once, and nothing about a name, a mod or the order reaches the caches.
//!
//! The work is the device's own start ([`crate::device::Device`]): an offscreen device is made,
//! every shader is created on it (its module is converted, through the converted-shader store
//! when there is one, and made on every core, with its pipeline library), the driver's pipeline
//! cache and the pipeline list are written. A shader the caches already hold costs a lookup
//! (the driver's cache returns its library at once), so a second run after a mod change makes
//! only the new hashes.

use d3d9_stream::{Backend, Cmd, Data};
use std::path::{Path, PathBuf};

/// What to precompile and where the caches live.
pub struct Options {
    /// Data folders or package files, lowest priority first.
    pub roots: Vec<PathBuf>,
    /// The converted-shader store (`None`: off).
    pub shader_cache: Option<crate::shader_cache::Settings>,
    /// The driver's pipeline cache and list folder.
    pub pipelines: Option<PathBuf>,
}

/// What a run did.
#[derive(Debug, Default, Clone)]
pub struct Report {
    pub packages: usize,
    /// Packages replaced by a same-named one of a later root.
    pub overridden: usize,
    pub shaders: usize,
    /// Distinct bytecodes.
    pub unique: usize,
    /// Shaders no reader or converter took (named by position, never by content).
    pub unreadable: usize,
    /// Modules, libraries and so on as the device counted them.
    pub waited_for: usize,
    pub elapsed: std::time::Duration,
}

/// The shader packages under `roots`, with a same-named package of a later root replacing an
/// earlier one: `(path, replaced)` in load order.
pub fn find_packages(roots: &[PathBuf]) -> (Vec<PathBuf>, usize) {
    let mut by_name: Vec<(String, PathBuf)> = Vec::new();
    let mut overridden = 0;
    let mut add = |p: PathBuf| {
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if let Some(slot) = by_name.iter_mut().find(|(n, _)| *n == name) {
            slot.1 = p;
            overridden += 1;
        } else {
            by_name.push((name, p));
        }
    };
    for root in roots {
        if root.is_file() {
            add(root.clone());
            continue;
        }
        for dir in [root.join("Shaders"), root.clone()] {
            // The folder's own name may be any case on a case-sensitive file system.
            let dir = case_insensitive_child(root, &dir);
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut files: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("sdp"))
                })
                .collect();
            files.sort();
            for f in files {
                add(f);
            }
        }
    }
    (by_name.into_iter().map(|(_, p)| p).collect(), overridden)
}

/// `dir` itself, or (when `dir` is `root/Shaders` and absent) the child of `root` whose name is
/// `Shaders` in any case.
fn case_insensitive_child(root: &Path, dir: &Path) -> PathBuf {
    if dir.exists() || dir == root {
        return dir.to_path_buf();
    }
    let want = dir.file_name().map(|n| n.to_ascii_lowercase());
    if let Ok(entries) = std::fs::read_dir(root) {
        for e in entries.flatten() {
            if Some(e.file_name().to_ascii_lowercase()) == want && e.path().is_dir() {
                return e.path();
            }
        }
    }
    dir.to_path_buf()
}

/// Every distinct bytecode of the packages, as tokens, in first-seen order, and how many shaders
/// the packages held and how many were unreadable.
pub fn collect(packages: &[PathBuf]) -> (Vec<Vec<u32>>, usize, usize) {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    let (mut total, mut bad) = (0, 0);
    for path in packages {
        let Ok(bytes) = std::fs::read(path) else {
            bad += 1;
            continue;
        };
        let Ok(pkg) = crate::sm::package::Package::parse(&bytes) else {
            bad += 1;
            continue;
        };
        for s in pkg.shaders {
            total += 1;
            if seen.insert(crate::shader_cache::hash128(&[&s.bytecode])) {
                out.push(
                    s.bytecode
                        .chunks_exact(4)
                        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .collect(),
                );
            }
        }
    }
    (out, total, bad)
}

fn words(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// Precompiles everything `options` names. An error is only that no GPU device could be made.
pub fn run(options: &Options) -> Result<Report, String> {
    let t = std::time::Instant::now();
    let (packages, overridden) = find_packages(&options.roots);
    let (shaders, total, bad) = collect(&packages);
    let mut report = Report {
        packages: packages.len(),
        overridden,
        shaders: total,
        unique: shaders.len(),
        unreadable: bad,
        ..Report::default()
    };
    let mut b = crate::WgpuBackend::new(crate::Output::Offscreen)
        .with_shader_cache(options.shader_cache.as_ref());
    if let Some(dir) = &options.pipelines {
        b = b.with_cache_dir(dir.clone());
    }
    // A 64x64 target: the device exists to compile, not to draw.
    let present = words(&[64, 64, 21, 1, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0]);
    let back = words(&[0x10]);
    b.execute(Cmd::CreateDevice {
        device: 1,
        focus: 0,
        window: 0,
        behavior: 0x54,
        present: Data::Bytes(&present),
        back_buffers: Data::Bytes(&back),
        depth_stencil: 0,
    });
    let Some(d) = b.device() else {
        return Err("no GPU device could be made".into());
    };
    // Every shader's library, listed or not: a precompile is what makes the unlisted ones'.
    d.precompiling();
    for (n, tokens) in shaders.iter().enumerate() {
        let bytes = words(tokens);
        b.execute(Cmd::CreateShader {
            device: 1,
            shader: 0x1000 + n as d3d9_stream::Handle,
            pixel: tokens.first().is_some_and(|v| v >> 16 == 0xFFFF),
            tokens: Data::Bytes(&bytes),
        });
    }
    // The first present ends the start: every queued module and library is waited for, behind
    // the loading screen's own progress.
    b.execute(Cmd::Present {
        device: 1,
        source: None,
        dest: None,
        window: 0,
        dirty: false,
        model: 0,
        frame: 1,
    });
    if let Some(d) = b.device() {
        report.waited_for = d.start_waited_for();
    }
    crate::save_pipeline_caches();
    drop(b);
    report.elapsed = t.elapsed();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(shaders: &[(&str, &[u32])]) -> Vec<u8> {
        let mut out = vec![0u8; 12];
        out[4..8].copy_from_slice(&(shaders.len() as u32).to_le_bytes());
        for (name, tokens) in shaders {
            let mut n = [0u8; 256];
            n[..name.len()].copy_from_slice(name.as_bytes());
            out.extend_from_slice(&n);
            out.extend_from_slice(&((tokens.len() * 4) as u32).to_le_bytes());
            for t in *tokens {
                out.extend_from_slice(&t.to_le_bytes());
            }
        }
        out
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nv-precompile-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const A: [u32; 2] = [0xFFFE_0200, crate::sm::END];
    const B: [u32; 2] = [0xFFFF_0300, crate::sm::END];

    /// A later root's package of the same name replaces the earlier one (a mod's loose file), a
    /// package only one root has stays, and shaders common to the surviving packages count once.
    #[test]
    fn a_later_root_replaces_a_package_and_shaders_are_counted_by_content() {
        let base = tmp("base");
        let moda = tmp("mod");
        std::fs::create_dir_all(base.join("Shaders")).unwrap();
        std::fs::create_dir_all(moda.join("shaders")).unwrap();
        std::fs::write(
            base.join("Shaders/shaderpackage001.sdp"),
            package(&[("a.vso", &A)]),
        )
        .unwrap();
        std::fs::write(
            base.join("Shaders/shaderpackage002.sdp"),
            package(&[("b.pso", &B)]),
        )
        .unwrap();
        // The mod ships package 001 again, under another case, with the same shader A under
        // another name and a pixel shader of its own.
        let c = [0xFFFF_0300, 0x0200_0001, crate::sm::END];
        std::fs::write(
            moda.join("shaders/SHADERPACKAGE001.sdp"),
            package(&[("renamed.vso", &A), ("new.pso", &c)]),
        )
        .unwrap();
        let (packages, overridden) = find_packages(&[base.clone(), moda.clone()]);
        assert_eq!((packages.len(), overridden), (2, 1));
        let (shaders, total, bad) = collect(&packages);
        // 001 (the mod's): A, c; 002: B. Three distinct, three in all.
        assert_eq!((shaders.len(), total, bad), (3, 3, 0));
        // The same two packages in the other order: the base replaces the mod's; A and B only.
        let (packages, _) = find_packages(&[moda.clone(), base.clone()]);
        let (shaders, ..) = collect(&packages);
        assert_eq!(shaders.len(), 2);
        let _ = std::fs::remove_dir_all(base);
        let _ = std::fs::remove_dir_all(moda);
    }

    /// Two different load orders that carry the same bytecode share cache keys: the key is the
    /// content, so the shaders both orders have are the same set.
    #[test]
    fn the_same_bytecode_in_two_load_orders_is_the_same_set_of_keys() {
        let a = tmp("order-a");
        let b = tmp("order-b");
        std::fs::write(a.join("p1.sdp"), package(&[("x.vso", &A)])).unwrap();
        std::fs::write(b.join("zzz.sdp"), package(&[("other-name.vso", &A)])).unwrap();
        let (pa, _) = find_packages(std::slice::from_ref(&a));
        let (pb, _) = find_packages(std::slice::from_ref(&b));
        assert_eq!(collect(&pa).0, collect(&pb).0);
        let _ = std::fs::remove_dir_all(a);
        let _ = std::fs::remove_dir_all(b);
    }

    #[test]
    fn a_damaged_package_is_counted_not_fatal() {
        let d = tmp("bad");
        std::fs::write(d.join("p.sdp"), b"not a package").unwrap();
        let (p, _) = find_packages(std::slice::from_ref(&d));
        assert_eq!(collect(&p), (Vec::new(), 0, 1));
        let _ = std::fs::remove_dir_all(d);
    }
}
