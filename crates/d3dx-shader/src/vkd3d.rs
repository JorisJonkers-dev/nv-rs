//! The libvkd3d-shader backend (nv-linux#200): HLSL (Shader Model 1 to 3 profiles) to d3dbc.
//!
//! The library is loaded with `dlopen` at the first use (nothing of it is linked, built or
//! shipped by this repository). libvkd3d-shader is LGPL-2.1-or-later (upstream COPYING of the
//! bound API, checked: see docs/fork/LINUX.md); the fork is GPL-3.0-or-later. A machine without
//! the library gets [`CompileError::Unavailable`] with what to install.
//!
//! The library has no assembler for d3d assembly text (its source types are HLSL, d3d bytecode,
//! TPF, DXIL, FX and TX; `D3D_ASM` is a target only), so [`Kind::Assemble`] is
//! [`CompileError::Unsupported`] here; the cache answers assembly that was recorded.
//!
//! The structures below are the C ABI of `vkd3d_shader.h` (libvkd3d-shader.so.1, API 1.x).

use crate::{CompileError, Include, Includes, Kind, Output, Request, ShaderCompiler};
use libloading::Library;
use std::ffi::{c_char, c_int, c_uint, c_void, CStr, CString};

const STRUCT_COMPILE_INFO: c_uint = 0;
const STRUCT_HLSL_SOURCE_INFO: c_uint = 6;
const STRUCT_PREPROCESS_INFO: c_uint = 7;
const SOURCE_HLSL: c_uint = 2;
const TARGET_D3D_BYTECODE: c_uint = 4;
const LOG_WARNING: c_uint = 2;
const OPTION_API_VERSION: c_uint = 4;
const OPTION_PACK_MATRIX_ORDER: c_uint = 7;
const OPTION_BACKWARD_COMPATIBILITY: c_uint = 8;
const PACK_ROW_MAJOR: c_uint = 1;
const PACK_COLUMN_MAJOR: c_uint = 2;
const BACKCOMPAT_MAP_SEMANTIC_NAMES: c_uint = 1;
const ERROR: c_int = -1;
/// The newest `VKD3D_SHADER_API_VERSION_1_x` this binding knows (1.19); a library that reports another major (the
/// package 2.1 ships API 1.19 under soname 1) is asked for it.
const LATEST_API: c_uint = 19;
/// `D3DXSHADER_PACKMATRIX_ROWMAJOR` and `..._COLUMNMAJOR`.
const D3DX_PACK_ROW: u32 = 1 << 4;
const D3DX_PACK_COLUMN: u32 = 1 << 5;

#[repr(C)]
struct Code {
    code: *const c_void,
    size: usize,
}

#[repr(C)]
struct CompileOption {
    name: c_uint,
    value: c_uint,
}

#[repr(C)]
struct CompileInfo {
    kind: c_uint,
    next: *const c_void,
    source: Code,
    source_type: c_uint,
    target_type: c_uint,
    options: *const CompileOption,
    option_count: c_uint,
    log_level: c_uint,
    source_name: *const c_char,
}

#[repr(C)]
struct HlslSourceInfo {
    kind: c_uint,
    next: *const c_void,
    entry_point: *const c_char,
    secondary_code: Code,
    profile: *const c_char,
}

#[repr(C)]
struct ShaderMacro {
    name: *const c_char,
    value: *const c_char,
}

type OpenInclude =
    extern "C" fn(*const c_char, bool, *const c_char, *mut c_void, *mut Code) -> c_int;
type CloseInclude = extern "C" fn(*const Code, *mut c_void);

#[repr(C)]
struct PreprocessInfo {
    kind: c_uint,
    next: *const c_void,
    macros: *const ShaderMacro,
    macro_count: c_uint,
    open_include: Option<OpenInclude>,
    close_include: Option<CloseInclude>,
    include_context: *mut c_void,
}

type Compile = unsafe extern "C" fn(*const CompileInfo, *mut Code, *mut *mut c_char) -> c_int;
type FreeCode = unsafe extern "C" fn(*mut Code);
type FreeMessages = unsafe extern "C" fn(*mut c_char);
type GetVersion = unsafe extern "C" fn(*mut c_uint, *mut c_uint) -> *const c_char;

/// What the include callbacks share while one compile runs.
struct IncludeState<'a> {
    includes: &'a mut dyn Includes,
    read: Vec<Include>,
}

extern "C" fn open_include(
    name: *const c_char,
    local: bool,
    _parent: *const c_char,
    context: *mut c_void,
    out: *mut Code,
) -> c_int {
    // SAFETY: the context is the `IncludeState` of the compile call that is running, and the
    // library passes a NUL-terminated name and a valid `out`.
    let (state, name) = unsafe {
        (
            &mut *(context as *mut IncludeState<'_>),
            CStr::from_ptr(name).to_bytes(),
        )
    };
    let Some(content) = state.includes.open(name, local) else {
        return ERROR;
    };
    let bytes: Box<[u8]> = content.clone().into_boxed_slice();
    let size = bytes.len();
    let code = Box::into_raw(bytes) as *const u8 as *const c_void;
    state.read.push(Include {
        name: name.to_vec(),
        local,
        content,
    });
    // SAFETY: `out` is valid (above); `close_include` gives the allocation back.
    unsafe { *out = Code { code, size } };
    0
}

extern "C" fn close_include(code: *const Code, _context: *mut c_void) {
    // SAFETY: `code` is what `open_include` made: a boxed slice of this pointer and length.
    unsafe {
        let code = &*code;
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
            code.code as *mut u8,
            code.size,
        )));
    }
}

/// A loaded libvkd3d-shader.
pub struct Vkd3d {
    compile: Compile,
    free_code: FreeCode,
    free_messages: FreeMessages,
    version: String,
    api_minor: c_uint,
    // Keeps the function pointers valid.
    _library: Library,
}

fn unavailable(detail: &str) -> CompileError {
    CompileError::Unavailable {
        reason: format!(
            "libvkd3d-shader is not available ({detail}): the Linux shader compiler needs it at run time. \
             Install vkd3d (Arch: vkd3d, Debian and Ubuntu: libvkd3d-shader1, Fedora: vkd3d, Nix: vkd3d) or \
             set NV_VKD3D_SHADER to the library's path, or record the shaders into the cache (NV_D3DX_CACHE)"
        ),
    }
}

impl Vkd3d {
    /// Loads the library: the file `NV_VKD3D_SHADER` names, else `libvkd3d-shader.so.1`, else
    /// `libvkd3d-shader.so`.
    pub fn load() -> Result<Self, CompileError> {
        let wanted = std::env::var("NV_VKD3D_SHADER")
            .ok()
            .filter(|v| !v.is_empty());
        let names: Vec<String> = match wanted {
            Some(path) => vec![path],
            None => vec!["libvkd3d-shader.so.1".into(), "libvkd3d-shader.so".into()],
        };
        let mut last = String::from("no candidate");
        for name in &names {
            // SAFETY: loading a shared library runs its initialisers; libvkd3d-shader's are inert.
            match unsafe { Library::new(name) } {
                Ok(library) => return Self::bind(library),
                Err(e) => last = e.to_string(),
            }
        }
        Err(unavailable(&last))
    }

    fn bind(library: Library) -> Result<Self, CompileError> {
        macro_rules! symbol {
            ($name:literal, $ty:ty) => {
                // SAFETY: the type is the C prototype of the symbol in vkd3d_shader.h.
                *unsafe { library.get::<$ty>(concat!($name, "\0").as_bytes()) }
                    .map_err(|e| unavailable(&e.to_string()))?
            };
        }
        let compile = symbol!("vkd3d_shader_compile", Compile);
        let free_code = symbol!("vkd3d_shader_free_shader_code", FreeCode);
        let free_messages = symbol!("vkd3d_shader_free_messages", FreeMessages);
        let get_version = symbol!("vkd3d_shader_get_version", GetVersion);
        let (mut major, mut minor) = (0, 0);
        // SAFETY: both out pointers are valid; the answer is a static NUL-terminated string.
        let text = unsafe { CStr::from_ptr(get_version(&mut major, &mut minor)) }
            .to_string_lossy()
            .into_owned();
        let version = if text.is_empty() {
            format!("vkd3d-shader {major}.{minor}")
        } else {
            text
        };
        let loaded = Vkd3d {
            compile,
            free_code,
            free_messages,
            version,
            api_minor: if major == 1 { minor } else { LATEST_API },
            _library: library,
        };
        // A library from before HLSL could be compiled to d3dbc (Ubuntu 24.04's 1.10, for one) loads but
        // cannot do the job: said here, once, instead of as a compile error on every shader.
        let probe = Request {
            kind: Kind::Compile,
            source: b"float4 main() : COLOR { return 1; }",
            entry: Some(b"main"),
            target: Some(b"ps_2_0"),
            flags: 0,
            macros: &[],
        };
        if loaded.compile(&probe, &mut crate::NoIncludes).is_err() {
            return Err(unavailable(&format!(
                "{} loads but cannot compile HLSL to d3dbc: it is too old",
                loaded.version
            )));
        }
        Ok(loaded)
    }

    /// The library's version string.
    pub fn version(&self) -> &str {
        &self.version
    }
}

/// The backend as the engine holds it: libvkd3d-shader loaded at the first compile (a machine that
/// never misses the cache never loads it), the failure to load kept and answered each time.
#[derive(Default)]
pub struct LazyVkd3d(std::cell::OnceCell<Result<Vkd3d, CompileError>>);

impl LazyVkd3d {
    pub fn new() -> Self {
        Self::default()
    }

    fn get(&self) -> Result<&Vkd3d, CompileError> {
        self.0
            .get_or_init(Vkd3d::load)
            .as_ref()
            .map_err(Clone::clone)
    }
}

impl ShaderCompiler for LazyVkd3d {
    fn identity(&self) -> String {
        self.get()
            .map_or_else(|_| "vkd3d-shader-not-loaded".into(), |v| v.identity())
    }

    fn compile(
        &self,
        r: &Request<'_>,
        includes: &mut dyn Includes,
    ) -> Result<Output, CompileError> {
        self.get()?.compile(r, includes)
    }
}

fn c_string(bytes: &[u8]) -> Result<CString, CompileError> {
    CString::new(bytes).map_err(|_| CompileError::Failed {
        messages: b"a NUL byte in a name or macro".to_vec(),
    })
}

impl ShaderCompiler for Vkd3d {
    fn identity(&self) -> String {
        self.version.clone()
    }

    fn compile(
        &self,
        r: &Request<'_>,
        includes: &mut dyn Includes,
    ) -> Result<Output, CompileError> {
        if r.kind == Kind::Assemble {
            return Err(CompileError::Unsupported {
                reason: "libvkd3d-shader has no assembler for d3d assembly text: record this shader into the shader cache (NV_D3DX_CACHE) \
                         on a machine that has d3dx9"
                    .into(),
            });
        }
        let entry = c_string(r.entry.unwrap_or(b"main"))?;
        let profile = c_string(r.target.unwrap_or(b""))?;
        let names: Vec<(CString, CString)> = r
            .macros
            .iter()
            .map(|m| Ok((c_string(&m.name)?, c_string(&m.value)?)))
            .collect::<Result<_, CompileError>>()?;
        let macros: Vec<ShaderMacro> = names
            .iter()
            .map(|(n, v)| ShaderMacro {
                name: n.as_ptr(),
                value: v.as_ptr(),
            })
            .collect();
        let mut state = IncludeState {
            includes,
            read: Vec::new(),
        };
        let preprocess = PreprocessInfo {
            kind: STRUCT_PREPROCESS_INFO,
            next: std::ptr::null(),
            macros: macros.as_ptr(),
            macro_count: macros.len() as c_uint,
            open_include: Some(open_include),
            close_include: Some(close_include),
            include_context: &mut state as *mut IncludeState<'_> as *mut c_void,
        };
        let hlsl = HlslSourceInfo {
            kind: STRUCT_HLSL_SOURCE_INFO,
            next: &preprocess as *const PreprocessInfo as *const c_void,
            entry_point: entry.as_ptr(),
            secondary_code: Code {
                code: std::ptr::null(),
                size: 0,
            },
            profile: profile.as_ptr(),
        };
        let mut options = vec![
            CompileOption {
                name: OPTION_API_VERSION,
                value: self.api_minor,
            },
            CompileOption {
                name: OPTION_BACKWARD_COMPATIBILITY,
                value: BACKCOMPAT_MAP_SEMANTIC_NAMES,
            },
        ];
        if r.flags & D3DX_PACK_ROW != 0 {
            options.push(CompileOption {
                name: OPTION_PACK_MATRIX_ORDER,
                value: PACK_ROW_MAJOR,
            });
        } else if r.flags & D3DX_PACK_COLUMN != 0 {
            options.push(CompileOption {
                name: OPTION_PACK_MATRIX_ORDER,
                value: PACK_COLUMN_MAJOR,
            });
        }
        let info = CompileInfo {
            kind: STRUCT_COMPILE_INFO,
            next: &hlsl as *const HlslSourceInfo as *const c_void,
            source: Code {
                code: r.source.as_ptr() as *const c_void,
                size: r.source.len(),
            },
            source_type: SOURCE_HLSL,
            target_type: TARGET_D3D_BYTECODE,
            options: options.as_ptr(),
            option_count: options.len() as c_uint,
            log_level: LOG_WARNING,
            source_name: std::ptr::null(),
        };
        let mut out = Code {
            code: std::ptr::null(),
            size: 0,
        };
        let mut messages: *mut c_char = std::ptr::null_mut();
        // SAFETY: `info` and everything it chains (strings, macros, the include state) outlive
        // the call; `out` and `messages` are valid out pointers.
        let status = unsafe { (self.compile)(&info, &mut out, &mut messages) };
        let text = if messages.is_null() {
            Vec::new()
        } else {
            // SAFETY: a NUL-terminated string owned by the library until freed.
            let t = unsafe { CStr::from_ptr(messages) }.to_bytes().to_vec();
            unsafe { (self.free_messages)(messages) };
            t
        };
        if status < 0 {
            return Err(CompileError::Failed { messages: text });
        }
        // SAFETY: on success `out` holds `size` bytes, owned by the library until freed.
        let bytecode =
            unsafe { std::slice::from_raw_parts(out.code as *const u8, out.size) }.to_vec();
        unsafe { (self.free_code)(&mut out) };
        Ok(Output {
            bytecode,
            includes: state.read,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{request_key, NoIncludes};

    /// The library, or `None` after saying so: the tests need `libvkd3d-shader` (CI installs it;
    /// a machine without it prints the skip). `NV_REQUIRE_VKD3D=1` turns the skip into a failure.
    fn library(test: &str) -> Option<Vkd3d> {
        match Vkd3d::load() {
            Ok(v) => {
                println!("executed: {test} ({})", v.version());
                Some(v)
            }
            Err(e) => {
                assert!(
                    !matches!(std::env::var("NV_REQUIRE_VKD3D").as_deref(), Ok("1")),
                    "{test}: {e}"
                );
                println!("skipped: {test} (libvkd3d-shader not available)");
                None
            }
        }
    }

    fn compile(v: &Vkd3d, source: &str, entry: &str, target: &str) -> Result<Output, CompileError> {
        let r = Request {
            kind: Kind::Compile,
            source: source.as_bytes(),
            entry: Some(entry.as_bytes()),
            target: Some(target.as_bytes()),
            flags: 0,
            macros: &[],
        };
        v.compile(&r, &mut NoIncludes)
    }

    fn words(bytes: &[u8]) -> Vec<u32> {
        bytes
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    }

    const PS: &str = "float4 c : register(c0);\nsampler2D s : register(s0);\n\
        float4 main(float2 uv : TEXCOORD0) : COLOR { return tex2D(s, uv) * c; }\n";
    const VS: &str = "float4x4 m : register(c0);\n\
        float4 main(float4 p : POSITION) : POSITION { return mul(p, m); }\n";

    #[test]
    fn compiles_the_shader_models() {
        let Some(v) = library("compiles_the_shader_models") else {
            return;
        };
        for (source, target, version) in [
            (PS, "ps_2_0", 0xffff_0200u32),
            (PS, "ps_3_0", 0xffff_0300),
            (VS, "vs_2_0", 0xfffe_0200),
            (VS, "vs_3_0", 0xfffe_0300),
        ] {
            let out =
                compile(&v, source, "main", target).unwrap_or_else(|e| panic!("{target}: {e}"));
            let w = words(&out.bytecode);
            assert_eq!(w[0], version, "{target}: version token");
            assert_eq!(*w.last().unwrap(), 0x0000_ffff, "{target}: end token");
            assert!(w.contains(&0x4241_5443), "{target}: no CTAB comment");
        }
    }

    #[test]
    fn errors_come_back_as_messages() {
        let Some(v) = library("errors_come_back_as_messages") else {
            return;
        };
        let err = compile(
            &v,
            "float4 main() : COLOR { return undeclared; }",
            "main",
            "ps_2_0",
        )
        .unwrap_err();
        let CompileError::Failed { messages } = err else {
            panic!("{err:?}")
        };
        assert!(!messages.is_empty());
    }

    #[test]
    fn macros_and_includes_reach_the_compiler() {
        let Some(v) = library("macros_and_includes_reach_the_compiler") else {
            return;
        };
        let source = "#include \"body.h\"\nfloat4 main() : COLOR { return VALUE; }\n";
        let macros = [crate::Macro {
            name: b"VALUE".to_vec(),
            value: b"float4(1,0,0,1)".to_vec(),
        }];
        let r = Request {
            kind: Kind::Compile,
            source: source.as_bytes(),
            entry: Some(b"main"),
            target: Some(b"ps_2_0"),
            flags: 0,
            macros: &macros,
        };
        let mut inc = |name: &[u8], local: bool| {
            assert_eq!(name, b"body.h");
            assert!(local);
            Some(b"// header\n".to_vec())
        };
        let out = v.compile(&r, &mut inc).unwrap();
        assert_eq!(
            out.includes,
            [Include {
                name: b"body.h".to_vec(),
                local: true,
                content: b"// header\n".to_vec()
            }]
        );
        assert!(!out.bytecode.is_empty());
        let missing = v.compile(&r, &mut NoIncludes).unwrap_err();
        assert!(matches!(missing, CompileError::Failed { .. }));
    }

    #[test]
    fn assembly_is_unsupported_not_wrong() {
        let Some(v) = library("assembly_is_unsupported_not_wrong") else {
            return;
        };
        let r = Request {
            kind: Kind::Assemble,
            source: b"ps_2_0\nmov oC0, c0\n",
            entry: None,
            target: None,
            flags: 0,
            macros: &[],
        };
        assert!(matches!(
            v.compile(&r, &mut NoIncludes),
            Err(CompileError::Unsupported { .. })
        ));
    }

    #[test]
    fn compiled_answer_goes_through_the_cache() {
        let Some(v) = library("compiled_answer_goes_through_the_cache") else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("d3dx-shader-vkd3d-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let identity = v.identity();
        let compiler = crate::CacheCompiler::new(crate::Cache::new(&dir), Some(Box::new(v)));
        let r = Request {
            kind: Kind::Compile,
            source: PS.as_bytes(),
            entry: Some(b"main"),
            target: Some(b"ps_2_0"),
            flags: 0,
            macros: &[],
        };
        let first = compiler.compile(&r, &mut NoIncludes).unwrap();
        let (dir_name, stored) = compiler.cache().lookup(&r, &mut NoIncludes).unwrap();
        assert_eq!(dir_name, crate::identity_dir(&identity));
        assert_eq!(stored, first.bytecode);
        assert_eq!(request_key(&r).len(), 32);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn absent_library_is_a_clear_error() {
        let e = unavailable("test");
        let text = e.to_string();
        assert!(
            text.contains("libvkd3d-shader")
                && text.contains("NV_VKD3D_SHADER")
                && text.contains("Install")
        );
    }
}
