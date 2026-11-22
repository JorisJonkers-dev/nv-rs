// SPDX-License-Identifier: GPL-3.0-only
//! A made-up plugin DLL, assembled here from our own source for the tests: nothing of any real
//! plugin and no binary in the repository. It is a 32-bit PE with one section, one export table,
//! one import (`KERNEL32.dll!GetCurrentProcessId`) and a relocation table, and this source:
//!
//! ```c
//! static const char NAME[] = "TestPlugin";
//! static CommandInfo CMD = { "TestPluginAnswer", "", 0, "", 0, 0, 0, Execute, 0, 0, 0 };
//! static unsigned g_last_type, g_count, g_pid;
//! bool NVSEPlugin_Query(const NVSEInterface *nvse, PluginInfo *info) {
//!     info->infoVersion = 1; info->name = NAME; info->version = 7; return true;
//! }
//! bool NVSEPlugin_Load(const NVSEInterface *nvse) {
//!     nvse->SetOpcodeBase(0x4100);
//!     nvse->RegisterCommand(&CMD);
//!     PluginHandle h = nvse->GetPluginHandle();
//!     NVSEMessagingInterface *m = nvse->QueryInterface(kInterface_Messaging);
//!     m->RegisterListener(h, "NVSE", Listener);
//!     g_pid = GetCurrentProcessId();
//!     return true;
//! }
//! bool Execute(COMMAND_ARGS) { *result = 42.0; return true; }
//! void Listener(Message *msg) { g_last_type = msg->type; g_count++; }
//! ```

pub const IMAGE_BASE: u32 = 0x1000_0000;
const SECTION_RVA: u32 = 0x1000;

// Offsets in the section.
pub const NAME: u32 = 0x00;
const CMD_NAME: u32 = 0x10;
const EMPTY: u32 = 0x30;
const SENDER: u32 = 0x34;
pub const G_LAST: u32 = 0x40;
pub const G_COUNT: u32 = 0x44;
pub const G_PID: u32 = 0x48;
const CMD: u32 = 0x50;
const EXPORTS: u32 = 0x80;
const IMPORTS: u32 = 0x100;
const ILT: u32 = 0x130;
const IAT: u32 = 0x140;
const HINT: u32 = 0x150;
const DLL_NAME: u32 = 0x1c0;
const RELOCS: u32 = 0x200;
const CODE: u32 = 0x300;

pub const OPCODE_BASE: u32 = 0x4100;

fn put32(v: &mut [u8], at: u32, x: u32) {
    v[at as usize..at as usize + 4].copy_from_slice(&x.to_le_bytes());
}

fn put16(v: &mut [u8], at: u32, x: u16) {
    v[at as usize..at as usize + 2].copy_from_slice(&x.to_le_bytes());
}

fn put_str(v: &mut [u8], at: u32, s: &str) {
    v[at as usize..at as usize + s.len()].copy_from_slice(s.as_bytes());
}

fn abs(off: u32) -> u32 {
    IMAGE_BASE + SECTION_RVA + off
}

/// Assembles the DLL. `extra_import` adds a second import the host does not provide (to test the
/// report); `query_ok` false makes the query answer no; `spin` makes the command loop forever.
pub fn build(extra_import: Option<&str>, spin: bool) -> Vec<u8> {
    let mut s = vec![0u8; 0x800];
    let mut relocs: Vec<u32> = Vec::new();
    put_str(&mut s, NAME, "TestPlugin");
    put_str(&mut s, CMD_NAME, "TestPluginAnswer");
    put_str(&mut s, SENDER, "NVSE");

    // CommandInfo: longName, shortName, opcode, help, needsParent+numParams, params, execute,
    // parse, eval, flags.
    let execute_at = CODE + 0xc0;
    put32(&mut s, CMD, abs(CMD_NAME));
    relocs.push(CMD);
    put32(&mut s, CMD + 4, abs(EMPTY));
    relocs.push(CMD + 4);
    put32(&mut s, CMD + 0x0c, abs(EMPTY));
    relocs.push(CMD + 0x0c);
    put32(&mut s, CMD + 0x18, abs(execute_at));
    relocs.push(CMD + 0x18);

    // Code.
    let mut c: Vec<u8> = Vec::new();
    let query_at = CODE;
    // Query(nvse, info)
    c.extend([0x8b, 0x44, 0x24, 0x08]); // mov eax,[esp+8]
    c.extend([0xc7, 0x00, 1, 0, 0, 0]); // mov dword [eax],1
    c.extend([0xc7, 0x40, 0x04]); // mov dword [eax+4], NAME
    relocs.push(CODE + c.len() as u32);
    c.extend(abs(NAME).to_le_bytes());
    c.extend([0xc7, 0x40, 0x08, 7, 0, 0, 0]); // mov dword [eax+8],7
    c.extend([0xb8, 1, 0, 0, 0, 0xc3]); // mov eax,1; ret
                                        // Load(nvse)
    while c.len() < 0x40 {
        c.push(0xcc);
    }
    let load_at = CODE + c.len() as u32;
    c.extend([0x53, 0x56, 0x57]); // push ebx; push esi; push edi
    c.extend([0x8b, 0x5c, 0x24, 0x10]); // mov ebx,[esp+16]
    c.extend([0x68]);
    c.extend(OPCODE_BASE.to_le_bytes()); // push base
    c.extend([0xff, 0x53, 0x14, 0x83, 0xc4, 0x04]); // call [ebx+0x14]; add esp,4
    c.push(0x68);
    relocs.push(CODE + c.len() as u32);
    c.extend(abs(CMD).to_le_bytes()); // push &CMD
    c.extend([0xff, 0x53, 0x10, 0x83, 0xc4, 0x04]); // call [ebx+0x10]; add esp,4
    c.extend([0xff, 0x53, 0x1c, 0x89, 0xc7]); // call [ebx+0x1c]; mov edi,eax
    c.extend([0x6a, 0x02, 0xff, 0x53, 0x18, 0x83, 0xc4, 0x04, 0x89, 0xc6]); // QueryInterface(2) -> esi
    c.push(0x68);
    relocs.push(CODE + c.len() as u32);
    c.extend(abs(CODE + 0x100).to_le_bytes()); // push Listener
    c.push(0x68);
    relocs.push(CODE + c.len() as u32);
    c.extend(abs(SENDER).to_le_bytes()); // push "NVSE"
    c.extend([0x57, 0xff, 0x56, 0x04, 0x83, 0xc4, 0x0c]); // push edi; call [esi+4]; add esp,12
    c.extend([0xff, 0x15]); // call [IAT]
    relocs.push(CODE + c.len() as u32);
    c.extend(abs(IAT).to_le_bytes());
    c.push(0xa3); // mov [G_PID],eax
    relocs.push(CODE + c.len() as u32);
    c.extend(abs(G_PID).to_le_bytes());
    if extra_import.is_some() {
        c.extend([0xff, 0x15]); // call [IAT+4]: the import the host does not provide
        relocs.push(CODE + c.len() as u32);
        c.extend(abs(IAT + 4).to_le_bytes());
    }
    c.extend([0xb8, 1, 0, 0, 0, 0x5f, 0x5e, 0x5b, 0xc3]); // mov eax,1; pop edi; pop esi; pop ebx; ret
    assert!(CODE + (c.len() as u32) <= execute_at);
    while (CODE + c.len() as u32) < execute_at {
        c.push(0xcc);
    }
    // Execute(..., result, ...) : *result = 42.0
    if spin {
        c.extend([0xeb, 0xfe]); // jmp $
    } else {
        c.extend([0x8b, 0x44, 0x24, 0x1c]); // mov eax,[esp+28]
        c.extend([0xc7, 0x00, 0, 0, 0, 0]); // mov dword [eax],0
        c.extend([0xc7, 0x40, 0x04, 0, 0, 0x45, 0x40]); // mov dword [eax+4],0x40450000
        c.extend([0xb8, 1, 0, 0, 0, 0xc3]);
    }
    while (CODE + c.len() as u32) < CODE + 0x100 {
        c.push(0xcc);
    }
    // Listener(msg): g_last = msg->type; g_count++
    c.extend([0x8b, 0x44, 0x24, 0x04, 0x8b, 0x40, 0x04]); // mov eax,[esp+4]; mov eax,[eax+4]
    c.push(0xa3);
    relocs.push(CODE + c.len() as u32);
    c.extend(abs(G_LAST).to_le_bytes());
    c.extend([0xff, 0x05]); // inc dword [G_COUNT]
    relocs.push(CODE + c.len() as u32);
    c.extend(abs(G_COUNT).to_le_bytes());
    c.push(0xc3);
    s[CODE as usize..CODE as usize + c.len()].copy_from_slice(&c);

    // Exports.
    let names = EXPORTS + 0x30;
    let n_load = names + 0x20;
    let n_query = n_load + 0x10;
    let dll = n_query + 0x14;
    put32(&mut s, EXPORTS + 12, SECTION_RVA + dll);
    put_str(&mut s, dll, "test.dll");
    put32(&mut s, EXPORTS + 16, 1);
    put32(&mut s, EXPORTS + 20, 2);
    put32(&mut s, EXPORTS + 24, 2);
    put32(&mut s, EXPORTS + 28, SECTION_RVA + EXPORTS + 0x28);
    put32(&mut s, EXPORTS + 32, SECTION_RVA + EXPORTS + 0x30);
    put32(&mut s, EXPORTS + 36, SECTION_RVA + EXPORTS + 0x38);
    put32(&mut s, EXPORTS + 0x28, SECTION_RVA + load_at);
    put32(&mut s, EXPORTS + 0x2c, SECTION_RVA + query_at);
    put32(&mut s, EXPORTS + 0x30, SECTION_RVA + n_load);
    put32(&mut s, EXPORTS + 0x34, SECTION_RVA + n_query);
    put16(&mut s, EXPORTS + 0x38, 0);
    put16(&mut s, EXPORTS + 0x3a, 1);
    put_str(&mut s, n_load, "NVSEPlugin_Load");
    put_str(&mut s, n_query, "NVSEPlugin_Query");

    // Imports: one descriptor, one or two functions.
    put32(&mut s, IMPORTS, SECTION_RVA + ILT);
    put32(&mut s, IMPORTS + 12, SECTION_RVA + DLL_NAME);
    put32(&mut s, IMPORTS + 16, SECTION_RVA + IAT);
    put_str(&mut s, DLL_NAME, "KERNEL32.dll");
    put_str(&mut s, HINT + 2, "GetCurrentProcessId");
    put32(&mut s, ILT, SECTION_RVA + HINT);
    put32(&mut s, IAT, SECTION_RVA + HINT);
    if let Some(name) = extra_import {
        put_str(&mut s, HINT + 0x1a + 2, name);
        put32(&mut s, ILT + 4, SECTION_RVA + HINT + 0x1a);
        put32(&mut s, IAT + 4, SECTION_RVA + HINT + 0x1a);
    }

    // Relocations: one block for the page.
    let size = 8 + 2 * relocs.len() as u32;
    let size = (size + 3) & !3;
    put32(&mut s, RELOCS, SECTION_RVA);
    put32(&mut s, RELOCS + 4, size);
    for (i, r) in relocs.iter().enumerate() {
        put16(
            &mut s,
            RELOCS + 8 + 2 * i as u32,
            0x3000 | ((*r as u16 + SECTION_RVA as u16) & 0xfff),
        );
    }
    let reloc_size = size;

    // The file: DOS header, PE header, optional header, one section.
    let raw_size = (s.len() as u32 + 0x1ff) & !0x1ff;
    let mut f = vec![0u8; 0x200 + raw_size as usize];
    f[0] = b'M';
    f[1] = b'Z';
    put32(&mut f, 0x3c, 0x40);
    f[0x40..0x44].copy_from_slice(b"PE\0\0");
    put16(&mut f, 0x44, 0x14c);
    put16(&mut f, 0x46, 1);
    put16(&mut f, 0x54, 0xe0);
    put16(&mut f, 0x56, 0x2102);
    let o = 0x58;
    put16(&mut f, o, 0x10b);
    put32(&mut f, o + 16, 0);
    put32(&mut f, o + 28, IMAGE_BASE);
    put32(&mut f, o + 32, 0x1000);
    put32(&mut f, o + 36, 0x200);
    put32(
        &mut f,
        o + 56,
        SECTION_RVA + ((s.len() as u32 + 0xfff) & !0xfff),
    );
    put32(&mut f, o + 60, 0x200);
    put32(&mut f, o + 92, 16);
    let dd = o + 96;
    put32(&mut f, dd, SECTION_RVA + EXPORTS);
    put32(&mut f, dd + 4, 0x60);
    put32(&mut f, dd + 8, SECTION_RVA + IMPORTS);
    put32(&mut f, dd + 12, 40);
    put32(&mut f, dd + 40, SECTION_RVA + RELOCS);
    put32(&mut f, dd + 44, reloc_size);
    let sh = o + 0xe0;
    put_str(&mut f, sh, ".text");
    put32(&mut f, sh + 8, s.len() as u32);
    put32(&mut f, sh + 12, SECTION_RVA);
    put32(&mut f, sh + 16, raw_size);
    put32(&mut f, sh + 20, 0x200);
    put32(&mut f, sh + 36, 0xe000_0060);
    f[0x200..0x200 + s.len()].copy_from_slice(&s);
    f
}
