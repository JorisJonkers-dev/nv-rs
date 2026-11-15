//! The on-disk cache of compiled shaders.
//!
//! Layout: `<root>/<identity dir>/<entry key hex>.bin` holds the bytecode of one entry byte for
//! byte; `<root>/<identity dir>/<request key hex>.inc` lists the include files the compile read
//! (so a lookup can ask the game for them and fold their contents into the key). The root is
//! `$NV_D3DX_CACHE/shaders` (the folder that already names the texture cache), else
//! `$XDG_CACHE_HOME/nv-rs/d3dx-cache/shaders`, else `$HOME/.cache/nv-rs/d3dx-cache/shaders`.

use crate::sha256::hex;
use crate::{
    entry_key, identity_dir, request_key, CompileError, Include, Includes, Output, Request,
    ShaderCompiler,
};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The cache folder from the environment (`get` reads one variable).
pub fn root_from_env(get: impl Fn(&str) -> Option<std::ffi::OsString>) -> Option<PathBuf> {
    let non_empty = |name: &str| get(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(dir) = non_empty("NV_D3DX_CACHE") {
        return Some(dir.join("shaders"));
    }
    let base =
        non_empty("XDG_CACHE_HOME").or_else(|| non_empty("HOME").map(|h| h.join(".cache")))?;
    Some(base.join("nv-rs").join("d3dx-cache").join("shaders"))
}

/// The cache folder of this process.
pub fn default_root() -> Option<PathBuf> {
    root_from_env(|n| std::env::var_os(n))
}

/// The order of trust of the identity directories: Microsoft's DLL, then Wine's built-in d3dx9,
/// then everything else (libvkd3d-shader), each group by name.
fn rank(dir: &str) -> u8 {
    if dir.starts_with("ms-") {
        0
    } else if dir.starts_with("wine-") {
        1
    } else {
        2
    }
}

/// A cache folder.
#[derive(Clone, Debug)]
pub struct Cache {
    root: PathBuf,
}

impl Cache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Cache { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The identity directories present, in the order of trust.
    pub fn identities(&self) -> Vec<String> {
        let mut dirs: Vec<String> = fs::read_dir(&self.root)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        dirs.sort_by(|a, b| (rank(a), a).cmp(&(rank(b), b)));
        dirs
    }

    fn includes_of(
        &self,
        dir: &str,
        request: &[u8; 32],
        includes: &mut dyn Includes,
    ) -> Option<Vec<Include>> {
        let Ok(manifest) = fs::read(self.root.join(dir).join(format!("{}.inc", hex(request))))
        else {
            return Some(Vec::new());
        };
        let mut found = Vec::new();
        for line in manifest.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
            let (local, name) = match line.split_first()? {
                (b'L', rest) => (true, rest),
                (b'S', rest) => (false, rest),
                _ => return None,
            };
            let content = includes.open(name, local)?;
            found.push(Include {
                name: name.to_vec(),
                local,
                content,
            });
        }
        Some(found)
    }

    /// The recorded bytecode of `request`, with the identity directory that held it. The include
    /// files a recorded compile read are fetched through `includes` and are part of the key: a
    /// changed include is a miss.
    pub fn lookup(
        &self,
        request: &Request<'_>,
        includes: &mut dyn Includes,
    ) -> Option<(String, Vec<u8>)> {
        let base = request_key(request);
        for dir in self.identities() {
            let Some(read) = self.includes_of(&dir, &base, includes) else {
                continue;
            };
            let key = entry_key(&base, &dir, &read);
            if let Ok(bytes) = fs::read(self.root.join(&dir).join(format!("{}.bin", hex(&key)))) {
                return Some((dir, bytes));
            }
        }
        None
    }

    /// Records `bytecode` for `request` under `identity` (the include files the compile read
    /// are part of the entry). Written to a temporary file and renamed, so a reader never sees a
    /// half-written entry.
    pub fn store(
        &self,
        identity: &str,
        request: &Request<'_>,
        includes: &[Include],
        bytecode: &[u8],
    ) -> io::Result<()> {
        let dir = identity_dir(identity);
        let base = request_key(request);
        let folder = self.root.join(&dir);
        fs::create_dir_all(&folder)?;
        let write = |name: String, bytes: &[u8]| -> io::Result<()> {
            let temp = folder.join(format!("{name}.tmp{}", std::process::id()));
            fs::write(&temp, bytes)?;
            fs::rename(&temp, folder.join(name))
        };
        if !includes.is_empty() {
            let mut manifest = Vec::new();
            for i in includes {
                if i.name.contains(&b'\n') {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "an include name with a line break",
                    ));
                }
                manifest.push(if i.local { b'L' } else { b'S' });
                manifest.extend_from_slice(&i.name);
                manifest.push(b'\n');
            }
            write(format!("{}.inc", hex(&base)), &manifest)?;
        }
        write(
            format!("{}.bin", hex(&entry_key(&base, &dir, includes))),
            bytecode,
        )
    }
}

/// The compiler of other targets: the cache first, then the backend when there is one.
pub struct CacheCompiler {
    cache: Cache,
    backend: Option<Box<dyn ShaderCompiler>>,
}

impl CacheCompiler {
    pub fn new(cache: Cache, backend: Option<Box<dyn ShaderCompiler>>) -> Self {
        CacheCompiler { cache, backend }
    }

    pub fn cache(&self) -> &Cache {
        &self.cache
    }

    fn miss(request: &Request<'_>) -> CompileError {
        CompileError::Miss {
            key: hex(&request_key(request)),
            target: String::from_utf8_lossy(request.target.unwrap_or(b"(none)")).into_owned(),
            entry: String::from_utf8_lossy(request.entry.unwrap_or(b"main")).into_owned(),
        }
    }
}

impl ShaderCompiler for CacheCompiler {
    fn identity(&self) -> String {
        "cache".into()
    }

    fn compile(
        &self,
        request: &Request<'_>,
        includes: &mut dyn Includes,
    ) -> Result<Output, CompileError> {
        if let Some((_, bytecode)) = self.cache.lookup(request, includes) {
            return Ok(Output {
                bytecode,
                includes: Vec::new(),
            });
        }
        let Some(backend) = &self.backend else {
            return Err(Self::miss(request));
        };
        let made = backend.compile(request, includes)?;
        // A cache that cannot be written (read-only folder) does not stop the run.
        let _ = self
            .cache
            .store(&backend.identity(), request, &made.includes, &made.bytecode);
        Ok(made)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Kind, Macro, NoIncludes};
    use std::cell::Cell;
    use std::rc::Rc;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("d3dx-shader-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn request(source: &[u8]) -> Request<'_> {
        Request {
            kind: Kind::Compile,
            source,
            entry: Some(b"main"),
            target: Some(b"ps_2_0"),
            flags: 0,
            macros: &[],
        }
    }

    /// Hand-built ps_2_0: version token, `mov oC0, c0`, end.
    fn ps_2_0() -> Vec<u8> {
        [
            0xffff_0200u32,
            0x0200_0001,
            0x800f_0800,
            0xa0e4_0000,
            0x0000_ffff,
        ]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect()
    }

    struct Fixed(Vec<u8>, &'static str, Rc<Cell<u32>>);

    impl ShaderCompiler for Fixed {
        fn identity(&self) -> String {
            self.1.into()
        }
        fn compile(&self, r: &Request<'_>, inc: &mut dyn Includes) -> Result<Output, CompileError> {
            self.2.set(self.2.get() + 1);
            let mut seen = Vec::new();
            if r.source.starts_with(b"#include") {
                let content = inc.open(b"a.h", true).ok_or(CompileError::Failed {
                    messages: b"no include".to_vec(),
                })?;
                seen.push(Include {
                    name: b"a.h".to_vec(),
                    local: true,
                    content,
                });
            }
            Ok(Output {
                bytecode: self.0.clone(),
                includes: seen,
            })
        }
    }

    #[test]
    fn round_trip_is_byte_for_byte() {
        let dir = scratch("round");
        let cache = Cache::new(&dir);
        let r = request(b"synthetic");
        assert!(cache.lookup(&r, &mut NoIncludes).is_none());
        cache.store("ms-d3dx9_38-test", &r, &[], &ps_2_0()).unwrap();
        let (identity, bytes) = cache.lookup(&r, &mut NoIncludes).unwrap();
        assert_eq!(identity, "ms-d3dx9_38-test");
        assert_eq!(bytes, ps_2_0());
        let other = Request {
            target: Some(b"ps_3_0"),
            ..r
        };
        assert!(cache.lookup(&other, &mut NoIncludes).is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn identities_are_separate_key_spaces_and_microsoft_wins() {
        let dir = scratch("spaces");
        let cache = Cache::new(&dir);
        let r = request(b"synthetic");
        cache.store("vkd3d-shader 1.19", &r, &[], b"vkd3d").unwrap();
        cache.store("wine-d3dx9-9", &r, &[], b"wine").unwrap();
        assert_eq!(cache.lookup(&r, &mut NoIncludes).unwrap().1, b"wine");
        cache.store("ms-d3dx9_38-abc", &r, &[], b"ms").unwrap();
        assert_eq!(cache.lookup(&r, &mut NoIncludes).unwrap().1, b"ms");
        assert_eq!(
            cache.identities(),
            ["ms-d3dx9_38-abc", "wine-d3dx9-9", "vkd3d-shader_1.19"]
        );
        // An entry copied into the wrong identity directory is not found: the directory is in the key.
        let from = dir.join("ms-d3dx9_38-abc");
        let stray = dir.join("zzz");
        fs::create_dir_all(&stray).unwrap();
        for e in fs::read_dir(&from).unwrap().flatten() {
            fs::copy(e.path(), stray.join(e.file_name())).unwrap();
        }
        fs::remove_dir_all(&from).unwrap();
        fs::remove_dir_all(dir.join("wine-d3dx9-9")).unwrap();
        fs::remove_dir_all(dir.join("vkd3d-shader_1.19")).unwrap();
        assert!(cache.lookup(&r, &mut NoIncludes).is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_changed_include_is_a_miss() {
        let dir = scratch("include");
        let cache = Cache::new(&dir);
        let r = request(b"#include \"a.h\"");
        let inc = Include {
            name: b"a.h".to_vec(),
            local: true,
            content: b"one".to_vec(),
        };
        cache.store("ms-x", &r, &[inc], b"bytes").unwrap();
        let mut same = |_: &[u8], _| Some(b"one".to_vec());
        assert_eq!(cache.lookup(&r, &mut same).unwrap().1, b"bytes");
        let mut changed = |_: &[u8], _| Some(b"two".to_vec());
        assert!(cache.lookup(&r, &mut changed).is_none());
        assert!(cache.lookup(&r, &mut NoIncludes).is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn miss_without_backend_is_the_named_error() {
        let dir = scratch("miss");
        let compiler = CacheCompiler::new(Cache::new(&dir), None);
        let macros = [Macro {
            name: b"HDR".to_vec(),
            value: b"1".to_vec(),
        }];
        let source = "SECRET_SOURCE_TEXT";
        let r = Request {
            macros: &macros,
            ..request(source.as_bytes())
        };
        let err = compiler.compile(&r, &mut NoIncludes).unwrap_err();
        let CompileError::Miss { key, target, entry } = &err else {
            panic!("{err:?}")
        };
        assert_eq!((target.as_str(), entry.as_str()), ("ps_2_0", "main"));
        assert_eq!(key, &hex(&request_key(&r)));
        let text = err.to_string();
        assert!(!text.contains(source), "source text in the error");
        assert!(!text.contains(dir.to_str().unwrap()), "a path in the error");
    }

    #[test]
    fn backend_answer_is_stored_and_served_from_the_cache() {
        let dir = scratch("backend");
        let calls = Rc::new(Cell::new(0));
        let backend = Fixed(ps_2_0(), "vkd3d-shader 9.9", calls.clone());
        let compiler = CacheCompiler::new(Cache::new(&dir), Some(Box::new(backend)));
        let r = request(b"#include \"a.h\"");
        let mut inc = |_: &[u8], _| Some(b"one".to_vec());
        let first = compiler.compile(&r, &mut inc).unwrap();
        assert_eq!(first.bytecode, ps_2_0());
        assert_eq!(first.includes.len(), 1);
        let second = compiler.compile(&r, &mut inc).unwrap();
        assert_eq!(second.bytecode, ps_2_0());
        assert_eq!(calls.get(), 1, "the second request is a cache hit");
        let mut changed = |_: &[u8], _| Some(b"two".to_vec());
        compiler.compile(&r, &mut changed).unwrap();
        assert_eq!(calls.get(), 2);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn root_follows_the_environment() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |n: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == n)
                    .map(|(_, v)| std::ffi::OsString::from(v))
            }
        };
        assert_eq!(
            root_from_env(env(&[("NV_D3DX_CACHE", "/c"), ("HOME", "/h")])),
            Some("/c/shaders".into())
        );
        assert_eq!(
            root_from_env(env(&[("XDG_CACHE_HOME", "/x"), ("HOME", "/h")])),
            Some("/x/nv-rs/d3dx-cache/shaders".into())
        );
        assert_eq!(
            root_from_env(env(&[("HOME", "/h")])),
            Some("/h/.cache/nv-rs/d3dx-cache/shaders".into())
        );
        assert_eq!(root_from_env(env(&[])), None);
    }
}
