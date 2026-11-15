//! The shader compiler seam of D3DX off Windows (nv-linux#178, #197, #200).
//!
//! The game calls `D3DXCompileShader` and `D3DXAssembleShader`; on Windows the fork loads the
//! player's own `d3dx9_38.dll` for them and this crate is not involved. Everywhere else a
//! [`ShaderCompiler`] answers:
//!
//! * [`CacheCompiler`] looks the request up in the on-disk [`Cache`]. The cache holds one directory
//!   per compiler identity (Microsoft's DLL, Wine's built-in d3dx9, libvkd3d-shader are separate key
//!   spaces and are never mixed; lookups try them in that order of trust);
//! * on a miss it asks an optional backend (another [`ShaderCompiler`]) and stores the answer under
//!   the backend's identity;
//! * with no backend a miss is [`CompileError::Miss`], which names the key, the target and the
//!   entry point and carries neither source text nor a path.
//!
//! Nothing here is game data: no shader, no cache file and no compiler binary is committed.

pub mod cache;
pub mod sha256;

pub use cache::{Cache, CacheCompiler};

use std::fmt;

/// What the game asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `D3DXCompileShader`: HLSL source to bytecode.
    Compile,
    /// `D3DXAssembleShader`: assembly source to bytecode.
    Assemble,
}

/// One `D3DXMACRO`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Macro {
    pub name: Vec<u8>,
    pub value: Vec<u8>,
}

/// A request to the compiler: every input the compiler reads except the include files (those are
/// asked for through [`Includes`] while it runs).
#[derive(Clone, Copy, Debug)]
pub struct Request<'a> {
    pub kind: Kind,
    pub source: &'a [u8],
    /// The entry point (`None` when the game passes none; D3DX then uses `main`).
    pub entry: Option<&'a [u8]>,
    /// The target profile such as `ps_2_0` (`None` for assembly).
    pub target: Option<&'a [u8]>,
    pub flags: u32,
    /// In the order the game gave them.
    pub macros: &'a [Macro],
}

/// An include file the compiler asked for, with the contents it was given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Include {
    pub name: Vec<u8>,
    /// `D3DXINC_LOCAL` (quoted) rather than `D3DXINC_SYSTEM` (angle brackets).
    pub local: bool,
    pub content: Vec<u8>,
}

/// The game's `ID3DXInclude`.
pub trait Includes {
    /// The contents of the include `name`, or `None` when the game's handler fails.
    fn open(&mut self, name: &[u8], local: bool) -> Option<Vec<u8>>;
}

impl<F: FnMut(&[u8], bool) -> Option<Vec<u8>>> Includes for F {
    fn open(&mut self, name: &[u8], local: bool) -> Option<Vec<u8>> {
        self(name, local)
    }
}

/// No include handler: every include fails.
pub struct NoIncludes;

impl Includes for NoIncludes {
    fn open(&mut self, _: &[u8], _: bool) -> Option<Vec<u8>> {
        None
    }
}

/// What a compiler made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    /// The d3dbc token stream (with its constant table comment).
    pub bytecode: Vec<u8>,
    /// The includes the compiler read, in the order it read them (empty for a cache hit's own
    /// accounting: the cache has verified them already).
    pub includes: Vec<Include>,
}

/// Why a request got no bytecode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompileError {
    /// The cache has no entry and no backend is available to make one. Names the request key, the
    /// target and the entry point; never the source and never a path.
    Miss {
        key: String,
        target: String,
        entry: String,
    },
    /// The compiler ran and refused: its diagnostics, which the game receives as the error buffer.
    Failed { messages: Vec<u8> },
    /// The backend cannot be used on this machine (the library is missing, too old, ...). The text
    /// says what to do.
    Unavailable { reason: String },
    /// The backend does not do this kind of request.
    Unsupported { reason: String },
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompileError::Miss { key, target, entry } => write!(
                f,
                "no compiled shader for request {key} (target {target}, entry {entry}) in the \
                 shader cache, and no shader compiler is available: install libvkd3d-shader \
                 (see docs/fork/LINUX.md) or record the shader into the cache (NV_D3DX_CACHE)"
            ),
            CompileError::Failed { messages } => {
                write!(
                    f,
                    "the shader compiler refused the source: {}",
                    String::from_utf8_lossy(messages)
                )
            }
            CompileError::Unavailable { reason } | CompileError::Unsupported { reason } => {
                f.write_str(reason)
            }
        }
    }
}

impl std::error::Error for CompileError {}

/// A shader compiler: HLSL or assembly in, d3dbc out.
pub trait ShaderCompiler {
    /// What the compiler is: names the cache directory and goes into every key. Stable for one
    /// build of one compiler, different for any other (a library version, a DLL hash).
    fn identity(&self) -> String;

    fn compile(
        &self,
        request: &Request<'_>,
        includes: &mut dyn Includes,
    ) -> Result<Output, CompileError>;
}

/// The constant table of d3dbc bytecode: the payload of its `CTAB` comment without the tag, which is
/// what `ID3DXConstantTable::GetBufferPointer` points at (it begins with the table's size field).
/// `None` when the bytecode has none or is malformed.
pub fn constant_table(bytecode: &[u8]) -> Option<&[u8]> {
    let word = |i: usize| {
        bytecode
            .get(4 * i..4 * i + 4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
    };
    let mut at = 1;
    loop {
        let token = word(at)?;
        at += 1;
        if token == 0x0000_ffff {
            return None;
        }
        if token & 0xffff == 0xfffe {
            let len = ((token >> 16) & 0x7fff) as usize;
            if len >= 1 {
                word(at + len - 1)?;
                if word(at)? == 0x4241_5443 {
                    return bytecode.get(4 * (at + 1)..4 * (at + len));
                }
            }
            at += len;
        }
    }
}

/// The length in bytes of the d3dbc token stream at the start of `words`' bytes, up to and
/// including the end token; `None` when it has no end token.
pub fn bytecode_length(bytes: &[u8]) -> Option<usize> {
    let word = |i: usize| {
        bytes
            .get(4 * i..4 * i + 4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
    };
    let mut at = 1;
    loop {
        let token = word(at)?;
        at += 1;
        if token == 0x0000_ffff {
            return Some(4 * at);
        }
        if token & 0xffff == 0xfffe {
            at += ((token >> 16) & 0x7fff) as usize;
        }
    }
}

/// The directory-name form of an identity: letters, digits, `.`, `_` and `-`, the rest `_`.
pub fn identity_dir(identity: &str) -> String {
    identity
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn field(h: &mut sha256::Sha256, bytes: &[u8]) {
    h.update(&(bytes.len() as u64).to_le_bytes());
    h.update(bytes);
}

fn optional(h: &mut sha256::Sha256, bytes: Option<&[u8]>) {
    match bytes {
        None => h.update(&[0]),
        Some(b) => {
            h.update(&[1]);
            field(h, b);
        }
    }
}

/// The request key: over the kind, the source, the entry, the target, the flags and the macros
/// (name and value, in order). Everything the compiler reads but the includes and its own
/// identity ([`entry_key`] adds those).
pub fn request_key(r: &Request<'_>) -> [u8; 32] {
    let mut h = sha256::Sha256::new();
    field(&mut h, b"nv-d3dx-shader-request-v1");
    h.update(&[match r.kind {
        Kind::Compile => 0,
        Kind::Assemble => 1,
    }]);
    field(&mut h, r.source);
    optional(&mut h, r.entry);
    optional(&mut h, r.target);
    h.update(&r.flags.to_le_bytes());
    h.update(&(r.macros.len() as u64).to_le_bytes());
    for m in r.macros {
        field(&mut h, &m.name);
        field(&mut h, &m.value);
    }
    h.finish()
}

/// The key of an entry: the request key, the directory form of the compiler's identity and the
/// include files (name, kind and contents, in the order they were read).
pub fn entry_key(request: &[u8; 32], identity: &str, includes: &[Include]) -> [u8; 32] {
    let mut h = sha256::Sha256::new();
    field(&mut h, b"nv-d3dx-shader-entry-v1");
    h.update(request);
    field(&mut h, identity_dir(identity).as_bytes());
    h.update(&(includes.len() as u64).to_le_bytes());
    for i in includes {
        field(&mut h, &i.name);
        h.update(&[u8::from(i.local)]);
        field(&mut h, &i.content);
    }
    h.finish()
}

/// [`entry_key`] of a request in one go.
pub fn key(r: &Request<'_>, identity: &str, includes: &[Include]) -> [u8; 32] {
    entry_key(&request_key(r), identity, includes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Request<'static> {
        Request {
            kind: Kind::Compile,
            source: b"float4 main() : COLOR { return 1; }",
            entry: Some(b"main"),
            target: Some(b"ps_2_0"),
            flags: 0,
            macros: &[],
        }
    }

    fn hex(k: [u8; 32]) -> String {
        sha256::hex(&k)
    }

    #[test]
    fn key_is_stable() {
        assert_eq!(
            hex(key(&base(), "ms-1", &[])),
            hex(key(&base(), "ms-1", &[]))
        );
    }

    #[test]
    fn key_changes_with_each_field() {
        let reference = hex(key(&base(), "ms-1", &[]));
        let macros = [Macro {
            name: b"HDR".to_vec(),
            value: b"1".to_vec(),
        }];
        let variants = [
            Request {
                kind: Kind::Assemble,
                ..base()
            },
            Request {
                source: b"float4 main() : COLOR { return 2; }",
                ..base()
            },
            Request {
                entry: Some(b"other"),
                ..base()
            },
            Request {
                entry: None,
                ..base()
            },
            Request {
                target: Some(b"ps_3_0"),
                ..base()
            },
            Request {
                target: None,
                ..base()
            },
            Request {
                flags: 1 << 4,
                ..base()
            },
            Request {
                macros: &macros,
                ..base()
            },
        ];
        let mut seen = vec![reference];
        for v in variants {
            let k = hex(key(&v, "ms-1", &[]));
            assert!(!seen.contains(&k), "a field did not change the key");
            seen.push(k);
        }
        assert!(!seen.contains(&hex(key(&base(), "ms-2", &[]))));
    }

    #[test]
    fn macros_count_name_value_and_order() {
        let m = |n: &str, v: &str| Macro {
            name: n.into(),
            value: v.into(),
        };
        let ab = [m("A", "1"), m("B", "2")];
        let ba = [m("B", "2"), m("A", "1")];
        let joined = [m("A", "12")];
        let swapped = [m("A", "2"), m("B", "1")];
        let keys: Vec<_> = [&ab[..], &ba, &joined, &swapped]
            .iter()
            .map(|ms| {
                hex(key(
                    &Request {
                        macros: ms,
                        ..base()
                    },
                    "x",
                    &[],
                ))
            })
            .collect();
        for i in 0..keys.len() {
            for j in 0..i {
                assert_ne!(keys[i], keys[j]);
            }
        }
    }

    #[test]
    fn includes_count_with_name_kind_and_contents() {
        let inc = |n: &str, local: bool, c: &str| Include {
            name: n.into(),
            local,
            content: c.into(),
        };
        let r = base();
        let keys = [
            hex(key(&r, "x", &[])),
            hex(key(&r, "x", &[inc("a.h", true, "1")])),
            hex(key(&r, "x", &[inc("a.h", true, "2")])),
            hex(key(&r, "x", &[inc("b.h", true, "1")])),
            hex(key(&r, "x", &[inc("a.h", false, "1")])),
        ];
        for i in 0..keys.len() {
            for j in 0..i {
                assert_ne!(keys[i], keys[j]);
            }
        }
    }

    #[test]
    fn identity_is_compared_by_its_directory_form() {
        assert_eq!(
            identity_dir("vkd3d-shader 1.19.0 (git x)"),
            "vkd3d-shader_1.19.0__git_x_"
        );
        assert_eq!(hex(key(&base(), "a b", &[])), hex(key(&base(), "a_b", &[])));
    }

    /// Hand-built vs_2_0: version, a comment holding a CTAB tag and `payload`, `mov oPos, v0`, end.
    fn with_ctab(payload: &[u32]) -> Vec<u8> {
        let mut w = vec![
            0xfffe_0200u32,
            0x0000_fffe | ((payload.len() as u32 + 1) << 16),
            0x4241_5443,
        ];
        w.extend_from_slice(payload);
        w.extend([0x0200_0001, 0x400f_0000, 0x10e4_0000, 0x0000_ffff]);
        w.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    #[test]
    fn constant_table_is_found_in_the_comment() {
        let bytes = with_ctab(&[0x1c, 0xdead_beef]);
        let table = constant_table(&bytes).unwrap();
        assert_eq!(table, [0x1c, 0, 0, 0, 0xef, 0xbe, 0xad, 0xde]);
        assert_eq!(bytecode_length(&bytes), Some(bytes.len()));
        // No comment at all, and a comment of another kind.
        let plain: Vec<u8> = [
            0xfffe_0200u32,
            0x0200_0001,
            0x400f_0000,
            0x10e4_0000,
            0x0000_ffff,
        ]
        .iter()
        .flat_map(|x| x.to_le_bytes())
        .collect();
        assert_eq!(constant_table(&plain), None);
        let other: Vec<u8> = [0xfffe_0200u32, 0x0001_fffe, 0x1234_5678, 0x0000_ffff]
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .collect();
        assert_eq!(constant_table(&other), None);
        // Truncated: never a panic.
        assert_eq!(constant_table(&bytes[..10]), None);
        assert_eq!(bytecode_length(&bytes[..bytes.len() - 4]), None);
    }

    #[test]
    fn miss_message_has_no_source_or_path() {
        let source = "SECRET_SOURCE_TEXT";
        let r = Request {
            source: source.as_bytes(),
            ..base()
        };
        let e = CompileError::Miss {
            key: hex(request_key(&r)),
            target: "ps_2_0".into(),
            entry: "main".into(),
        };
        let text = e.to_string();
        assert!(
            text.contains("ps_2_0")
                && text.contains("main")
                && text.contains(&hex(request_key(&r)))
        );
        assert!(!text.contains(source));
    }
}
