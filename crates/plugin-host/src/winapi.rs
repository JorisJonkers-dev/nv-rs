// SPDX-License-Identifier: GPL-3.0-only
//! The kernel32 and user32 functions a plugin imports beyond the few in `lib.rs`, as host
//! functions: last error, file handles over the mod-files seam, locks (one thread runs, so they
//! do nothing), thread-local slots, string conversion, time. Written from the functions'
//! documented behaviour.

use crate::crt::{rd, rt, wr};
use crate::x86::Stop;
use crate::{report, state, HostFn};
use engine_core::prelude::*;

const INVALID_HANDLE: u32 = u32::MAX;
const ERROR_FILE_NOT_FOUND: u32 = 2;

fn zero(_: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    Ok(0)
}
fn one(_: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    Ok(1)
}
fn get_last_error(e: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    Ok(rt(e).last_error)
}
fn set_last_error(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    rt(e).last_error = a[0];
    Ok(0)
}
fn tick64(e: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    rt(e).hi = Some(0);
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u32))
}
fn system_time_as_file_time(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    // 100 ns ticks since 1601
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| (d.as_nanos() / 100) as u64)
        + 116_444_736_000_000_000;
    wr(e, a[0], &t.to_le_bytes())?;
    Ok(0)
}
fn slist_head(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    wr(e, a[0], &[0u8; 8])?;
    Ok(0)
}
fn message_box(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let text = String::from_utf8_lossy(&e.mem.cstr(a[1])).into_owned();
    let st = state(e);
    let who = st
        .running
        .last()
        .map_or(String::new(), |&i| st.plugins[i].file.clone());
    report(e, format!("{who}: MessageBox: {text}"));
    Ok(1)
}
fn module_file_name(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let who = state(e)
        .plugins
        .iter()
        .find(|p| p.base == a[0])
        .map(|p| format!("Data\\NVSE\\Plugins\\{}", p.file));
    let mut s = who
        .unwrap_or_else(|| "FalloutNV.exe".to_string())
        .into_bytes();
    s.truncate((a[2] as usize).saturating_sub(1));
    let n = s.len() as u32;
    s.push(0);
    wr(e, a[1], &s)?;
    Ok(n)
}
fn current_directory(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let s = b".";
    if a[0] < 2 {
        return Ok(2);
    }
    wr(e, a[1], &[s[0], 0])?;
    Ok(1)
}
fn multi_byte_to_wide(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    // (codepage, flags, src, srclen, dst, dstlen); each byte is one UTF-16 unit
    let mut s = if a[3] == INVALID_HANDLE {
        let mut s = e.mem.cstr(a[2]);
        s.push(0);
        s
    } else {
        rd(e, a[2], a[3])?
    };
    let units: Vec<u16> = s.drain(..).map(u16::from).collect();
    if a[5] == 0 {
        return Ok(units.len() as u32);
    }
    if (units.len() as u32) > a[5] {
        rt(e).last_error = 122;
        return Ok(0);
    }
    let bytes: Vec<u8> = units.iter().flat_map(|u| u.to_le_bytes()).collect();
    wr(e, a[4], &bytes)?;
    Ok(units.len() as u32)
}
fn wide_to_multi_byte(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    // (codepage, flags, src, srclen, dst, dstlen, default, used)
    let mut units = Vec::new();
    if a[3] == INVALID_HANDLE {
        let mut p = a[2];
        loop {
            let u = e.mem.u16(p);
            units.push(u);
            if u == 0 {
                break;
            }
            p += 2;
        }
    } else {
        for i in 0..a[3] {
            units.push(e.mem.u16(a[2] + 2 * i));
        }
    }
    let out: Vec<u8> = units
        .iter()
        .map(|&u| if u < 0x80 { u as u8 } else { b'?' })
        .collect();
    if a[5] == 0 {
        return Ok(out.len() as u32);
    }
    if out.len() as u32 > a[5] {
        rt(e).last_error = 122;
        return Ok(0);
    }
    wr(e, a[4], &out)?;
    Ok(out.len() as u32)
}
fn tls_alloc(e: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    rt(e).tls.push(0);
    Ok(rt(e).tls.len() as u32 - 1)
}
fn tls_get(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    Ok(rt(e).tls.get(a[0] as usize).copied().unwrap_or(0))
}
fn tls_set(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    match rt(e).tls.get_mut(a[0] as usize) {
        Some(s) => {
            *s = a[1];
            Ok(1)
        }
        None => Ok(0),
    }
}
fn heap_alloc(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    // HeapAlloc(heap, flags, size); flag 8 = zero
    let p = e.mem.alloc_asked(a[2].max(1));
    if a[1] & 8 != 0 {
        e.mem.zero(p, a[2].max(1));
    }
    Ok(p)
}
fn heap_free(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    if e.mem.asked_size(a[2]).is_some() {
        e.mem.free_asked(a[2]);
    }
    Ok(1)
}
fn local_free(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    if a[0] != 0 && e.mem.asked_size(a[0]).is_some() {
        e.mem.free_asked(a[0]);
    }
    Ok(0)
}
fn heap_handle(_: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    Ok(0x0100_0000)
}
fn private_profile_int(_: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    // GetPrivateProfileIntA(section, key, default, file): no file settings are read here.
    Ok(a[2])
}
fn invalid_handle(e: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    rt(e).last_error = ERROR_FILE_NOT_FOUND;
    Ok(INVALID_HANDLE)
}
fn create_file(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    // CreateFileA(name, access, share, security, disposition, flags, template)
    let name = e.mem.cstr(a[0]);
    let writing = a[1] & 0x4000_0000 != 0;
    let mode: &[u8] = match (writing, a[4]) {
        (true, 4) => b"a",
        (true, _) => b"w",
        (false, _) => b"r",
    };
    match crate::crt::open(e, &name, mode) {
        Some(h) => Ok(h),
        None => invalid_handle(e, a),
    }
}
fn read_file(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    // ReadFile(h, buf, n, *read, overlapped)
    let Some(f) = rt(e).files.get_mut(&a[0]) else {
        return Ok(0);
    };
    let n = (a[2] as usize).min(f.data.len().saturating_sub(f.pos));
    let chunk = f.data[f.pos..f.pos + n].to_vec();
    f.pos += n;
    wr(e, a[1], &chunk)?;
    if a[3] != 0 {
        wr(e, a[3], &(n as u32).to_le_bytes())?;
    }
    Ok(1)
}
fn get_file_size(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    Ok(rt(e)
        .files
        .get(&a[0])
        .map_or(INVALID_HANDLE, |f| f.data.len() as u32))
}
fn get_file_size_ex(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    let Some(n) = rt(e).files.get(&a[0]).map(|f| f.data.len() as u64) else {
        return Ok(0);
    };
    wr(e, a[1], &n.to_le_bytes())?;
    Ok(1)
}
fn set_file_pointer(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    // SetFilePointer(h, lo, *hi, method)
    let Some(f) = rt(e).files.get_mut(&a[0]) else {
        return Ok(INVALID_HANDLE);
    };
    let base = match a[3] {
        0 => 0i64,
        1 => f.pos as i64,
        _ => f.data.len() as i64,
    };
    f.pos = (base + i64::from(a[1] as i32)).max(0) as usize;
    Ok(f.pos as u32)
}
fn close_handle(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    if let Some(f) = rt(e).files.remove(&a[0]) {
        if f.write && f.dirty {
            let _ = engine_core::mod_files::write(e, &f.name, &f.data);
        }
    }
    Ok(1)
}

fn exit_process(_: &mut Engine, _: &[u32]) -> Result<u32, Stop> {
    Err(Stop::Exit)
}
fn identity(_: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    Ok(a[0])
}
fn write_file(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    // WriteFile(h, buf, n, *written, overlapped)
    let d = rd(e, a[1], a[2])?;
    let ok = match rt(e).files.get_mut(&a[0]) {
        Some(f) if f.write => {
            let end = f.pos + d.len();
            if f.data.len() < end {
                f.data.resize(end, 0);
            }
            f.data[f.pos..end].copy_from_slice(&d);
            f.pos = end;
            f.dirty = true;
            true
        }
        // The console handles swallow what is written.
        _ => rt(e).iob.contains(&a[0]) || a[0] <= 0x20,
    };
    if ok && a[3] != 0 {
        wr(e, a[3], &a[2].to_le_bytes())?;
    }
    Ok(u32::from(ok))
}
fn module_handle_ex(e: &mut Engine, a: &[u32]) -> Result<u32, Stop> {
    // GetModuleHandleExA(flags, name_or_address, *module): the plugin that owns the address
    let base = state(e)
        .plugins
        .iter()
        .find(|p| p.base != 0 && a[1] >= p.base && a[1] < p.base + 0x0400_0000)
        .map_or(0x0040_0000, |p| p.base);
    if a[2] != 0 {
        wr(e, a[2], &base.to_le_bytes())?;
    }
    Ok(1)
}

/// (name, function, argument words); all stdcall.
pub const KERNEL32: &[(&str, HostFn, u32)] = &[
    ("ExitProcess", exit_process, 1),
    ("DecodePointer", identity, 1),
    ("EncodePointer", identity, 1),
    ("WriteFile", write_file, 5),
    ("GetModuleHandleExA", module_handle_ex, 3),
    ("GetLastError", get_last_error, 0),
    ("SetLastError", set_last_error, 1),
    ("GetTickCount64", tick64, 0),
    ("GetSystemTimeAsFileTime", system_time_as_file_time, 1),
    ("InitializeSListHead", slist_head, 1),
    ("Sleep", zero, 1),
    ("CloseHandle", close_handle, 1),
    ("CreateFileA", create_file, 7),
    ("ReadFile", read_file, 5),
    ("GetFileSize", get_file_size, 2),
    ("GetFileSizeEx", get_file_size_ex, 2),
    ("SetFilePointer", set_file_pointer, 4),
    ("GetModuleFileNameA", module_file_name, 3),
    ("GetCurrentDirectoryA", current_directory, 2),
    ("EnterCriticalSection", zero, 1),
    ("LeaveCriticalSection", zero, 1),
    ("InitializeCriticalSection", zero, 1),
    ("InitializeCriticalSectionAndSpinCount", one, 2),
    ("DeleteCriticalSection", zero, 1),
    ("TryEnterCriticalSection", one, 1),
    ("AcquireSRWLockExclusive", zero, 1),
    ("AcquireSRWLockShared", zero, 1),
    ("ReleaseSRWLockExclusive", zero, 1),
    ("ReleaseSRWLockShared", zero, 1),
    ("SleepConditionVariableSRW", zero, 4),
    ("WakeAllConditionVariable", zero, 1),
    ("WakeConditionVariable", zero, 1),
    ("UnhandledExceptionFilter", zero, 1),
    ("SetUnhandledExceptionFilter", zero, 1),
    ("GetPrivateProfileIntA", private_profile_int, 4),
    ("MultiByteToWideChar", multi_byte_to_wide, 6),
    ("WideCharToMultiByte", wide_to_multi_byte, 8),
    ("TlsAlloc", tls_alloc, 0),
    ("TlsGetValue", tls_get, 1),
    ("TlsSetValue", tls_set, 2),
    ("TlsFree", one, 1),
    ("GetProcessHeap", heap_handle, 0),
    ("HeapAlloc", heap_alloc, 3),
    ("HeapFree", heap_free, 3),
    ("LocalFree", local_free, 1),
    ("FindClose", one, 1),
    ("AreFileApisANSI", one, 0),
    ("GetFileAttributesExW", zero, 3),
    ("CreateDirectoryW", zero, 2),
    ("FormatMessageA", zero, 7),
];

/// (name, function, argument words); all stdcall.
pub const USER32: &[(&str, HostFn, u32)] = &[
    ("MessageBoxA", message_box, 4),
    ("GetKeyboardLayout", zero, 1),
    ("GetKeyboardState", zero, 1),
    ("MapVirtualKeyExA", zero, 3),
    ("ToUnicodeEx", zero, 7),
];

/// The shim for a kernel32 or user32 import: (function, argument words).
pub fn lookup(dll: &str, name: &str) -> Option<(HostFn, u32)> {
    let table = match dll.to_ascii_lowercase().as_str() {
        "kernel32.dll" => KERNEL32,
        "user32.dll" => USER32,
        _ => return None,
    };
    table.iter().find(|s| s.0 == name).map(|s| (s.1, s.2))
}
