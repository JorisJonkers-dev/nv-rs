// SPDX-License-Identifier: GPL-3.0-only
//! A 32-bit PE image (a plugin DLL): its headers read, its sections mapped into guest memory at a
//! base the loader chooses, its relocations applied and its imports bound to addresses the host
//! gives. Written from Microsoft's published PE format; nothing is copied from a loader.

use engine_core::prelude::*;

#[derive(Debug, PartialEq, Eq)]
pub enum PeError {
    NotPe,
    Not32Bit,
    Truncated(&'static str),
    NoRelocations {
        /// The base the image declares in its header.
        wanted_base: u32,
        /// Where the loader actually put it in the guest heap.
        allocated_base: u32,
    },
}

impl std::fmt::Display for PeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PeError::NotPe => write!(f, "not a PE file"),
            PeError::Not32Bit => write!(f, "not a 32-bit x86 image"),
            PeError::Truncated(what) => write!(f, "truncated: {what}"),
            PeError::NoRelocations {
                wanted_base,
                allocated_base,
            } => {
                let (sign, magnitude) = if *allocated_base >= *wanted_base {
                    ('+', *allocated_base - *wanted_base)
                } else {
                    ('-', *wanted_base - *allocated_base)
                };
                write!(
                    f,
                    "no relocations: the image was mapped at {allocated_base:#x}, a delta of {sign}{magnitude:#x} from its declared base {wanted_base:#x}; without relocations it can only run at its declared base, which the loader cannot provide because it allocates from the guest heap"
                )
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Section {
    pub name: String,
    pub va: u32,
    pub vsize: u32,
    pub raw: u32,
    pub raw_size: u32,
}

/// One imported function: by name or by ordinal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub dll: String,
    pub name: String,
    /// The RVA of the import address table slot the loader fills.
    pub slot: u32,
}

#[derive(Debug)]
pub struct Pe {
    pub image_base: u32,
    pub size_of_image: u32,
    pub entry: u32,
    pub sections: Vec<Section>,
    pub relocs: Vec<u32>,
    pub imports: Vec<Import>,
    pub exports: Vec<(String, u32)>,
    pub headers_size: u32,
    /// The RVA of the TLS directory (data directory 9), 0 for none.
    pub tls: u32,
}

fn u16_at(b: &[u8], o: usize) -> Result<u32, PeError> {
    b.get(o..o + 2)
        .map(|s| u32::from(u16::from_le_bytes([s[0], s[1]])))
        .ok_or(PeError::Truncated("header"))
}

fn u32_at(b: &[u8], o: usize) -> Result<u32, PeError> {
    b.get(o..o + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or(PeError::Truncated("header"))
}

impl Pe {
    pub fn parse(b: &[u8]) -> Result<Pe, PeError> {
        if b.get(..2) != Some(b"MZ") {
            return Err(PeError::NotPe);
        }
        let pe = u32_at(b, 0x3c)? as usize;
        if b.get(pe..pe + 4) != Some(b"PE\0\0") {
            return Err(PeError::NotPe);
        }
        let machine = u16_at(b, pe + 4)?;
        let nsec = u16_at(b, pe + 6)? as usize;
        let opt_size = u16_at(b, pe + 20)? as usize;
        let opt = pe + 24;
        if machine != 0x14c || u16_at(b, opt)? != 0x10b {
            return Err(PeError::Not32Bit);
        }
        let entry = u32_at(b, opt + 16)?;
        let image_base = u32_at(b, opt + 28)?;
        let size_of_image = u32_at(b, opt + 56)?;
        let headers_size = u32_at(b, opt + 60)?;
        let dd = opt + 96;
        let dir = |i: usize| -> Result<(u32, u32), PeError> {
            Ok((u32_at(b, dd + 8 * i)?, u32_at(b, dd + 8 * i + 4)?))
        };
        let so = opt + opt_size;
        let mut sections = Vec::new();
        for i in 0..nsec {
            let o = so + 40 * i;
            let raw_name = b.get(o..o + 8).unwrap_or(&[]);
            let end = raw_name
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(raw_name.len());
            sections.push(Section {
                name: String::from_utf8_lossy(&raw_name[..end]).into_owned(),
                vsize: u32_at(b, o + 8)?,
                va: u32_at(b, o + 12)?,
                raw_size: u32_at(b, o + 16)?,
                raw: u32_at(b, o + 20)?,
            });
        }
        let mut pe = Pe {
            image_base,
            size_of_image,
            entry,
            sections,
            relocs: Vec::new(),
            imports: Vec::new(),
            exports: Vec::new(),
            headers_size,
            tls: dir(9).map_or(0, |(rva, _)| rva),
        };
        let file_of = |pe: &Pe, rva: u32| -> Option<usize> {
            if rva < pe.headers_size {
                return Some(rva as usize);
            }
            pe.sections
                .iter()
                .find(|s| rva >= s.va && rva < s.va + s.vsize.max(s.raw_size))
                .map(|s| (rva - s.va + s.raw) as usize)
        };
        let cstr = |at: usize| -> String {
            let end = b[at..]
                .iter()
                .position(|&c| c == 0)
                .map_or(b.len(), |p| at + p);
            String::from_utf8_lossy(&b[at..end]).into_owned()
        };
        // base relocations
        let (rr, rs) = dir(5)?;
        if rr != 0 {
            let mut o = file_of(&pe, rr).ok_or(PeError::Truncated("relocations"))?;
            let end = o + rs as usize;
            while o + 8 <= end {
                let page = u32_at(b, o)?;
                let size = u32_at(b, o + 4)? as usize;
                if size < 8 {
                    break;
                }
                for k in 0..(size - 8) / 2 {
                    let entry = u16_at(b, o + 8 + 2 * k)?;
                    if entry >> 12 == 3 {
                        pe.relocs.push(page + (entry & 0xfff));
                    }
                }
                o += size;
            }
        }
        // imports
        let (ir, _) = dir(1)?;
        if ir != 0 {
            let mut o = file_of(&pe, ir).ok_or(PeError::Truncated("imports"))?;
            loop {
                let (oft, name, ft) = (u32_at(b, o)?, u32_at(b, o + 12)?, u32_at(b, o + 16)?);
                if name == 0 {
                    break;
                }
                let dll = cstr(file_of(&pe, name).ok_or(PeError::Truncated("import name"))?)
                    .to_ascii_lowercase();
                let lookup = if oft != 0 { oft } else { ft };
                let mut t = file_of(&pe, lookup).ok_or(PeError::Truncated("import table"))?;
                let mut slot = ft;
                loop {
                    let v = u32_at(b, t)?;
                    if v == 0 {
                        break;
                    }
                    let name = if v & 0x8000_0000 != 0 {
                        format!("#{}", v & 0xffff)
                    } else {
                        cstr(file_of(&pe, v).ok_or(PeError::Truncated("import by name"))? + 2)
                    };
                    pe.imports.push(Import {
                        dll: dll.clone(),
                        name,
                        slot,
                    });
                    t += 4;
                    slot += 4;
                }
                o += 20;
            }
        }
        // exports
        let (er, _) = dir(0)?;
        if er != 0 {
            let o = file_of(&pe, er).ok_or(PeError::Truncated("exports"))?;
            let n = u32_at(b, o + 24)? as usize;
            let funcs =
                file_of(&pe, u32_at(b, o + 28)?).ok_or(PeError::Truncated("export table"))?;
            let names =
                file_of(&pe, u32_at(b, o + 32)?).ok_or(PeError::Truncated("export names"))?;
            let ords =
                file_of(&pe, u32_at(b, o + 36)?).ok_or(PeError::Truncated("export ordinals"))?;
            for i in 0..n {
                let name = cstr(
                    file_of(&pe, u32_at(b, names + 4 * i)?)
                        .ok_or(PeError::Truncated("export name"))?,
                );
                let ord = u16_at(b, ords + 2 * i)? as usize;
                pe.exports.push((name, u32_at(b, funcs + 4 * ord)?));
            }
        }
        Ok(pe)
    }

    pub fn export(&self, name: &str) -> Option<u32> {
        self.exports
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, rva)| *rva)
    }

    /// Maps the image into the guest heap (page aligned), applies the relocations for the base it
    /// got and writes `bind(import)` into each import slot. Returns the base.
    pub fn map(
        &self,
        e: &mut Engine,
        bytes: &[u8],
        mut bind: impl FnMut(&Import) -> u32,
    ) -> Result<u32, PeError> {
        let block = e.mem.alloc(self.size_of_image + 0x1000);
        let base = (block + 0xfff) & !0xfff;
        e.mem.zero(base, self.size_of_image);
        let n = (self.headers_size as usize).min(bytes.len());
        e.mem.write(base, &bytes[..n]);
        for s in &self.sections {
            let len = s.raw_size.min(s.vsize.max(s.raw_size)) as usize;
            let from = s.raw as usize;
            let part = bytes
                .get(from..from + len)
                .ok_or(PeError::Truncated("section"))?;
            e.mem.write(base + s.va, part);
        }
        let delta = base.wrapping_sub(self.image_base);
        if delta != 0 {
            if self.relocs.is_empty() {
                return Err(PeError::NoRelocations {
                    wanted_base: self.image_base,
                    allocated_base: base,
                });
            }
            for &r in &self.relocs {
                let a = base + r;
                let v = e.mem.u32(a);
                e.mem.set_u32(a, v.wrapping_add(delta));
            }
        }
        for import in &self.imports {
            let v = bind(import);
            e.mem.set_u32(base + import.slot, v);
        }
        Ok(base)
    }
}
