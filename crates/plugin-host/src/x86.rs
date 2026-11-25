// SPDX-License-Identifier: GPL-3.0-only
//! A 32-bit x86 interpreter over the guest memory (ADR-0018).
//!
//! It runs the code of a plugin DLL: the integer instruction set of 32-bit user code (the common
//! 80386 opcodes plus `movzx`, `movsx`, `imul`, `setcc`, `cmovcc`, `bswap`, string moves) with exact
//! flags. Not built yet, each answering [`Stop::BadOpcode`] with the bytes at the instruction: SSE and
//! SSE2 (the profile's heavy plugins use `movss`, `movsd`, `movups`, `xorps`, `cvtdq2pd` and about
//! thirty more), x87 (`fld`, `fstp`, `fmul`, `fadd`: engine-core's x87 model is to be reused),
//! `lock`-prefixed and `cmpxchg` instructions, `cpuid`, segment-prefixed accesses (`fs:`), 16-bit
//! arithmetic outside `mov`, `rcl` and `rcr`, and `bt`/`bsf`/`bsr`. Nothing here is taken from a
//! plugin's code or from an emulator; it is written from the Intel manual's instruction semantics.

use engine_core::prelude::*;

pub const EAX: usize = 0;
pub const ECX: usize = 1;
pub const EDX: usize = 2;
pub const EBX: usize = 3;
pub const ESP: usize = 4;
pub const EBP: usize = 5;
pub const ESI: usize = 6;
pub const EDI: usize = 7;

/// Why a run ended without returning to its caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stop {
    /// A read or write of an address that is not mapped.
    Fault { addr: u32, write: bool },
    /// The same, with the address of the instruction that made it (what [`Cpu::run`] answers).
    FaultAt { addr: u32, write: bool, eip: u32 },
    /// A host function (an import or interface call, named) reached an unmapped address with an
    /// argument it was given.
    HostFault {
        func: String,
        addr: u32,
        write: bool,
    },
    /// An instruction this interpreter does not have (the address and its first bytes).
    BadOpcode { eip: u32, bytes: [u8; 4] },
    /// The instruction budget of one call ran out (a loop that does not end).
    Budget,
    /// A call into something the host does not provide, named.
    Missing(String),
    /// The plugin asked for the process to end.
    Exit,
    /// A divide by zero or an overflowing divide.
    Divide { eip: u32 },
}

impl std::fmt::Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Stop::Fault { addr, write } => write!(
                f,
                "{} of unmapped address {addr:#010x}",
                if *write { "write" } else { "read" }
            ),
            Stop::FaultAt { addr, write, eip } => write!(
                f,
                "{} of unmapped address {addr:#010x} at {eip:#010x}",
                if *write { "write" } else { "read" }
            ),
            Stop::HostFault { func, addr, write } => write!(
                f,
                "{func} was given unmapped address {addr:#010x} to {}",
                if *write { "write" } else { "read" }
            ),
            Stop::BadOpcode { eip, bytes } => write!(
                f,
                "instruction not built at {eip:#010x}: {:02x} {:02x} {:02x} {:02x}",
                bytes[0], bytes[1], bytes[2], bytes[3]
            ),
            Stop::Budget => write!(f, "instruction budget used up"),
            Stop::Missing(what) => write!(f, "wanted {what}, which the host does not provide"),
            Stop::Exit => write!(f, "asked the process to end"),
            Stop::Divide { eip } => write!(f, "divide error at {eip:#010x}"),
        }
    }
}

type R<T> = Result<T, Stop>;

/// What the interpreter asks of its host at an address it does not run: the host's thunks
/// (imports, interface functions) and calls into the game's own code. `Some` says the address was
/// the host's and the call is done (the return address popped); `None` says run it as code.
pub trait Bridge {
    fn at(&mut self, e: &mut Engine, cpu: &mut Cpu, eip: u32) -> Option<R<()>>;
}

/// The registers and flags of one thread of plugin code.
#[derive(Clone, Debug, Default)]
pub struct Cpu {
    pub r: [u32; 8],
    pub eip: u32,
    pub cf: bool,
    pub zf: bool,
    pub sf: bool,
    pub of: bool,
    pub pf: bool,
    pub df: bool,
    /// Instructions run, for the budget.
    pub steps: u64,
    /// Where writes into the game's code go ([`crate::patches`]).
    pub shadow: Option<crate::patches::Shared>,
    /// The base of the `fs` segment: the thread block ([`crate::thread`]).
    pub fs_base: u32,
    /// The segment base the instruction being run adds to its memory operands (an `fs:` prefix:
    /// [`Cpu::fs_base`]; else 0, as `ds`, `es` and `ss` are flat).
    pub(crate) seg: u32,
    /// The instruction being run has an address-size prefix (`0x67`).
    pub(crate) addr16: bool,
}

#[derive(Clone, Copy)]
enum Loc {
    Reg(usize),
    Mem(u32),
}

struct Modrm {
    reg: usize,
    loc: Loc,
}

fn mask(size: u32) -> u32 {
    match size {
        1 => 0xff,
        2 => 0xffff,
        _ => 0xffff_ffff,
    }
}

fn sign_bit(size: u32) -> u32 {
    match size {
        1 => 0x80,
        2 => 0x8000,
        _ => 0x8000_0000,
    }
}

fn sext(v: u32, size: u32) -> i32 {
    match size {
        1 => v as u8 as i8 as i32,
        2 => v as u16 as i16 as i32,
        _ => v as i32,
    }
}

impl Cpu {
    // ---- memory ----
    fn rd(&self, e: &Engine, a: u32, size: u32) -> R<u32> {
        let mut b = [0u8; 4];
        if e.mem.try_read(a, &mut b[..size as usize]).is_err() {
            match &self.shadow {
                Some(s) if crate::patches::in_image(a, size) => {
                    s.borrow_mut().read(a, &mut b[..size as usize]);
                }
                _ => {
                    return Err(Stop::Fault {
                        addr: a,
                        write: false,
                    })
                }
            }
        }
        Ok(u32::from_le_bytes(b))
    }

    fn wr(&self, e: &mut Engine, a: u32, size: u32, v: u32) -> R<()> {
        let bytes = &v.to_le_bytes()[..size as usize];
        if e.mem.try_write(a, bytes).is_err() {
            match &self.shadow {
                Some(s) if crate::patches::in_image(a, size) => s.borrow_mut().write(a, bytes),
                _ => {
                    return Err(Stop::Fault {
                        addr: a,
                        write: true,
                    })
                }
            }
        }
        Ok(())
    }

    pub fn push(&mut self, e: &mut Engine, v: u32) -> R<()> {
        self.r[ESP] = self.r[ESP].wrapping_sub(4);
        self.wr(e, self.r[ESP], 4, v)
    }

    pub fn pop(&mut self, e: &Engine) -> R<u32> {
        let v = self.rd(e, self.r[ESP], 4)?;
        self.r[ESP] = self.r[ESP].wrapping_add(4);
        Ok(v)
    }

    // ---- registers by size ----
    fn reg(&self, n: usize, size: u32) -> u32 {
        match size {
            1 => {
                if n < 4 {
                    self.r[n] & 0xff
                } else {
                    (self.r[n - 4] >> 8) & 0xff
                }
            }
            2 => self.r[n] & 0xffff,
            _ => self.r[n],
        }
    }

    fn set_reg(&mut self, n: usize, size: u32, v: u32) {
        match size {
            1 => {
                if n < 4 {
                    self.r[n] = (self.r[n] & !0xff) | (v & 0xff);
                } else {
                    self.r[n - 4] = (self.r[n - 4] & !0xff00) | ((v & 0xff) << 8);
                }
            }
            2 => self.r[n] = (self.r[n] & !0xffff) | (v & 0xffff),
            _ => self.r[n] = v,
        }
    }

    fn get(&self, e: &Engine, loc: Loc, size: u32) -> R<u32> {
        match loc {
            Loc::Reg(n) => Ok(self.reg(n, size)),
            Loc::Mem(a) => self.rd(e, a, size),
        }
    }

    fn put(&mut self, e: &mut Engine, loc: Loc, size: u32, v: u32) -> R<()> {
        match loc {
            Loc::Reg(n) => {
                self.set_reg(n, size, v);
                Ok(())
            }
            Loc::Mem(a) => self.wr(e, a, size, v),
        }
    }

    // ---- instruction stream ----
    fn f8(&mut self, e: &Engine) -> R<u32> {
        let v = self.rd(e, self.eip, 1)?;
        self.eip = self.eip.wrapping_add(1);
        Ok(v)
    }

    fn f16(&mut self, e: &Engine) -> R<u32> {
        let v = self.rd(e, self.eip, 2)?;
        self.eip = self.eip.wrapping_add(2);
        Ok(v)
    }

    fn f32(&mut self, e: &Engine) -> R<u32> {
        let v = self.rd(e, self.eip, 4)?;
        self.eip = self.eip.wrapping_add(4);
        Ok(v)
    }

    fn modrm(&mut self, e: &Engine) -> R<Modrm> {
        let b = self.f8(e)?;
        let md = b >> 6;
        let reg = ((b >> 3) & 7) as usize;
        let rm = (b & 7) as usize;
        if md == 3 {
            return Ok(Modrm {
                reg,
                loc: Loc::Reg(rm),
            });
        }
        if self.addr16 {
            // 16-bit addressing (`0x67` before a memory operand): only the bare 16-bit offset
            // (mod 0, r/m 6), as in `67 64 ff 36 00 00` (`push fs:[0]`); register-based forms
            // are not built.
            if md == 0 && rm == 6 {
                let off = self.f16(e)?;
                return Ok(Modrm {
                    reg,
                    loc: Loc::Mem(off.wrapping_add(self.seg)),
                });
            }
            return Err(Stop::Missing("16-bit addressing (0x67 prefix)".into()));
        }
        let mut addr: u32;
        if rm == 4 {
            let sib = self.f8(e)?;
            let scale = sib >> 6;
            let index = ((sib >> 3) & 7) as usize;
            let base = (sib & 7) as usize;
            addr = if base == 5 && md == 0 {
                self.f32(e)?
            } else {
                self.r[base]
            };
            if index != 4 {
                addr = addr.wrapping_add(self.r[index] << scale);
            }
        } else if rm == 5 && md == 0 {
            addr = self.f32(e)?;
        } else {
            addr = self.r[rm];
        }
        match md {
            1 => addr = addr.wrapping_add(self.f8(e)? as u8 as i8 as i32 as u32),
            2 => addr = addr.wrapping_add(self.f32(e)?),
            _ => {}
        }
        Ok(Modrm {
            reg,
            loc: Loc::Mem(addr.wrapping_add(self.seg)),
        })
    }

    // ---- arithmetic with exact flags ----
    fn set_szp(&mut self, r: u32, size: u32) {
        let r = r & mask(size);
        self.zf = r == 0;
        self.sf = r & sign_bit(size) != 0;
        self.pf = (r as u8).count_ones() % 2 == 0;
    }

    /// `op`: 0 add, 1 or, 2 adc, 3 sbb, 4 and, 5 sub, 6 xor, 7 cmp.
    fn alu(&mut self, op: u8, a: u32, b: u32, size: u32) -> u32 {
        let m = u64::from(mask(size));
        let (a, b) = (u64::from(a) & m, u64::from(b) & m);
        let s = u64::from(sign_bit(size));
        let carry = u64::from(self.cf);
        let r = match op {
            0 | 2 => {
                let c = if op == 2 { carry } else { 0 };
                let r = a + b + c;
                self.cf = r > m;
                self.of = (a ^ r) & (b ^ r) & s != 0;
                r
            }
            5 | 7 | 3 => {
                let c = if op == 3 { carry } else { 0 };
                let r = a.wrapping_sub(b).wrapping_sub(c);
                self.cf = a < b + c;
                self.of = (a ^ b) & (a ^ r) & s != 0;
                r
            }
            1 => {
                self.cf = false;
                self.of = false;
                a | b
            }
            4 => {
                self.cf = false;
                self.of = false;
                a & b
            }
            _ => {
                self.cf = false;
                self.of = false;
                a ^ b
            }
        };
        let r = (r & m) as u32;
        self.set_szp(r, size);
        r
    }

    fn cond(&self, cc: u32) -> bool {
        let r = match cc >> 1 {
            0 => self.of,
            1 => self.cf,
            2 => self.zf,
            3 => self.cf || self.zf,
            4 => self.sf,
            5 => self.pf,
            6 => self.sf != self.of,
            _ => self.zf || self.sf != self.of,
        };
        r != (cc & 1 == 1)
    }

    fn shift(&mut self, op: usize, v: u32, count: u32, size: u32) -> R<u32> {
        let n = count & 31;
        if n == 0 {
            return Ok(v);
        }
        let bits = size * 8;
        let m = mask(size);
        let v = v & m;
        let r = match op {
            0 => {
                // rol
                let k = n % bits;
                let r = if k == 0 {
                    v
                } else {
                    ((v << k) | (v >> (bits - k))) & m
                };
                self.cf = r & 1 != 0;
                if n == 1 {
                    self.of = (r & sign_bit(size) != 0) != self.cf;
                }
                return Ok(r);
            }
            1 => {
                // ror
                let k = n % bits;
                let r = if k == 0 {
                    v
                } else {
                    ((v >> k) | (v << (bits - k))) & m
                };
                self.cf = r & sign_bit(size) != 0;
                if n == 1 {
                    self.of = (r & sign_bit(size) != 0) != (r & (sign_bit(size) >> 1) != 0);
                }
                return Ok(r);
            }
            4 | 6 => {
                // shl
                self.cf = if n <= bits {
                    (u64::from(v) >> (bits - n)) & 1 != 0
                } else {
                    false
                };
                let r = if n >= bits { 0 } else { (v << n) & m };
                if n == 1 {
                    self.of = (r & sign_bit(size) != 0) != self.cf;
                }
                r
            }
            5 => {
                // shr
                self.cf = (u64::from(v) >> (n - 1)) & 1 != 0;
                let r = if n >= bits { 0 } else { v >> n };
                if n == 1 {
                    self.of = v & sign_bit(size) != 0;
                }
                r
            }
            7 => {
                // sar
                let s = i64::from(sext(v, size));
                self.cf = ((s >> (n - 1).min(63)) & 1) != 0;
                if n == 1 {
                    self.of = false;
                }
                ((s >> n.min(63)) as u32) & m
            }
            _ => return Err(Stop::Missing("rcl/rcr".into())),
        };
        self.set_szp(r, size);
        Ok(r)
    }

    fn bad(&self, e: &Engine, start: u32) -> Stop {
        let mut b = [0u8; 4];
        let _ = e.mem.try_read(start, &mut b);
        Stop::BadOpcode {
            eip: start,
            bytes: b,
        }
    }

    /// Runs one instruction.
    pub fn step(&mut self, e: &mut Engine) -> R<()> {
        let start = self.eip;
        let mut size = 4u32;
        let mut rep = 0u8;
        self.seg = 0;
        self.addr16 = false;
        let mut fs = false;
        let mut op = self.f8(e)?;
        loop {
            match op {
                0x66 => size = 2,
                0xf2 | 0xf3 => rep = op as u8,
                0x2e | 0x3e | 0x26 | 0x36 => {}
                0x64 => fs = true,
                0x67 => self.addr16 = true,
                // `gs` has no base on 32-bit Windows; a plugin reading through it is not built.
                0x65 => return Err(self.bad(e, start)),
                _ => break,
            }
            op = self.f8(e)?;
        }
        if fs {
            // `movs` reads its source through the segment and has no operand to carry it.
            if matches!(op, 0xa4 | 0xa5) {
                return Err(self.bad(e, start));
            }
            self.seg = self.fs_base;
        }
        // With `0x67` the string and count-register instructions use `si`, `di` and `cx`: not
        // built.
        if self.addr16 && matches!(op, 0xa4..=0xaf | 0xe0..=0xe3) {
            return Err(self.bad(e, start));
        }
        match op {
            // ALU, eight forms for each of the eight operations
            0x00..=0x3f if op & 7 < 6 => {
                let which = (op >> 3) as u8;
                let sz = if op & 1 == 0 { 1 } else { size };
                match op & 7 {
                    0 | 1 => {
                        let m = self.modrm(e)?;
                        let a = self.get(e, m.loc, sz)?;
                        let b = self.reg(m.reg, sz);
                        let r = self.alu(which, a, b, sz);
                        if which != 7 {
                            self.put(e, m.loc, sz, r)?;
                        }
                    }
                    2 | 3 => {
                        let m = self.modrm(e)?;
                        let a = self.reg(m.reg, sz);
                        let b = self.get(e, m.loc, sz)?;
                        let r = self.alu(which, a, b, sz);
                        if which != 7 {
                            self.set_reg(m.reg, sz, r);
                        }
                    }
                    _ => {
                        let imm = if sz == 1 {
                            self.f8(e)?
                        } else if sz == 2 {
                            self.f16(e)?
                        } else {
                            self.f32(e)?
                        };
                        let a = self.reg(EAX, sz);
                        let r = self.alu(which, a, imm, sz);
                        if which != 7 {
                            self.set_reg(EAX, sz, r);
                        }
                    }
                }
            }
            0x40..=0x4f => {
                let n = (op & 7) as usize;
                let dec = op >= 0x48;
                let a = self.reg(n, size);
                let cf = self.cf;
                let r = self.alu(if dec { 5 } else { 0 }, a, 1, size);
                self.cf = cf;
                self.set_reg(n, size, r);
            }
            0x50..=0x57 => {
                let v = self.r[(op & 7) as usize];
                self.push(e, v)?;
            }
            0x58..=0x5f => {
                let v = self.pop(e)?;
                self.r[(op & 7) as usize] = v;
            }
            0x68 => {
                let v = self.f32(e)?;
                self.push(e, v)?;
            }
            0x6a => {
                let v = self.f8(e)? as u8 as i8 as i32 as u32;
                self.push(e, v)?;
            }
            0x69 | 0x6b => {
                let m = self.modrm(e)?;
                let a = sext(self.get(e, m.loc, size)?, size);
                let imm = if op == 0x6b {
                    self.f8(e)? as u8 as i8 as i32
                } else if size == 2 {
                    self.f16(e)? as u16 as i16 as i32
                } else {
                    self.f32(e)? as i32
                };
                let r = i64::from(a) * i64::from(imm);
                let t = sext(r as u32, size);
                self.cf = i64::from(t) != r;
                self.of = self.cf;
                self.set_szp(r as u32, size);
                self.set_reg(m.reg, size, r as u32);
            }
            0x70..=0x7f => {
                let d = self.f8(e)? as u8 as i8 as i32 as u32;
                if self.cond(op & 15) {
                    self.eip = self.eip.wrapping_add(d);
                }
            }
            0x80 | 0x81 | 0x83 => {
                let sz = if op == 0x80 { 1 } else { size };
                let m = self.modrm(e)?;
                let imm = if op == 0x81 {
                    if sz == 2 {
                        self.f16(e)?
                    } else {
                        self.f32(e)?
                    }
                } else {
                    let v = self.f8(e)?;
                    if op == 0x83 {
                        v as u8 as i8 as i32 as u32
                    } else {
                        v
                    }
                };
                let which = m.reg as u8;
                let a = self.get(e, m.loc, sz)?;
                let r = self.alu(which, a, imm, sz);
                if which != 7 {
                    self.put(e, m.loc, sz, r)?;
                }
            }
            0x84 | 0x85 => {
                let sz = if op == 0x84 { 1 } else { size };
                let m = self.modrm(e)?;
                let a = self.get(e, m.loc, sz)?;
                let b = self.reg(m.reg, sz);
                self.alu(4, a, b, sz);
            }
            0x86 | 0x87 => {
                let sz = if op == 0x86 { 1 } else { size };
                let m = self.modrm(e)?;
                let a = self.get(e, m.loc, sz)?;
                let b = self.reg(m.reg, sz);
                self.put(e, m.loc, sz, b)?;
                self.set_reg(m.reg, sz, a);
            }
            0x88..=0x8b => {
                let sz = if op & 1 == 0 { 1 } else { size };
                let m = self.modrm(e)?;
                if op < 0x8a {
                    let v = self.reg(m.reg, sz);
                    self.put(e, m.loc, sz, v)?;
                } else {
                    let v = self.get(e, m.loc, sz)?;
                    self.set_reg(m.reg, sz, v);
                }
            }
            0x8d => {
                let m = self.modrm(e)?;
                match m.loc {
                    // `lea` gives the offset: a segment prefix does not add its base.
                    Loc::Mem(a) => self.set_reg(m.reg, size, a.wrapping_sub(self.seg)),
                    Loc::Reg(_) => return Err(self.bad(e, start)),
                }
            }
            0x8f => {
                let m = self.modrm(e)?;
                let v = self.pop(e)?;
                self.put(e, m.loc, 4, v)?;
            }
            0x90 => {}
            0x91..=0x97 => {
                let n = (op & 7) as usize;
                self.r.swap(EAX, n);
            }
            0x98 => {
                if size == 2 {
                    let v = self.r[EAX] as u8 as i8 as i16 as u16 as u32;
                    self.set_reg(EAX, 2, v);
                } else {
                    self.r[EAX] = self.r[EAX] as u16 as i16 as i32 as u32;
                }
            }
            0x99 => {
                self.r[EDX] = if self.r[EAX] & 0x8000_0000 != 0 {
                    u32::MAX
                } else {
                    0
                };
            }
            0xa4 | 0xa5 | 0xaa | 0xab => {
                let sz = if op & 1 == 0 { 1 } else { size };
                let step = if self.df { 0u32.wrapping_sub(sz) } else { sz };
                loop {
                    if rep != 0 && self.r[ECX] == 0 {
                        break;
                    }
                    if op < 0xa8 {
                        let v = self.rd(e, self.r[ESI], sz)?;
                        self.wr(e, self.r[EDI], sz, v)?;
                        self.r[ESI] = self.r[ESI].wrapping_add(step);
                    } else {
                        self.wr(e, self.r[EDI], sz, self.reg(EAX, sz))?;
                    }
                    self.r[EDI] = self.r[EDI].wrapping_add(step);
                    if rep == 0 {
                        break;
                    }
                    self.r[ECX] -= 1;
                }
            }
            0xa0..=0xa3 => {
                let sz = if op & 1 == 0 { 1 } else { size };
                // With `0x67` the offset is 16 bits (`64 67 a1 18 00`: `mov eax, fs:[0x18]`).
                let off = if self.addr16 {
                    self.f16(e)?
                } else {
                    self.f32(e)?
                };
                let at = off.wrapping_add(self.seg);
                if op < 0xa2 {
                    let v = self.rd(e, at, sz)?;
                    self.set_reg(EAX, sz, v);
                } else {
                    let v = self.reg(EAX, sz);
                    self.wr(e, at, sz, v)?;
                }
            }
            0xa8 | 0xa9 => {
                let sz = if op == 0xa8 { 1 } else { size };
                let imm = if sz == 1 {
                    self.f8(e)?
                } else if sz == 2 {
                    self.f16(e)?
                } else {
                    self.f32(e)?
                };
                let a = self.reg(EAX, sz);
                self.alu(4, a, imm, sz);
            }
            0xb0..=0xb7 => {
                let v = self.f8(e)?;
                self.set_reg((op & 7) as usize, 1, v);
            }
            0xb8..=0xbf => {
                let v = if size == 2 {
                    self.f16(e)?
                } else {
                    self.f32(e)?
                };
                self.set_reg((op & 7) as usize, size, v);
            }
            0xc0 | 0xc1 | 0xd0..=0xd3 => {
                let sz = if op & 1 == 0 { 1 } else { size };
                let m = self.modrm(e)?;
                let count = match op {
                    0xc0 | 0xc1 => self.f8(e)?,
                    0xd0 | 0xd1 => 1,
                    _ => self.r[ECX] & 0xff,
                };
                let v = self.get(e, m.loc, sz)?;
                let r = self.shift(m.reg, v, count, sz)?;
                self.put(e, m.loc, sz, r)?;
            }
            0xc2 => {
                let n = self.f16(e)?;
                self.eip = self.pop(e)?;
                self.r[ESP] = self.r[ESP].wrapping_add(n);
            }
            0xc3 => self.eip = self.pop(e)?,
            0xc6 | 0xc7 => {
                let sz = if op == 0xc6 { 1 } else { size };
                let m = self.modrm(e)?;
                let v = if sz == 1 {
                    self.f8(e)?
                } else if sz == 2 {
                    self.f16(e)?
                } else {
                    self.f32(e)?
                };
                self.put(e, m.loc, sz, v)?;
            }
            0xc9 => {
                self.r[ESP] = self.r[EBP];
                self.r[EBP] = self.pop(e)?;
            }
            0xcc => return Err(self.bad(e, start)),
            0xe8 => {
                let d = self.f32(e)?;
                let ret = self.eip;
                self.push(e, ret)?;
                self.eip = self.eip.wrapping_add(d);
            }
            0xe9 => {
                let d = self.f32(e)?;
                self.eip = self.eip.wrapping_add(d);
            }
            0xeb => {
                let d = self.f8(e)? as u8 as i8 as i32 as u32;
                self.eip = self.eip.wrapping_add(d);
            }
            0xf6 | 0xf7 => {
                let sz = if op == 0xf6 { 1 } else { size };
                let m = self.modrm(e)?;
                match m.reg {
                    0 | 1 => {
                        let imm = if sz == 1 {
                            self.f8(e)?
                        } else if sz == 2 {
                            self.f16(e)?
                        } else {
                            self.f32(e)?
                        };
                        let a = self.get(e, m.loc, sz)?;
                        self.alu(4, a, imm, sz);
                    }
                    2 => {
                        let a = self.get(e, m.loc, sz)?;
                        self.put(e, m.loc, sz, !a)?;
                    }
                    3 => {
                        let a = self.get(e, m.loc, sz)?;
                        let r = self.alu(5, 0, a, sz);
                        self.cf = a & mask(sz) != 0;
                        self.put(e, m.loc, sz, r)?;
                    }
                    4 | 5 if sz == 4 => {
                        let a = self.get(e, m.loc, 4)?;
                        let (lo, hi, over) = if m.reg == 4 {
                            let p = u64::from(self.r[EAX]) * u64::from(a);
                            (p as u32, (p >> 32) as u32, (p >> 32) != 0)
                        } else {
                            let p = i64::from(self.r[EAX] as i32) * i64::from(a as i32);
                            (p as u32, (p >> 32) as u32, p != i64::from(p as i32))
                        };
                        self.r[EAX] = lo;
                        self.r[EDX] = hi;
                        self.cf = over;
                        self.of = over;
                    }
                    6 | 7 if sz == 4 => {
                        let d = self.get(e, m.loc, 4)?;
                        if d == 0 {
                            return Err(Stop::Divide { eip: start });
                        }
                        let n = (u64::from(self.r[EDX]) << 32) | u64::from(self.r[EAX]);
                        if m.reg == 6 {
                            let q = n / u64::from(d);
                            if q > u64::from(u32::MAX) {
                                return Err(Stop::Divide { eip: start });
                            }
                            self.r[EDX] = (n % u64::from(d)) as u32;
                            self.r[EAX] = q as u32;
                        } else {
                            let n = n as i64;
                            let d = i64::from(d as i32);
                            let q = n / d;
                            if q != i64::from(q as i32) {
                                return Err(Stop::Divide { eip: start });
                            }
                            self.r[EDX] = (n % d) as u32;
                            self.r[EAX] = q as u32;
                        }
                    }
                    _ => return Err(self.bad(e, start)),
                }
            }
            0xfc => self.df = false,
            0xfd => self.df = true,
            0xfe | 0xff => {
                let sz = if op == 0xfe { 1 } else { size };
                let m = self.modrm(e)?;
                match m.reg {
                    0 | 1 => {
                        let a = self.get(e, m.loc, sz)?;
                        let cf = self.cf;
                        let r = self.alu(if m.reg == 0 { 0 } else { 5 }, a, 1, sz);
                        self.cf = cf;
                        self.put(e, m.loc, sz, r)?;
                    }
                    2 if op == 0xff => {
                        let t = self.get(e, m.loc, 4)?;
                        let ret = self.eip;
                        self.push(e, ret)?;
                        self.eip = t;
                    }
                    4 if op == 0xff => self.eip = self.get(e, m.loc, 4)?,
                    6 if op == 0xff => {
                        let v = self.get(e, m.loc, 4)?;
                        self.push(e, v)?;
                    }
                    _ => return Err(self.bad(e, start)),
                }
            }
            0x0f => {
                let op2 = self.f8(e)?;
                match op2 {
                    0x1f => {
                        self.modrm(e)?;
                    }
                    0x40..=0x4f => {
                        let m = self.modrm(e)?;
                        let v = self.get(e, m.loc, size)?;
                        if self.cond(op2 & 15) {
                            self.set_reg(m.reg, size, v);
                        }
                    }
                    0x80..=0x8f => {
                        let d = self.f32(e)?;
                        if self.cond(op2 & 15) {
                            self.eip = self.eip.wrapping_add(d);
                        }
                    }
                    0x90..=0x9f => {
                        let m = self.modrm(e)?;
                        let v = u32::from(self.cond(op2 & 15));
                        self.put(e, m.loc, 1, v)?;
                    }
                    0xaf => {
                        let m = self.modrm(e)?;
                        let a = sext(self.reg(m.reg, size), size);
                        let b = sext(self.get(e, m.loc, size)?, size);
                        let r = i64::from(a) * i64::from(b);
                        self.cf = i64::from(sext(r as u32, size)) != r;
                        self.of = self.cf;
                        self.set_szp(r as u32, size);
                        self.set_reg(m.reg, size, r as u32);
                    }
                    0xb6 | 0xb7 => {
                        let sz = if op2 == 0xb6 { 1 } else { 2 };
                        let m = self.modrm(e)?;
                        let v = self.get(e, m.loc, sz)?;
                        self.set_reg(m.reg, size, v);
                    }
                    0xbe | 0xbf => {
                        let sz = if op2 == 0xbe { 1 } else { 2 };
                        let m = self.modrm(e)?;
                        let v = sext(self.get(e, m.loc, sz)?, sz) as u32;
                        self.set_reg(m.reg, size, v);
                    }
                    0xc8..=0xcf => {
                        let n = (op2 & 7) as usize;
                        self.r[n] = self.r[n].swap_bytes();
                    }
                    _ => return Err(self.bad(e, start)),
                }
            }
            _ => return Err(self.bad(e, start)),
        }
        Ok(())
    }

    /// Runs from `self.eip` until it reaches `stop_at` (a return address nothing runs), asking
    /// `bridge` at every address first. `budget` is the most instructions it runs.
    pub fn run(
        &mut self,
        e: &mut Engine,
        bridge: &mut dyn Bridge,
        stop_at: u32,
        budget: u64,
    ) -> R<()> {
        let end = self.steps + budget;
        loop {
            if self.eip == stop_at {
                return Ok(());
            }
            if let Some(done) = bridge.at(e, self, self.eip) {
                done?;
                continue;
            }
            if self.steps >= end {
                return Err(Stop::Budget);
            }
            self.steps += 1;
            let at = self.eip;
            self.step(e).map_err(|s| match s {
                Stop::Fault { addr, write } => Stop::FaultAt {
                    addr,
                    write,
                    eip: at,
                },
                other => other,
            })?;
        }
    }
}
