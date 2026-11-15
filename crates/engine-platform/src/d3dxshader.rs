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
//! bytes are the shader's `CTAB`); its reading slots (`GetDesc`, `GetConstantDesc`,
//! `GetSamplerIndex`, `GetConstant`, `GetConstantByName`, `GetConstantElement`) are answered from
//! `d3dx_shader::ctab` (pure Rust, nv-linux#198). The setters (`SetDefaults`, `SetFloat`, ...) need
//! the device and still stop the run with a named message.

use super::d3d9::{CODE_BASE, D3DERR_INVALIDCALL};
use crate::{AbiFn, Engine, Ptr};
use d3dx_shader::ctab::{Node, Table, RS_SAMPLER};
use d3dx_shader::{
    cache, vkd3d::LazyVkd3d, Cache, CacheCompiler, CompileError, Kind, Macro, Request,
    ShaderCompiler,
};
use std::cell::OnceCell;

const BUFFER_CODE: u32 = CODE_BASE + 0x0a00;
const TABLE_CODE: u32 = CODE_BASE + 0x0a40;
const TABLE_SLOTS: u32 = 29;
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
    let readers: [AbiFn; 6] = [
        crate::entry!(0, table_get_desc(Ptr, Ptr) -> u32).1,
        crate::entry!(0, table_get_constant_desc(Ptr, u32, Ptr, Ptr) -> u32).1,
        crate::entry!(0, table_get_sampler_index(Ptr, u32) -> u32).1,
        crate::entry!(0, table_get_constant(Ptr, u32, u32) -> u32).1,
        crate::entry!(0, table_get_constant_by_name(Ptr, u32, u32) -> u32).1,
        crate::entry!(0, table_get_constant_element(Ptr, u32, u32) -> u32).1,
    ];
    for (i, f) in readers.into_iter().enumerate() {
        e.register(TABLE_CODE + 4 * (5 + i as u32), f);
    }
    for i in 11..TABLE_SLOTS {
        e.register(
            TABLE_CODE + 4 * i,
            crate::entry!(0, table_not_provided(Ptr) -> u32).1,
        );
    }
}

/// An object `[vtable, references, data, size, handles, handle count]` of `bytes`, with `code` as
/// its slot table. The handles (tables only) are a block of 16-byte records, one per constant,
/// array element and structure member of the table; a handle is the address of its record.
fn new_object(e: &mut Engine, code: u32, slots: u32, bytes: &[u8]) -> u32 {
    register(e);
    let table = e.mem.alloc(4 * slots);
    for i in 0..slots {
        e.mem.set_u32(table + 4 * i, code + 4 * i);
    }
    let object = e.mem.alloc(24);
    let data = e.mem.alloc(bytes.len().max(1) as u32);
    e.mem.write(data, bytes);
    let count = if code == TABLE_CODE {
        Table::parse(bytes).map_or(0, |t| t.nodes().len() as u32)
    } else {
        0
    };
    let handles = if count == 0 {
        0
    } else {
        e.mem.alloc(16 * count)
    };
    for i in 0..count {
        e.mem.set_u32(handles + 16 * i, object);
        e.mem.set_u32(handles + 16 * i + 4, i);
    }
    for (i, v) in [table, 1, data, bytes.len() as u32, handles, count]
        .into_iter()
        .enumerate()
    {
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
        let handles = e.mem.u32(this.addr() + 16);
        if handles != 0 {
            e.mem.free(handles);
        }
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
        "ID3DXConstantTable's setters (SetDefaults, SetFloat, SetMatrix, ...) are not provided off Windows yet \
         (nv-linux#178: they need the game device)"
    )
}

/// The table object's `CTAB`, parsed, with its nodes; `f` gets them and the payload's address.
fn with_ctab<R>(
    e: &Engine,
    this: Ptr,
    f: impl FnOnce(&Table<'_>, &[Node], u32, u32) -> Option<R>,
) -> Option<R> {
    let (data, size) = (e.mem.u32(this.addr() + 8), e.mem.u32(this.addr() + 12));
    let bytes = e.mem.bytes(data, size);
    let table = Table::parse(&bytes)?;
    let nodes = table.nodes();
    f(&table, &nodes, data, e.mem.u32(this.addr() + 16))
}

/// The node a handle names: its record's index, or (as D3DX accepts) a name string.
fn resolve(
    table: &Table<'_>,
    nodes: &[Node],
    e: &Engine,
    handle: u32,
    block: u32,
) -> Option<usize> {
    if block != 0 && handle >= block && ((handle - block) as usize) < 16 * nodes.len() {
        return ((handle - block) % 16 == 0).then(|| ((handle - block) / 16) as usize);
    }
    if handle == 0 {
        return None;
    }
    table.by_name(nodes, None, &e.mem.cstr(handle))
}

fn handle_of(block: u32, node: Option<usize>) -> u32 {
    node.map_or(0, |i| block + 16 * i as u32)
}

/// `GetDesc(out)`: `D3DXCONSTANTTABLE_DESC { Creator, Version, Constants }`.
fn table_get_desc(e: &mut Engine, this: Ptr, out: Ptr) -> u32 {
    if out.is_null() {
        return D3DERR_INVALIDCALL;
    }
    let desc = with_ctab(e, this, |t, _, data, _| {
        let d = t.desc();
        Some((d.creator.map_or(0, |c| data + c), d.version, d.constants))
    });
    let Some((creator, version, constants)) = desc else {
        return D3DERR_INVALIDCALL;
    };
    for (i, v) in [creator, version, constants].into_iter().enumerate() {
        e.mem.set_u32(out.addr() + 4 * i as u32, v);
    }
    0
}

/// `GetConstantDesc(handle, out, count)`: `D3DXCONSTANT_DESC` (12 words) of one handle, `*count`
/// set to 1. A handle that names nothing answers `D3DERR_INVALIDCALL`.
fn table_get_constant_desc(e: &mut Engine, this: Ptr, handle: u32, out: Ptr, count: Ptr) -> u32 {
    if out.is_null() && count.is_null() {
        return D3DERR_INVALIDCALL;
    }
    let found = with_ctab(e, this, |t, nodes, data, block| {
        let node = &nodes[resolve(t, nodes, e, handle, block)?];
        Some((node.desc.clone(), data))
    });
    let Some((d, data)) = found else {
        return D3DERR_INVALIDCALL;
    };
    if !out.is_null() {
        let words = [
            data + d.name,
            d.register_set,
            d.register_index,
            d.register_count,
            d.class,
            d.kind,
            d.rows,
            d.columns,
            d.elements,
            d.struct_members,
            d.bytes,
            d.default.map_or(0, |x| data + x),
        ];
        for (i, v) in words.into_iter().enumerate() {
            e.mem.set_u32(out.addr() + 4 * i as u32, v);
        }
    }
    if !count.is_null() {
        e.mem.set_u32(count.addr(), 1);
    }
    0
}

/// `GetSamplerIndex(handle)`: the sampler register, `0xffff_ffff` for anything else.
fn table_get_sampler_index(e: &mut Engine, this: Ptr, handle: u32) -> u32 {
    with_ctab(e, this, |t, nodes, _, block| {
        let d = &nodes[resolve(t, nodes, e, handle, block)?].desc;
        (d.register_set == RS_SAMPLER).then_some(d.register_index)
    })
    .unwrap_or(u32::MAX)
}

/// `GetConstant(handle, index)`: constant `index` of the table (null handle) or member `index` of
/// a structure; a null handle (`D3DXHANDLE`) answers for nothing.
fn table_get_constant(e: &mut Engine, this: Ptr, handle: u32, index: u32) -> u32 {
    with_ctab(e, this, |t, nodes, _, block| {
        let parent = match handle {
            0 => None,
            h => Some(resolve(t, nodes, e, h, block)?),
        };
        Some(handle_of(block, t.get_constant(nodes, parent, index)))
    })
    .unwrap_or(0)
}

/// `GetConstantByName(handle, name)`: a null handle searches the table, another the children of
/// that constant; `a.b[2]` paths are followed.
fn table_get_constant_by_name(e: &mut Engine, this: Ptr, handle: u32, name: u32) -> u32 {
    if name == 0 {
        return 0;
    }
    let name = e.mem.cstr(name);
    with_ctab(e, this, |t, nodes, _, block| {
        let parent = match handle {
            0 => None,
            h => Some(resolve(t, nodes, e, h, block)?),
        };
        Some(handle_of(block, t.by_name(nodes, parent, &name)))
    })
    .unwrap_or(0)
}

/// `GetConstantElement(handle, index)`: element `index` of an array constant (a constant that is
/// not an array has the one element, itself).
fn table_get_constant_element(e: &mut Engine, this: Ptr, handle: u32, index: u32) -> u32 {
    with_ctab(e, this, |t, nodes, _, block| {
        let at = resolve(t, nodes, e, handle, block)?;
        let hit = if nodes[at].desc.elements > 1 {
            t.element(nodes, at, index)
        } else {
            (index == 0).then_some(at)
        };
        Some(handle_of(block, hit))
    })
    .unwrap_or(0)
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

/// The profile string in engine memory.
fn profile(e: &mut Engine, text: &str) -> u32 {
    let at = e.mem.alloc(text.len() as u32 + 1);
    e.mem.write(at, text.as_bytes());
    e.mem.set_u8(at + text.len() as u32, 0);
    at
}

const CAPS_VERTEX_SHADER_VERSION: u32 = 196;
const CAPS_PIXEL_SHADER_VERSION: u32 = 204;
const CAPS_VS20: u32 = 248;
const CAPS_PS20: u32 = 264;
const D3DCAPS9_SIZE: u32 = 304;

/// The device's `D3DCAPS9` (`IDirect3DDevice9::GetDeviceCaps`, slot 7); `None` for a null device or
/// a device that refuses.
fn device_caps(e: &mut Engine, device: Ptr) -> Option<Vec<u8>> {
    if device.is_null() {
        return None;
    }
    e.with_stack(D3DCAPS9_SIZE, |e, caps| {
        let hr = e.vcall(device.addr(), 4 * 7, &[caps.addr()]).u32() as i32;
        (hr >= 0).then(|| e.mem.bytes(caps.addr(), D3DCAPS9_SIZE))
    })
}

fn caps_word(caps: &[u8], at: u32) -> u32 {
    let at = at as usize;
    u32::from_le_bytes([caps[at], caps[at + 1], caps[at + 2], caps[at + 3]])
}

/// The vertex profile the caps give, as D3DX picks it: the shader version, and for 2.x the `vs_2_a`
/// profile when the device has predication, dynamic flow control or more than 12 temporaries.
pub fn vertex_profile(caps: &[u8]) -> Option<&'static str> {
    let version = caps_word(caps, CAPS_VERTEX_SHADER_VERSION);
    let (major, _minor) = ((version >> 8) & 0xff, version & 0xff);
    match major {
        3.. => Some("vs_3_0"),
        2 => {
            let (flags, dynamic, temps) = (
                caps_word(caps, CAPS_VS20),
                caps_word(caps, CAPS_VS20 + 4),
                caps_word(caps, CAPS_VS20 + 8),
            );
            Some(if flags & 1 != 0 || dynamic > 0 || temps > 12 {
                "vs_2_a"
            } else {
                "vs_2_0"
            })
        }
        1 => Some("vs_1_1"),
        _ => None,
    }
}

/// The pixel profile the caps give: 1.1 to 1.4 by minor version, 2.x as `ps_2_a` with dynamic flow
/// control, `ps_2_b` with 512 or more instruction slots, else `ps_2_0`, and `ps_3_0`.
pub fn pixel_profile(caps: &[u8]) -> Option<&'static str> {
    let version = caps_word(caps, CAPS_PIXEL_SHADER_VERSION);
    let (major, minor) = ((version >> 8) & 0xff, version & 0xff);
    match major {
        3.. => Some("ps_3_0"),
        2 => {
            let (dynamic, slots) = (
                caps_word(caps, CAPS_PS20 + 4),
                caps_word(caps, CAPS_PS20 + 16),
            );
            Some(if dynamic > 0 {
                "ps_2_a"
            } else if slots >= 512 {
                "ps_2_b"
            } else {
                "ps_2_0"
            })
        }
        1 => Some(match minor {
            4.. => "ps_1_4",
            3 => "ps_1_3",
            2 => "ps_1_2",
            _ => "ps_1_1",
        }),
        _ => None,
    }
}

// Platform: replaces d3dx9_38.dll D3DXGetVertexShaderProfile (import)
/// `D3DXGetVertexShaderProfile(device)`: the profile of the device's caps, null (as D3DX) for a
/// null device or one without shaders.
pub fn get_vertex_shader_profile(e: &mut Engine, device: Ptr) -> u32 {
    match device_caps(e, device).as_deref().and_then(vertex_profile) {
        Some(p) => profile(e, p),
        None => 0,
    }
}

// Platform: replaces d3dx9_38.dll D3DXGetPixelShaderProfile (import, thunk 00ee6e28)
/// `D3DXGetPixelShaderProfile(device)`: as the vertex one, for the pixel shader version.
pub fn get_pixel_shader_profile(e: &mut Engine, device: Ptr) -> u32 {
    match device_caps(e, device).as_deref().and_then(pixel_profile) {
        Some(p) => profile(e, p),
        None => 0,
    }
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

    // A mock device whose `GetDeviceCaps` (slot 7) answers `CAPS`.
    thread_local! {
        static CAPS: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    fn mock_get_device_caps(e: &mut Engine, _this: Ptr, out: Ptr) -> u32 {
        CAPS.with(|c| e.mem.write(out.addr(), &c.borrow()));
        0
    }

    fn mock_device(e: &mut Engine, vs: u32, ps: u32, extra: &[(u32, u32)]) -> u32 {
        let mut caps = vec![0u8; 304];
        caps[196..200].copy_from_slice(&vs.to_le_bytes());
        caps[204..208].copy_from_slice(&ps.to_le_bytes());
        for (at, v) in extra {
            caps[*at as usize..*at as usize + 4].copy_from_slice(&v.to_le_bytes());
        }
        CAPS.with(|c| *c.borrow_mut() = caps);
        e.register(
            0x7e00_0000,
            crate::entry!(0, mock_get_device_caps(Ptr, Ptr) -> u32).1,
        );
        let table = e.mem.alloc(4 * 8);
        for i in 0..8 {
            e.mem
                .set_u32(table + 4 * i, if i == 7 { 0x7e00_0000 } else { 0 });
        }
        let device = e.mem.alloc(4);
        e.mem.set_u32(device, table);
        device
    }

    fn cstr_of(e: &mut Engine, f: u32, args: &[u32]) -> Vec<u8> {
        let p = e.call(f, args).u32();
        e.mem.cstr(p)
    }

    fn engine_with_imports() -> Engine {
        let mut e = Engine::new();
        for (i, (_, _, f)) in imports().iter().enumerate() {
            e.register(0x7f00_0000 + 4 * i as u32, *f);
        }
        e
    }

    #[test]
    fn profiles_follow_the_device_caps() {
        let (vs_call, ps_call) = (0x7f00_0000 + 8, 0x7f00_0000 + 12);
        let mut e = engine_with_imports();
        let sm3 = mock_device(&mut e, 0xfffe_0300, 0xffff_0300, &[]);
        let (v, p) = (e.call(vs_call, &[sm3]).u32(), e.call(ps_call, &[sm3]).u32());
        assert_eq!(e.mem.cstr(v), b"vs_3_0");
        assert_eq!(e.mem.cstr(p), b"ps_3_0");
        let sm2 = mock_device(&mut e, 0xfffe_0200, 0xffff_0200, &[(256, 12), (272, 12)]);
        let (v, p) = (e.call(vs_call, &[sm2]).u32(), e.call(ps_call, &[sm2]).u32());
        assert_eq!(e.mem.cstr(v), b"vs_2_0");
        assert_eq!(e.mem.cstr(p), b"ps_2_0");
        let sm1 = mock_device(&mut e, 0xfffe_0101, 0xffff_0104, &[]);
        assert_eq!(cstr_of(&mut e, vs_call, &[sm1]), b"vs_1_1");
        assert_eq!(cstr_of(&mut e, ps_call, &[sm1]), b"ps_1_4");
        let a = mock_device(&mut e, 0xfffe_0200, 0xffff_0200, &[(252, 24), (268, 24)]);
        assert_eq!(cstr_of(&mut e, vs_call, &[a]), b"vs_2_a");
        assert_eq!(cstr_of(&mut e, ps_call, &[a]), b"ps_2_a");
        let b = mock_device(&mut e, 0xfffe_0200, 0xffff_0200, &[(280, 512)]);
        assert_eq!(cstr_of(&mut e, ps_call, &[b]), b"ps_2_b");
        // A null device: NULL, as D3DX.
        assert_eq!(e.call(vs_call, &[0]).u32(), 0);
        assert_eq!(e.call(ps_call, &[0]).u32(), 0);
    }

    /// Hand-built bytecode: version token, a `CTAB` comment of the fixture table, end token.
    fn bytecode_with_table(table: &[u8]) -> Vec<u8> {
        let mut padded = b"CTAB".to_vec();
        padded.extend_from_slice(table);
        while padded.len() % 4 != 0 {
            padded.push(0);
        }
        let mut words = vec![0xffff_0300u32, 0xfffe | (((padded.len() / 4) as u32) << 16)];
        words.extend(
            padded
                .chunks(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])),
        );
        words.push(0x0000_ffff);
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    #[test]
    fn the_constant_table_answers_its_readers_from_hand_built_ctab() {
        let mut e = engine_with_imports();
        let code = bytecode_with_table(&d3dx_shader::ctab::synthetic_table());
        let at = e.mem.alloc(code.len() as u32);
        e.mem.write(at, &code);
        let out = e.mem.alloc(4);
        assert_eq!(e.call(0x7f00_0000 + 4, &[at, out]).u32(), 0);
        let t = e.mem.u32(out);
        let text = |e: &Engine, p: u32| String::from_utf8(e.mem.cstr(p)).unwrap();
        let name = |e: &mut Engine, s: &str| {
            let p = e.mem.alloc(s.len() as u32 + 1);
            e.mem.write(p, s.as_bytes());
            e.mem.set_u8(p + s.len() as u32, 0);
            p
        };
        fn by_name(e: &mut Engine, t: u32, slot: u32, h: u32, s: &str) -> u32 {
            let p = e.mem.alloc(s.len() as u32 + 1);
            e.mem.write(p, s.as_bytes());
            e.mem.set_u8(p + s.len() as u32, 0);
            e.vcall(t, slot, &[h, p]).u32()
        }
        // GetDesc (slot 5).
        let desc = e.mem.alloc(12 + 48 + 4);
        assert_eq!(e.vcall(t, 20, &[desc]).u32(), 0);
        assert!(text(&e, e.mem.u32(desc)).starts_with("Microsoft (R) HLSL"));
        assert_eq!((e.mem.u32(desc + 4), e.mem.u32(desc + 8)), (0xffff_0300, 5));
        // GetConstantByName (slot 9) then GetConstantDesc (slot 6).
        let tint_name = name(&mut e, "tint");
        let tint = e.vcall(t, 36, &[0, tint_name]).u32();
        assert_ne!(tint, 0);
        let (d, count) = (e.mem.alloc(48), e.mem.alloc(4));
        assert_eq!(e.vcall(t, 24, &[tint, d, count]).u32(), 0);
        assert_eq!(text(&e, e.mem.u32(d)), "tint");
        let words: Vec<u32> = (1..12).map(|i| e.mem.u32(d + 4 * i)).collect();
        assert_eq!(words[..10], [2, 4, 1, 1, 3, 1, 4, 1, 0, 16]);
        assert_eq!(e.mem.u32(count), 1);
        assert_eq!(e.mem.bytes(words[10], 4), 1.0f32.to_le_bytes());
        // A name as the handle is accepted, an unknown one is refused.
        assert_eq!(e.vcall(t, 24, &[tint_name, d, 0]).u32(), 0);
        let bad = name(&mut e, "nope");
        assert_eq!(e.vcall(t, 24, &[bad, d, 0]).u32(), D3DERR_INVALIDCALL);
        assert_eq!(e.vcall(t, 36, &[0, bad]).u32(), 0);
        // GetConstant (8), GetConstantElement (10), GetSamplerIndex (7).
        let first = e.vcall(t, 32, &[0, 0]).u32();
        assert_eq!(first, tint);
        let lights = by_name(&mut e, t, 36, 0, "lights");
        let second = e.vcall(t, 40, &[lights, 1]).u32();
        assert_eq!(second, by_name(&mut e, t, 36, 0, "lights[1]"));
        assert_eq!(e.vcall(t, 40, &[lights, 2]).u32(), 0);
        assert_eq!(e.vcall(t, 24, &[second, d, 0]).u32(), 0);
        assert_eq!(e.mem.u32(d + 8), 9);
        let diffuse = by_name(&mut e, t, 36, 0, "diffuse");
        assert_eq!(e.vcall(t, 28, &[diffuse]).u32(), 1);
        assert_eq!(e.vcall(t, 28, &[tint]).u32(), u32::MAX);
        let s = by_name(&mut e, t, 36, 0, "s");
        let m = e.vcall(t, 32, &[s, 1]).u32();
        assert_eq!(m, by_name(&mut e, t, 36, s, "m"));
        // Bytes and size of the table object still answer; Release frees it.
        assert_eq!(e.vcall(t, 8, &[]).u32(), 0);
    }

    #[test]
    fn a_shader_without_a_table_answers_invaliddata() {
        let mut e = engine_with_imports();
        let code: Vec<u8> = [0xffff_0300u32, 0x0000_ffff]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        let at = e.mem.alloc(8);
        e.mem.write(at, &code);
        let out = e.mem.alloc(4);
        assert_eq!(
            e.call(0x7f00_0000 + 4, &[at, out]).u32(),
            D3DXERR_INVALIDDATA
        );
        assert_eq!(e.mem.u32(out), 0);
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
