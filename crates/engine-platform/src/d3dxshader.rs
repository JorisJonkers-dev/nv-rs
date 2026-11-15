//! The shader compiler imports off Windows (nv-linux#178, #197, #200). Fork-owned.
//!
//! On Windows the six imports (`D3DXCompileShader`, `D3DXGetShaderConstantTable`,
//! `D3DXGetVertexShaderProfile`, `D3DXGetPixelShaderProfile`, `D3DXCreateBuffer`,
//! `D3DXAssembleShader`) go to the player's own `d3dx9_38.dll` (`d3dxreal`, unchanged). Here they
//! are answered by the seam of the `d3dx-shader` crate: the compile cache first, then
//! libvkd3d-shader loaded at run time. A request neither can answer stops the run with the named
//! error of the seam (no source text in it); a compile the compiler refuses answers
//! `D3DXERR_INVALIDDATA` and an error buffer, as D3DX does.
//!
//! The objects handed to the game are `ID3DXBuffer` and `ID3DXConstantTable`, kept entirely in
//! engine memory (`[vtable, references, data, size]`). The table's two buffer slots answer (its
//! bytes are the shader's `CTAB`); its reading slots (`GetDesc`, `GetConstantDesc`, ...) stop the
//! run with a named message until the pure-Rust table over `CTAB` of the follow-up is in.

use super::d3d9::{CODE_BASE, D3DERR_INVALIDCALL};
use crate::{AbiFn, Engine, Ptr};
use d3dx_shader::{
    cache, vkd3d::LazyVkd3d, Cache, CacheCompiler, CompileError, Kind, Macro, Request,
    ShaderCompiler,
};
use std::cell::OnceCell;

const BUFFER_CODE: u32 = CODE_BASE + 0x0a00;
const TABLE_CODE: u32 = CODE_BASE + 0x0a40;
const TABLE_SLOTS: u32 = 20;
const D3DXERR_INVALIDDATA: u32 = 0x8876_0b59;
const E_NOINTERFACE: u32 = 0x8000_4002;

thread_local! {
    static SEAM: OnceCell<CacheCompiler> = const { OnceCell::new() };
}

/// The compiler of this thread: the cache folder of the environment, and libvkd3d-shader behind it
/// unless `NV_D3DX_SHADER_BACKEND=off` (cache only).
fn with_seam<R>(f: impl FnOnce(&CacheCompiler) -> R) -> R {
    SEAM.with(|cell| {
        f(cell.get_or_init(|| {
            let root = cache::default_root()
                .unwrap_or_else(|| std::env::temp_dir().join("nv-rs-d3dx-shaders"));
            let off = std::env::var("NV_D3DX_SHADER_BACKEND").is_ok_and(|v| v == "off");
            let backend: Option<Box<dyn ShaderCompiler>> =
                (!off).then(|| Box::new(LazyVkd3d::new()) as Box<dyn ShaderCompiler>);
            CacheCompiler::new(Cache::new(root), backend)
        }))
    })
}

/// The six imports, as the Windows table names them.
pub fn imports() -> Vec<(&'static str, &'static str, AbiFn)> {
    let d = "d3dx9_38.dll";
    vec![
        (d, "D3DXCompileShader", crate::entry!(0, compile_shader(Ptr, u32, Ptr, Ptr, Ptr, Ptr, u32, Ptr, Ptr, Ptr) -> u32).1),
        (d, "D3DXGetShaderConstantTable", crate::entry!(0, get_shader_constant_table(Ptr, Ptr) -> u32).1),
        (d, "D3DXGetVertexShaderProfile", crate::entry!(0, get_vertex_shader_profile(Ptr) -> u32).1),
        (d, "D3DXGetPixelShaderProfile", crate::entry!(0, get_pixel_shader_profile(Ptr) -> u32).1),
        (d, "D3DXCreateBuffer", crate::entry!(0, create_buffer(u32, Ptr) -> u32).1),
        (d, "D3DXAssembleShader", crate::entry!(0, assemble_shader(Ptr, u32, Ptr, Ptr, u32, Ptr, Ptr) -> u32).1),
    ]
}

fn register(e: &mut Engine) {
    let buffer: [AbiFn; 5] = [
        crate::entry!(0, no_interface(Ptr, Ptr, Ptr) -> u32).1,
        crate::entry!(0, add_ref(Ptr) -> u32).1,
        crate::entry!(0, release(Ptr) -> u32).1,
        crate::entry!(0, pointer(Ptr) -> u32).1,
        crate::entry!(0, size(Ptr) -> u32).1,
    ];
    for (i, f) in buffer.into_iter().enumerate() {
        e.register(BUFFER_CODE + 4 * i as u32, f);
        // The table shares the buffer slots 0 to 4.
        e.register(TABLE_CODE + 4 * i as u32, f);
    }
    for i in 5..=10 {
        e.register(
            TABLE_CODE + 4 * i,
            crate::entry!(0, table_not_provided(Ptr) -> u32).1,
        );
    }
}

/// An object `[vtable, references, data, size]` of `bytes`, with `code` as its slot table.
fn new_object(e: &mut Engine, code: u32, slots: u32, bytes: &[u8]) -> u32 {
    register(e);
    let table = e.mem.alloc(4 * slots);
    for i in 0..slots {
        e.mem.set_u32(table + 4 * i, code + 4 * i);
    }
    let object = e.mem.alloc(16);
    let data = e.mem.alloc(bytes.len().max(1) as u32);
    e.mem.write(data, bytes);
    for (i, v) in [table, 1, data, bytes.len() as u32].into_iter().enumerate() {
        e.mem.set_u32(object + 4 * i as u32, v);
    }
    object
}

fn new_buffer(e: &mut Engine, bytes: &[u8]) -> u32 {
    new_object(e, BUFFER_CODE, 5, bytes)
}

fn new_table(e: &mut Engine, ctab: &[u8]) -> u32 {
    new_object(e, TABLE_CODE, TABLE_SLOTS, ctab)
}

fn no_interface(e: &mut Engine, _this: Ptr, _iid: Ptr, out: Ptr) -> u32 {
    if !out.is_null() {
        e.mem.set_u32(out.addr(), 0);
    }
    E_NOINTERFACE
}

fn add_ref(e: &mut Engine, this: Ptr) -> u32 {
    let n = e.mem.u32(this.addr() + 4) + 1;
    e.mem.set_u32(this.addr() + 4, n);
    n
}

fn release(e: &mut Engine, this: Ptr) -> u32 {
    let n = e.mem.u32(this.addr() + 4) - 1;
    e.mem.set_u32(this.addr() + 4, n);
    if n == 0 {
        let (table, data) = (e.mem.u32(this.addr()), e.mem.u32(this.addr() + 8));
        e.mem.free(data);
        e.mem.free(table);
        e.mem.free(this.addr());
    }
    n
}

fn pointer(e: &mut Engine, this: Ptr) -> u32 {
    e.mem.u32(this.addr() + 8)
}

fn size(e: &mut Engine, this: Ptr) -> u32 {
    e.mem.u32(this.addr() + 12)
}

fn table_not_provided(_e: &mut Engine, _this: Ptr) -> u32 {
    panic!(
        "ID3DXConstantTable's reading slots (GetDesc, GetConstantDesc, GetConstant, ...) are not provided off Windows yet \
         (nv-linux#178: the pure-Rust constant table over the shader's CTAB)"
    )
}

/// The `D3DXMACRO` pairs at `defines` (up to a null name).
fn read_macros(e: &Engine, defines: Ptr) -> Vec<Macro> {
    let mut macros = Vec::new();
    if defines.is_null() {
        return macros;
    }
    let text = |at: u32| if at == 0 { Vec::new() } else { e.mem.cstr(at) };
    let mut at = defines.addr();
    loop {
        let (name, value) = (e.mem.u32(at), e.mem.u32(at + 4));
        if name == 0 {
            return macros;
        }
        macros.push(Macro {
            name: text(name),
            value: text(value),
        });
        at += 8;
    }
}

/// The game's `ID3DXInclude::Open` (slot 0) then `Close` (slot 1): the contents copied out.
fn game_include(e: &mut Engine, include: u32, name: &[u8], local: bool) -> Option<Vec<u8>> {
    if include == 0 {
        return None;
    }
    let frame = e.mem.alloc(name.len() as u32 + 1 + 8);
    e.mem.write(frame, name);
    e.mem.set_u8(frame + name.len() as u32, 0);
    let out = frame + name.len() as u32 + 1;
    e.mem.set_u32(out, 0);
    e.mem.set_u32(out + 4, 0);
    // D3DXINC_LOCAL is 0, D3DXINC_SYSTEM 1; the parent data is not tracked.
    let hr = e
        .vcall(include, 0, &[u32::from(!local), frame, 0, out, out + 4])
        .u32() as i32;
    let (at, len) = (e.mem.u32(out), e.mem.u32(out + 4));
    e.mem.free(frame);
    if hr < 0 {
        return None;
    }
    let content = e.mem.bytes(at, len);
    e.vcall(include, 4, &[at]);
    Some(content)
}

/// Runs one request and writes the answers: the shader and error buffers (and the table when
/// `constants` is given). Returns the HRESULT.
#[allow(clippy::too_many_arguments)]
fn run(
    e: &mut Engine,
    request: &Request<'_>,
    include: Ptr,
    shader: Ptr,
    errors: Ptr,
    constants: Ptr,
) -> u32 {
    let game = include.addr();
    let result = with_seam(|seam| {
        seam.compile(request, &mut |name: &[u8], local| {
            game_include(e, game, name, local)
        })
    });
    for out in [shader, errors, constants] {
        if !out.is_null() {
            e.mem.set_u32(out.addr(), 0);
        }
    }
    match result {
        Ok(made) => {
            if !shader.is_null() {
                let object = new_buffer(e, &made.bytecode);
                e.mem.set_u32(shader.addr(), object);
            }
            if !constants.is_null() {
                if let Some(ctab) = d3dx_shader::constant_table(&made.bytecode) {
                    let object = new_table(e, ctab);
                    e.mem.set_u32(constants.addr(), object);
                }
            }
            0
        }
        Err(CompileError::Failed { mut messages }) => {
            if !errors.is_null() {
                messages.push(0);
                let object = new_buffer(e, &messages);
                e.mem.set_u32(errors.addr(), object);
            }
            D3DXERR_INVALIDDATA
        }
        // A miss, a missing library, an assembler that does not exist: the run cannot go on with a
        // wrong shader, so it stops, naming what is missing.
        Err(other) => panic!("{other}"),
    }
}

fn optional_text(e: &Engine, at: Ptr) -> Option<Vec<u8>> {
    (!at.is_null()).then(|| e.mem.cstr(at.addr()))
}

// Platform: replaces d3dx9_38.dll D3DXCompileShader (import, thunk 00ee6e0a)
/// `D3DXCompileShader(source, length, defines, include, function, profile, flags, shader out,
/// errors out, constant table out)`.
#[allow(clippy::too_many_arguments)]
pub fn compile_shader(
    e: &mut Engine,
    source: Ptr,
    length: u32,
    defines: Ptr,
    include: Ptr,
    function: Ptr,
    profile: Ptr,
    flags: u32,
    shader: Ptr,
    errors: Ptr,
    constants: Ptr,
) -> u32 {
    let text = e.mem.bytes(source.addr(), length);
    let (entry, target) = (optional_text(e, function), optional_text(e, profile));
    let macros = read_macros(e, defines);
    let request = Request {
        kind: Kind::Compile,
        source: &text,
        entry: entry.as_deref(),
        target: target.as_deref(),
        flags,
        macros: &macros,
    };
    run(e, &request, include, shader, errors, constants)
}

// Platform: replaces d3dx9_38.dll D3DXAssembleShader (import)
/// `D3DXAssembleShader(source, length, defines, include, flags, shader out, errors out)`.
#[allow(clippy::too_many_arguments)]
pub fn assemble_shader(
    e: &mut Engine,
    source: Ptr,
    length: u32,
    defines: Ptr,
    include: Ptr,
    flags: u32,
    shader: Ptr,
    errors: Ptr,
) -> u32 {
    let text = e.mem.bytes(source.addr(), length);
    let macros = read_macros(e, defines);
    let request = Request {
        kind: Kind::Assemble,
        source: &text,
        entry: None,
        target: None,
        flags,
        macros: &macros,
    };
    run(e, &request, include, shader, errors, Ptr::NULL)
}

// Platform: replaces d3dx9_38.dll D3DXGetShaderConstantTable (import, thunk 00ee6e40)
/// `D3DXGetShaderConstantTable(function, table out)`: the table of the bytecode's `CTAB`.
pub fn get_shader_constant_table(e: &mut Engine, function: Ptr, out: Ptr) -> u32 {
    if function.is_null() || out.is_null() {
        return D3DERR_INVALIDCALL;
    }
    // The stream's length is not known to the caller: read it word by word up to the end token.
    let mut bytes = Vec::new();
    let mut at = function.addr();
    loop {
        let token = e.mem.u32(at);
        bytes.extend_from_slice(&token.to_le_bytes());
        at += 4;
        if token == 0x0000_ffff && bytes.len() > 4 {
            break;
        }
        if token & 0xffff == 0xfffe && bytes.len() > 4 {
            let len = (token >> 16) & 0x7fff;
            bytes.extend(e.mem.bytes(at, 4 * len));
            at += 4 * len;
        }
    }
    let object = match d3dx_shader::constant_table(&bytes) {
        Some(ctab) => new_table(e, ctab),
        None => {
            e.mem.set_u32(out.addr(), 0);
            return D3DXERR_INVALIDDATA;
        }
    };
    e.mem.set_u32(out.addr(), object);
    0
}

/// The profile string in engine memory. The modelled device is a Shader Model 3 one (d3d9-wgpu's
/// converter reads Shader Models 1 to 3 and the device model answers 3.0 caps), so both answers are
/// the 3.0 profiles, as the DLL gives for such caps.
fn profile(e: &mut Engine, text: &str) -> u32 {
    let at = e.mem.alloc(text.len() as u32 + 1);
    e.mem.write(at, text.as_bytes());
    e.mem.set_u8(at + text.len() as u32, 0);
    at
}

// Platform: replaces d3dx9_38.dll D3DXGetVertexShaderProfile (import)
pub fn get_vertex_shader_profile(e: &mut Engine, _device: Ptr) -> u32 {
    profile(e, "vs_3_0")
}

// Platform: replaces d3dx9_38.dll D3DXGetPixelShaderProfile (import, thunk 00ee6e28)
pub fn get_pixel_shader_profile(e: &mut Engine, _device: Ptr) -> u32 {
    profile(e, "ps_3_0")
}

// Platform: replaces d3dx9_38.dll D3DXCreateBuffer (import, thunk 00ee6e3a)
/// `D3DXCreateBuffer(size, out)`: a zeroed buffer; a null out answers `D3DERR_INVALIDCALL`.
pub fn create_buffer(e: &mut Engine, size: u32, out: Ptr) -> u32 {
    if out.is_null() {
        return D3DERR_INVALIDCALL;
    }
    let object = new_buffer(e, &vec![0u8; size as usize]);
    e.mem.set_u32(out.addr(), object);
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIX: [&str; 6] = [
        "D3DXCompileShader",
        "D3DXGetShaderConstantTable",
        "D3DXGetVertexShaderProfile",
        "D3DXGetPixelShaderProfile",
        "D3DXCreateBuffer",
        "D3DXAssembleShader",
    ];

    #[test]
    fn the_six_imports_are_in_the_table_off_windows() {
        let table = crate::imports();
        let found: Vec<&str> = table
            .iter()
            .filter(|(d, n, _)| *d == "d3dx9_38.dll" && SIX.contains(n))
            .map(|(_, n, _)| *n)
            .collect();
        for name in SIX {
            assert_eq!(found.iter().filter(|n| **n == name).count(), 1, "{name}");
        }
        assert_eq!(
            imports().iter().map(|(_, n, _)| *n).collect::<Vec<_>>(),
            SIX
        );
    }

    #[test]
    fn create_buffer_makes_a_zeroed_buffer_and_releases_it() {
        let mut e = Engine::new();
        let out = e.mem.alloc(4);
        let table = imports();
        for (i, (_, _, f)) in table.iter().enumerate() {
            e.register(0x7f00_0000 + 4 * i as u32, *f);
        }
        let create = 0x7f00_0000 + 4 * 4;
        assert_eq!(e.call(create, &[16, out]).u32(), 0);
        let object = e.mem.u32(out);
        let vtable = e.mem.u32(object);
        assert_eq!(e.vcall(object, 12, &[]).u32(), e.mem.u32(object + 8));
        assert_eq!(e.vcall(object, 16, &[]).u32(), 16);
        assert_eq!(e.mem.bytes(e.mem.u32(object + 8), 16), vec![0u8; 16]);
        assert_eq!(e.mem.u32(vtable + 12), BUFFER_CODE + 12);
        assert_eq!(e.vcall(object, 4, &[]).u32(), 2);
        assert_eq!(e.vcall(object, 8, &[]).u32(), 1);
        assert_eq!(e.vcall(object, 8, &[]).u32(), 0);
        assert_eq!(e.call(create, &[16, 0]).u32(), D3DERR_INVALIDCALL);
    }

    /// A scratch folder as the seam's cache, with libvkd3d-shader behind it; `None` (a printed skip)
    /// where the library is not installed (`NV_REQUIRE_VKD3D=1` fails instead).
    fn engine_with_backend(test: &str) -> Option<(Engine, std::path::PathBuf)> {
        if let Err(e) = d3dx_shader::vkd3d::Vkd3d::load() {
            assert!(
                !matches!(std::env::var("NV_REQUIRE_VKD3D").as_deref(), Ok("1")),
                "{test}: {e}"
            );
            println!("skipped: {test} (libvkd3d-shader not available)");
            return None;
        }
        println!("executed: {test}");
        let dir = std::env::temp_dir().join(format!("nv-d3dxshader-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        SEAM.with(|c| {
            let _ = c.set(CacheCompiler::new(
                Cache::new(&dir),
                Some(Box::new(LazyVkd3d::new())),
            ));
        });
        let mut e = Engine::new();
        for (i, (_, _, f)) in imports().iter().enumerate() {
            e.register(0x7f00_0000 + 4 * i as u32, *f);
        }
        Some((e, dir))
    }

    fn put(e: &mut Engine, text: &str) -> u32 {
        let at = e.mem.alloc(text.len() as u32 + 1);
        e.mem.write(at, text.as_bytes());
        e.mem.set_u8(at + text.len() as u32, 0);
        at
    }

    #[test]
    fn compile_shader_answers_buffers_and_a_table() {
        let Some((mut e, dir)) = engine_with_backend("compile_shader_answers_buffers_and_a_table")
        else {
            return;
        };
        let source = "float4 c : register(c0);\nfloat4 main(float2 uv : TEXCOORD0) : COLOR { return c * uv.x; }\n";
        let (text, entry, target) = (
            put(&mut e, source),
            put(&mut e, "main"),
            put(&mut e, "ps_2_0"),
        );
        let out = e.mem.alloc(12);
        let hr = e
            .call(
                0x7f00_0000,
                &[
                    text,
                    source.len() as u32,
                    0,
                    0,
                    entry,
                    target,
                    0,
                    out,
                    out + 4,
                    out + 8,
                ],
            )
            .u32();
        assert_eq!(hr, 0);
        let (shader, errors, table) = (e.mem.u32(out), e.mem.u32(out + 4), e.mem.u32(out + 8));
        assert_eq!(errors, 0);
        let (data, len) = (
            e.vcall(shader, 12, &[]).u32(),
            e.vcall(shader, 16, &[]).u32(),
        );
        assert_eq!(e.mem.u32(data), 0xffff_0200);
        assert_eq!(e.mem.u32(data + len - 4), 0x0000_ffff);
        assert_ne!(table, 0, "the compile made a constant table");
        let (ctab, ctab_len) = (e.vcall(table, 12, &[]).u32(), e.vcall(table, 16, &[]).u32());
        assert!(
            ctab_len >= 28 && e.mem.u32(ctab) == 28,
            "CTAB starts with its size field"
        );
        // The same bytecode's table through D3DXGetShaderConstantTable.
        let again = e.mem.alloc(4);
        assert_eq!(e.call(0x7f00_0000 + 4, &[data, again]).u32(), 0);
        let second = e.mem.u32(again);
        assert_eq!(e.vcall(second, 16, &[]).u32(), ctab_len);
        // A refused source: the HRESULT and the error buffer, no shader.
        let bad = "float4 main() : COLOR { return undeclared; }";
        let text = put(&mut e, bad);
        let hr = e
            .call(
                0x7f00_0000,
                &[
                    text,
                    bad.len() as u32,
                    0,
                    0,
                    entry,
                    target,
                    0,
                    out,
                    out + 4,
                    out + 8,
                ],
            )
            .u32();
        assert_eq!(hr, D3DXERR_INVALIDDATA);
        assert_eq!((e.mem.u32(out), e.mem.u32(out + 8)), (0, 0));
        let messages = e.mem.u32(out + 4);
        assert_ne!(messages, 0);
        assert!(e.vcall(messages, 16, &[]).u32() > 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}
