// SPDX-License-Identifier: GPL-3.0-only
//! The C runtime a plugin imports (UCRT's `api-ms-win-crt-*` and `vcruntime140`), as host
//! functions over guest memory: allocation, the string and memory functions, `printf`, `FILE`
//! streams over the mod-files seam, the start-up and exit tables. Written from the functions'
//! documented behaviour. The C++ library (`msvcp140`) is not here: it is mapped from a DLL the
//! user supplies (ADR-0018).

use crate::printf;
use crate::x86::Stop;
use crate::{current, state, HostFn};
use engine_core::prelude::*;
use std::collections::HashMap;

/// One open stream or file handle: its bytes in memory, written back through the mod-files seam
/// when closed or flushed.
#[derive(Default, Clone, Debug)]
pub struct VFile {
    pub name: Vec<u8>,
    pub data: Vec<u8>,
    pub pos: usize,
    pub write: bool,
    pub dirty: bool,
    pub eof: bool,
}

#[derive(Default)]
pub struct Rt {
    pub last_error: u32,
    pub errno: u32,
    pub tls: Vec<u32>,
    pub onexit: Vec<u32>,
    pub cur_exc: u32,
    pub iob: [u32; 3],
    pub files: HashMap<u32, VFile>,
    pub aligned: HashMap<u32, u32>,
    /// The high word of a 64-bit answer (`edx`) the function just set.
    pub hi: Option<u32>,
    pub env: HashMap<Vec<u8>, Vec<u8>>,
}

pub(crate) fn rt(e: &mut Engine) -> &mut Rt {
    &mut state(e).rt
}

pub(crate) fn rd(e: &Engine, a: u32, n: u32) -> Result<Vec<u8>, Stop> {
    let mut v = vec![0u8; n as usize];
    e.mem.try_read(a, &mut v).map_err(|_| Stop::Fault {
        addr: a,
        write: false,
    })?;
    Ok(v)
}

pub(crate) fn wr(e: &mut Engine, a: u32, d: &[u8]) -> Result<(), Stop> {
    e.mem.try_write(a, d).map_err(|_| Stop::Fault {
        addr: a,
        write: true,
    })
}

fn cstr(e: &Engine, a: u32) -> Result<Vec<u8>, Stop> {
    let mut b = [0u8; 1];
    e.mem.try_read(a, &mut b).map_err(|_| Stop::Fault {
        addr: a,
        write: false,
    })?;
    Ok(e.mem.cstr(a))
}

fn put32(e: &mut Engine, a: u32, v: u32) -> Result<(), Stop> {
    wr(e, a, &v.to_le_bytes())
}

// ---- memory and strings ----------------------------------------------------------------------

fn memcpy(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let d = rd(e, a[1], a[2])?;
    wr(e, a[0], &d)?;
    Ok(a[0])
}
fn memset(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    wr(e, a[0], &vec![a[1] as u8; a[2] as usize])?;
    Ok(a[0])
}
fn memcmp(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let (x, y) = (rd(e, a[0], a[2])?, rd(e, a[1], a[2])?);
    Ok(match x.cmp(&y) {
        std::cmp::Ordering::Less => u32::MAX,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    })
}
fn strlen(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    Ok(cstr(e, a[0])?.len() as u32)
}
fn strchr(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let s = cstr(e, a[0])?;
    let c = a[1] as u8;
    if c == 0 {
        return Ok(a[0] + s.len() as u32);
    }
    Ok(s.iter()
        .position(|&b| b == c)
        .map_or(0, |i| a[0] + i as u32))
}
fn strrchr(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let s = cstr(e, a[0])?;
    let c = a[1] as u8;
    if c == 0 {
        return Ok(a[0] + s.len() as u32);
    }
    Ok(s.iter()
        .rposition(|&b| b == c)
        .map_or(0, |i| a[0] + i as u32))
}
fn strstr(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let (h, n) = (cstr(e, a[0])?, cstr(e, a[1])?);
    if n.is_empty() {
        return Ok(a[0]);
    }
    Ok(h.windows(n.len())
        .position(|w| w == n.as_slice())
        .map_or(0, |i| a[0] + i as u32))
}
fn cmp_bytes(x: &[u8], y: &[u8], fold: bool) -> u32 {
    let f = |b: &u8| if fold { b.to_ascii_lowercase() } else { *b };
    let (x, y): (Vec<u8>, Vec<u8>) = (x.iter().map(f).collect(), y.iter().map(f).collect());
    match x.cmp(&y) {
        std::cmp::Ordering::Less => u32::MAX,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}
fn strcmp(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    Ok(cmp_bytes(&cstr(e, a[0])?, &cstr(e, a[1])?, false))
}
fn stricmp(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    Ok(cmp_bytes(&cstr(e, a[0])?, &cstr(e, a[1])?, true))
}
fn strnicmp(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let n = a[2] as usize;
    let (mut x, mut y) = (cstr(e, a[0])?, cstr(e, a[1])?);
    x.truncate(n);
    y.truncate(n);
    Ok(cmp_bytes(&x, &y, true))
}
fn strncmp(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let n = a[2] as usize;
    let (mut x, mut y) = (cstr(e, a[0])?, cstr(e, a[1])?);
    x.truncate(n);
    y.truncate(n);
    Ok(cmp_bytes(&x, &y, false))
}
fn tolower(_: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    Ok(u32::from((a[0] as u8).to_ascii_lowercase()))
}
fn toupper(_: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    Ok(u32::from((a[0] as u8).to_ascii_uppercase()))
}
const ERANGE: u32 = 34;
const EINVAL: u32 = 22;
const TRUNCATE: u32 = u32::MAX;
fn strcpy_s(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let s = cstr(e, a[2])?;
    if s.len() as u32 + 1 > a[1] {
        if a[1] > 0 {
            wr(e, a[0], &[0])?;
        }
        return Ok(ERANGE);
    }
    let mut v = s;
    v.push(0);
    wr(e, a[0], &v)?;
    Ok(0)
}
fn strncpy_s(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let mut s = cstr(e, a[2])?;
    let n = if a[3] == TRUNCATE {
        a[1].saturating_sub(1)
    } else {
        a[3]
    };
    if a[3] != TRUNCATE && (n as usize) < s.len() {
        s.truncate(n as usize);
    }
    if s.len() as u32 + 1 > a[1] {
        if a[3] == TRUNCATE {
            s.truncate(a[1].saturating_sub(1) as usize);
        } else {
            if a[1] > 0 {
                wr(e, a[0], &[0])?;
            }
            return Ok(ERANGE);
        }
    }
    s.push(0);
    wr(e, a[0], &s)?;
    Ok(0)
}
fn strcat_s(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let d = cstr(e, a[0])?;
    let s = cstr(e, a[2])?;
    if (d.len() + s.len()) as u32 + 1 > a[1] {
        return Ok(ERANGE);
    }
    let mut v = s;
    v.push(0);
    wr(e, a[0] + d.len() as u32, &v)?;
    Ok(0)
}
fn strncat_s(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let d = cstr(e, a[0])?;
    let mut s = cstr(e, a[2])?;
    if a[3] != TRUNCATE {
        s.truncate(a[3] as usize);
    }
    if (d.len() + s.len()) as u32 + 1 > a[1] {
        return Ok(ERANGE);
    }
    s.push(0);
    wr(e, a[0] + d.len() as u32, &s)?;
    Ok(0)
}
fn strdup(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let mut s = cstr(e, a[0])?;
    s.push(0);
    let p = e.mem.alloc_asked(s.len() as u32);
    wr(e, p, &s)?;
    Ok(p)
}

/// The number at the start of `s` as `strtol` reads it: (value, bytes used, overflowed).
fn parse_int(s: &[u8], base: u32) -> (i128, usize) {
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_whitespace() {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'-' || s[i] == b'+') {
        neg = s[i] == b'-';
        i += 1;
    }
    let mut base = base;
    if (base == 0 || base == 16) && s[i..].starts_with(b"0x") || s[i..].starts_with(b"0X") {
        if base == 0 || base == 16 {
            base = 16;
            i += 2;
        }
    } else if base == 0 {
        base = if s.get(i) == Some(&b'0') { 8 } else { 10 };
    }
    let start = i;
    let mut v: i128 = 0;
    while let Some(d) = s.get(i).and_then(|&c| (c as char).to_digit(base.max(2))) {
        v = (v * i128::from(base) + i128::from(d)).min(1 << 70);
        i += 1;
    }
    if i == start {
        return (0, 0);
    }
    (if neg { -v } else { v }, i)
}
fn strto(e: &mut Engine, a: &[u32], signed: bool, wide: bool) -> Result<u32, Stop> {
    let s = cstr(e, a[0])?;
    let (v, used) = parse_int(&s, a[2]);
    if a[1] != 0 {
        put32(e, a[1], a[0] + used as u32)?;
    }
    let v = if signed && !wide {
        v.clamp(i128::from(i32::MIN), i128::from(i32::MAX))
    } else if !signed && !wide {
        v.clamp(i128::from(i32::MIN), i128::from(u32::MAX))
    } else if signed {
        v.clamp(i128::from(i64::MIN), i128::from(i64::MAX))
    } else {
        v.clamp(i128::from(i64::MIN), i128::from(u64::MAX))
    };
    let bits = v as u64;
    rt(e).hi = Some((bits >> 32) as u32);
    Ok(bits as u32)
}
fn strtol(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    strto(e, a, true, false)
}
fn strtoul(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    strto(e, a, false, false)
}
fn strtoull(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    strto(e, a, false, true)
}

fn getenv_s(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    // getenv_s(&len, buf, size, name)
    let name = cstr(e, a[3])?;
    let value = rt(e).env.get(&name).cloned();
    match value {
        None => {
            if a[0] != 0 {
                put32(e, a[0], 0)?;
            }
            if a[1] != 0 && a[2] > 0 {
                wr(e, a[1], &[0])?;
            }
            Ok(0)
        }
        Some(v) => {
            if a[0] != 0 {
                put32(e, a[0], v.len() as u32 + 1)?;
            }
            if v.len() as u32 + 1 > a[2] {
                return Ok(ERANGE);
            }
            let mut v = v;
            v.push(0);
            wr(e, a[1], &v)?;
            Ok(0)
        }
    }
}
fn putenv_s(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let (n, v) = (cstr(e, a[0])?, cstr(e, a[1])?);
    if n.is_empty() {
        return Ok(EINVAL);
    }
    rt(e).env.insert(n, v);
    Ok(0)
}

// ---- heap ----------------------------------------------------------------------------------

fn malloc(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    Ok(e.mem.alloc_asked(a[0].max(1)))
}
fn calloc(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let n = a[0].saturating_mul(a[1]).max(1);
    let p = e.mem.alloc_asked(n);
    e.mem.zero(p, n);
    Ok(p)
}
fn free(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    if a[0] != 0 && e.mem.asked_size(a[0]).is_some() {
        e.mem.free_asked(a[0]);
    }
    Ok(0)
}
fn realloc(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    if a[0] == 0 {
        return malloc(e, &a[1..]);
    }
    if a[1] == 0 {
        free(e, a)?;
        return Ok(0);
    }
    let old = e.mem.asked_size(a[0]).unwrap_or(0);
    let p = e.mem.alloc_asked(a[1]);
    let n = old.min(a[1]);
    let d = rd(e, a[0], n)?;
    wr(e, p, &d)?;
    free(e, a)?;
    Ok(p)
}
fn aligned_malloc(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let al = a[1].max(16).next_power_of_two();
    let raw = e.mem.alloc_asked(a[0].saturating_add(al));
    let p = (raw + al - 1) & !(al - 1);
    rt(e).aligned.insert(p, raw);
    Ok(p)
}
fn aligned_free(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    if let Some(raw) = rt(e).aligned.remove(&a[0]) {
        e.mem.free_asked(raw);
    }
    Ok(0)
}

// ---- start-up, exit, exceptions ------------------------------------------------------------

fn zero(_: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    Ok(0)
}
fn exit(_: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    Err(Stop::Exit)
}
fn initterm(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    run_table(e, a[0], a[1], false)
}
fn initterm_e(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    run_table(e, a[0], a[1], true)
}
fn run_table(e: &mut Engine, from: u32, to: u32, checked: bool) -> Result<u32, Stop> {
    let plugin = current(e)?;
    let mut p = from;
    while p < to {
        let f = rd(e, p, 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))?;
        if f != 0 {
            let r = crate::call_guest(e, plugin, f, &[])?;
            if checked && r != 0 {
                return Ok(r);
            }
        }
        p += 4;
    }
    Ok(0)
}
fn onexit_init(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    wr(e, a[0], &[0u8; 12])?;
    Ok(0)
}
fn onexit_register(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    rt(e).onexit.push(a[1]);
    Ok(0)
}
fn atexit(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    rt(e).onexit.push(a[0]);
    Ok(0)
}
fn errno_addr(e: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    if rt(e).errno == 0 {
        let p = e.mem.alloc(8);
        e.mem.zero(p, 8);
        rt(e).errno = p;
    }
    Ok(rt(e).errno)
}
fn current_exception(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let _ = a;
    if rt(e).cur_exc == 0 {
        let p = e.mem.alloc(16);
        e.mem.zero(p, 16);
        rt(e).cur_exc = p;
    }
    Ok(rt(e).cur_exc)
}
fn current_exception_context(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    Ok(current_exception(e, a)? + 8)
}
fn exception_copy(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    // __std_exception_copy(src, dst): both {const char *what; bool do_free}
    let w = rd(e, a[0], 4)?;
    wr(e, a[1], &w)?;
    wr(e, a[1] + 4, &[0, 0, 0, 0])?;
    Ok(0)
}

// ---- stdio -----------------------------------------------------------------------------------

fn new_file(e: &mut Engine, f: VFile) -> u32 {
    let p = e.mem.alloc(32);
    e.mem.zero(p, 32);
    rt(e).files.insert(p, f);
    p
}

/// Opens `name` as `fopen` does with `mode`; `None` when it cannot be (a file that is not there
/// for reading).
pub(crate) fn open(e: &mut Engine, name: &[u8], mode: &[u8]) -> Option<u32> {
    let m = mode.first().copied()?;
    let mut f = VFile {
        name: name.to_vec(),
        ..VFile::default()
    };
    match m {
        b'r' => {
            f.data = engine_core::mod_files::read(e, name)?;
            f.write = mode.contains(&b'+');
        }
        b'w' => {
            f.write = true;
            f.dirty = true;
        }
        b'a' => {
            f.data = engine_core::mod_files::read(e, name).unwrap_or_default();
            f.pos = f.data.len();
            f.write = true;
            f.dirty = true;
        }
        _ => return None,
    }
    Some(new_file(e, f))
}

fn flush_file(e: &mut Engine, p: u32) {
    let Some(f) = rt(e).files.get(&p).cloned() else {
        return;
    };
    if f.write && f.dirty {
        let _ = engine_core::mod_files::write(e, &f.name, &f.data);
    }
}

fn fopen(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let (n, m) = (cstr(e, a[0])?, cstr(e, a[1])?);
    Ok(open(e, &n, &m).unwrap_or(0))
}
fn fsopen(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    fopen(e, a)
}
fn fopen_s(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let (n, m) = (cstr(e, a[1])?, cstr(e, a[2])?);
    match open(e, &n, &m) {
        Some(p) => {
            put32(e, a[0], p)?;
            Ok(0)
        }
        None => {
            put32(e, a[0], 0)?;
            Ok(2)
        }
    }
}
fn wide(e: &Engine, mut a: u32) -> Vec<u8> {
    let mut v = Vec::new();
    loop {
        let c = e.mem.u16(a);
        if c == 0 {
            return v;
        }
        v.push(if c < 0x80 { c as u8 } else { b'?' });
        a += 2;
    }
}
fn wfopen_s(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let (n, m) = (wide(e, a[1]), wide(e, a[2]));
    match open(e, &n, &m) {
        Some(p) => {
            put32(e, a[0], p)?;
            Ok(0)
        }
        None => {
            put32(e, a[0], 0)?;
            Ok(2)
        }
    }
}
fn fclose(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    flush_file(e, a[0]);
    Ok(if rt(e).files.remove(&a[0]).is_some() {
        0
    } else {
        u32::MAX
    })
}
fn fflush(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    flush_file(e, a[0]);
    if let Some(f) = rt(e).files.get_mut(&a[0]) {
        f.dirty = false;
    }
    Ok(0)
}
fn fread(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let (buf, size, count, p) = (a[0], a[1] as usize, a[2] as usize, a[3]);
    let Some(f) = rt(e).files.get_mut(&p) else {
        return Ok(0);
    };
    if size == 0 {
        return Ok(0);
    }
    let avail = f.data.len().saturating_sub(f.pos);
    let items = (avail / size).min(count);
    if items < count {
        f.eof = true;
    }
    let chunk = f.data[f.pos..f.pos + items * size].to_vec();
    f.pos += chunk.len();
    wr(e, buf, &chunk)?;
    Ok(items as u32)
}
fn write_to(e: &mut Engine, p: u32, d: &[u8]) -> bool {
    if rt(e).iob.contains(&p) {
        return true;
    }
    let Some(f) = rt(e).files.get_mut(&p) else {
        return false;
    };
    if !f.write {
        return false;
    }
    let end = f.pos + d.len();
    if f.data.len() < end {
        f.data.resize(end, 0);
    }
    f.data[f.pos..end].copy_from_slice(d);
    f.pos = end;
    f.dirty = true;
    true
}
fn fwrite(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let d = rd(e, a[0], a[1].saturating_mul(a[2]))?;
    Ok(if write_to(e, a[3], &d) { a[2] } else { 0 })
}
fn fputs(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let d = cstr(e, a[0])?;
    Ok(if write_to(e, a[1], &d) { 0 } else { u32::MAX })
}
fn fputc(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    Ok(if write_to(e, a[1], &[a[0] as u8]) {
        a[0] & 0xff
    } else {
        u32::MAX
    })
}
fn fgetc(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let Some(f) = rt(e).files.get_mut(&a[0]) else {
        return Ok(u32::MAX);
    };
    match f.data.get(f.pos) {
        Some(&b) => {
            f.pos += 1;
            Ok(u32::from(b))
        }
        None => {
            f.eof = true;
            Ok(u32::MAX)
        }
    }
}
fn ungetc(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let Some(f) = rt(e).files.get_mut(&a[1]) else {
        return Ok(u32::MAX);
    };
    if f.pos == 0 || a[0] == u32::MAX {
        return Ok(u32::MAX);
    }
    f.pos -= 1;
    f.eof = false;
    Ok(a[0] & 0xff)
}
fn seek(e: &mut Engine, p: u32, off: i64, whence: u32) -> u32 {
    let Some(f) = rt(e).files.get_mut(&p) else {
        return u32::MAX;
    };
    let base = match whence {
        0 => 0i64,
        1 => f.pos as i64,
        _ => f.data.len() as i64,
    };
    let to = base + off;
    if to < 0 {
        return u32::MAX;
    }
    f.pos = to as usize;
    f.eof = false;
    0
}
fn fseek(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    Ok(seek(e, a[0], i64::from(a[1] as i32), a[2]))
}
fn fseeki64(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let off = (u64::from(a[2]) << 32 | u64::from(a[1])) as i64;
    Ok(seek(e, a[0], off, a[3]))
}
fn ftell(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    Ok(rt(e).files.get(&a[0]).map_or(u32::MAX, |f| f.pos as u32))
}
fn fgetpos(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let Some(pos) = rt(e).files.get(&a[0]).map(|f| f.pos as u64) else {
        return Ok(1);
    };
    wr(e, a[1], &pos.to_le_bytes())?;
    Ok(0)
}
fn fsetpos(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let b = rd(e, a[1], 8)?;
    let pos = u64::from_le_bytes(b.try_into().unwrap_or_default());
    Ok(seek(e, a[0], pos as i64, 0))
}
fn iob(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let i = (a[0] as usize).min(2);
    if rt(e).iob[i] == 0 {
        let p = e.mem.alloc(32);
        e.mem.zero(p, 32);
        rt(e).iob[i] = p;
    }
    Ok(rt(e).iob[i])
}
fn vsprintf(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    // (options lo, hi, buffer, count, format, locale, arglist)
    let fmt = cstr(e, a[4])?;
    let mut out = printf::format(e, &fmt, a[6])?;
    let n = out.len() as u32;
    if a[3] == 0 {
        return Ok(n);
    }
    if n + 1 > a[3] {
        out.truncate(a[3] as usize - 1);
        out.push(0);
        wr(e, a[2], &out)?;
        // vsprintf_s: the error; vsnprintf-style: the length it wanted. The caller's options say.
        return Ok(if a[0] & 2 != 0 { n } else { u32::MAX });
    }
    out.push(0);
    wr(e, a[2], &out)?;
    Ok(n)
}
fn vfprintf(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    // (options lo, hi, FILE*, format, locale, arglist)
    let fmt = cstr(e, a[3])?;
    let out = printf::format(e, &fmt, a[5])?;
    let n = out.len() as u32;
    Ok(if write_to(e, a[2], &out) { n } else { u32::MAX })
}

/// (dll group, name, function, argument words, stdcall). The group is a prefix of the DLL name
/// compared without case.
pub const SHIMS: &[(&str, &str, HostFn, u32)] = &[
    ("vcruntime", "memcpy", memcpy, 3),
    ("vcruntime", "memmove", memcpy, 3),
    ("vcruntime", "memset", memset, 3),
    ("vcruntime", "memcmp", memcmp, 3),
    ("vcruntime", "strchr", strchr, 2),
    ("vcruntime", "strrchr", strrchr, 2),
    ("vcruntime", "strstr", strstr, 2),
    ("vcruntime", "__current_exception", current_exception, 0),
    (
        "vcruntime",
        "__current_exception_context",
        current_exception_context,
        0,
    ),
    ("vcruntime", "__std_exception_copy", exception_copy, 2),
    ("vcruntime", "__std_exception_destroy", zero, 1),
    ("vcruntime", "__std_type_info_destroy_list", zero, 1),
    ("vcruntime", "__std_terminate", exit, 0),
    ("api-ms-win-crt-runtime", "_initterm", initterm, 2),
    ("api-ms-win-crt-runtime", "_initterm_e", initterm_e, 2),
    ("api-ms-win-crt-runtime", "_configure_narrow_argv", zero, 1),
    (
        "api-ms-win-crt-runtime",
        "_initialize_narrow_environment",
        zero,
        0,
    ),
    (
        "api-ms-win-crt-runtime",
        "_initialize_onexit_table",
        onexit_init,
        1,
    ),
    (
        "api-ms-win-crt-runtime",
        "_register_onexit_function",
        onexit_register,
        2,
    ),
    ("api-ms-win-crt-runtime", "_execute_onexit_table", zero, 1),
    ("api-ms-win-crt-runtime", "_crt_atexit", atexit, 1),
    ("api-ms-win-crt-runtime", "_cexit", zero, 0),
    ("api-ms-win-crt-runtime", "terminate", exit, 0),
    ("api-ms-win-crt-runtime", "abort", exit, 0),
    ("api-ms-win-crt-runtime", "_invoke_watson", exit, 5),
    ("api-ms-win-crt-runtime", "_seh_filter_dll", zero, 2),
    ("api-ms-win-crt-runtime", "_errno", errno_addr, 0),
    ("api-ms-win-crt-heap", "malloc", malloc, 1),
    ("api-ms-win-crt-heap", "calloc", calloc, 2),
    ("api-ms-win-crt-heap", "realloc", realloc, 2),
    ("api-ms-win-crt-heap", "free", free, 1),
    ("api-ms-win-crt-heap", "_aligned_malloc", aligned_malloc, 2),
    ("api-ms-win-crt-heap", "_aligned_free", aligned_free, 1),
    ("api-ms-win-crt-heap", "_callnewh", zero, 1),
    ("api-ms-win-crt-string", "strcpy_s", strcpy_s, 3),
    ("api-ms-win-crt-string", "strncpy_s", strncpy_s, 4),
    ("api-ms-win-crt-string", "strcat_s", strcat_s, 3),
    ("api-ms-win-crt-string", "strncat_s", strncat_s, 4),
    ("api-ms-win-crt-string", "_stricmp", stricmp, 2),
    ("api-ms-win-crt-string", "_strnicmp", strnicmp, 3),
    ("api-ms-win-crt-string", "strcmp", strcmp, 2),
    ("api-ms-win-crt-string", "strncmp", strncmp, 3),
    ("api-ms-win-crt-string", "strlen", strlen, 1),
    ("api-ms-win-crt-string", "tolower", tolower, 1),
    ("api-ms-win-crt-string", "toupper", toupper, 1),
    ("api-ms-win-crt-string", "_strdup", strdup, 1),
    ("api-ms-win-crt-convert", "strtol", strtol, 3),
    ("api-ms-win-crt-convert", "strtoul", strtoul, 3),
    ("api-ms-win-crt-convert", "strtoull", strtoull, 3),
    ("api-ms-win-crt-environment", "getenv_s", getenv_s, 4),
    ("api-ms-win-crt-environment", "_putenv_s", putenv_s, 2),
    ("api-ms-win-crt-filesystem", "_lock_file", zero, 1),
    ("api-ms-win-crt-filesystem", "_unlock_file", zero, 1),
    ("api-ms-win-crt-stdio", "fopen", fopen, 2),
    ("api-ms-win-crt-stdio", "_fsopen", fsopen, 3),
    ("api-ms-win-crt-stdio", "fopen_s", fopen_s, 3),
    ("api-ms-win-crt-stdio", "_wfopen_s", wfopen_s, 3),
    ("api-ms-win-crt-stdio", "fclose", fclose, 1),
    ("api-ms-win-crt-stdio", "fflush", fflush, 1),
    ("api-ms-win-crt-stdio", "fread", fread, 4),
    ("api-ms-win-crt-stdio", "fwrite", fwrite, 4),
    ("api-ms-win-crt-stdio", "fputs", fputs, 2),
    ("api-ms-win-crt-stdio", "fputc", fputc, 2),
    ("api-ms-win-crt-stdio", "fgetc", fgetc, 1),
    ("api-ms-win-crt-stdio", "ungetc", ungetc, 2),
    ("api-ms-win-crt-stdio", "fseek", fseek, 3),
    ("api-ms-win-crt-stdio", "_fseeki64", fseeki64, 4),
    ("api-ms-win-crt-stdio", "ftell", ftell, 1),
    ("api-ms-win-crt-stdio", "fgetpos", fgetpos, 2),
    ("api-ms-win-crt-stdio", "fsetpos", fsetpos, 2),
    ("api-ms-win-crt-stdio", "setvbuf", zero, 4),
    ("api-ms-win-crt-stdio", "__acrt_iob_func", iob, 1),
    (
        "api-ms-win-crt-stdio",
        "__stdio_common_vsprintf",
        vsprintf,
        7,
    ),
    (
        "api-ms-win-crt-stdio",
        "__stdio_common_vsprintf_s",
        vsprintf,
        7,
    ),
    (
        "api-ms-win-crt-stdio",
        "__stdio_common_vfprintf",
        vfprintf,
        6,
    ),
];

/// The shim for an import of a C runtime DLL, as (function, argument words).
pub fn lookup(dll: &str, name: &str) -> Option<(HostFn, u32)> {
    let dll = dll.to_ascii_lowercase();
    SHIMS
        .iter()
        .find(|s| dll.starts_with(s.0) && s.1 == name)
        .map(|s| (s.2, s.3))
}
