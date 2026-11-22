// SPDX-License-Identifier: GPL-3.0-only
//! The game's own code, as a plugin sees it. The engine runs the game as translated Rust, so the
//! executable's code bytes are not in guest memory (and Steam's are encrypted on disk): a plugin
//! that patches the game (a hook, a changed constant, a `jmp` into its own code) writes to
//! addresses that are not mapped. Those writes are kept here, per plugin, in the order made: they
//! are the plugin's patch-site table. A write is never applied (nothing runs from these bytes);
//! a read gives back what the plugin wrote there itself, else zero, and is counted.
//!
//! What the patches meant is the engine's to supply where it matters (the fork's own fixes and
//! the built-in extenders); the table says which sites a plugin touches and whether a site is a
//! function's entry, which the engine can wrap, or the inside of one, which it cannot.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// The addresses of the executable's image (code and data) that are not guest memory here.
pub const GAME_IMAGE: std::ops::Range<u32> = 0x0040_0000..0x0140_9000;

/// One run of bytes a plugin wrote into the game's code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Write {
    /// The index of the plugin (in [`crate::plugins_loaded`]) that wrote it.
    pub plugin: usize,
    pub addr: u32,
    pub bytes: Vec<u8>,
}

#[derive(Default, Debug)]
pub struct Shadow {
    bytes: BTreeMap<u32, u8>,
    /// The plugin whose code runs (the writes are its).
    pub who: usize,
    pub writes: Vec<Write>,
    /// Addresses read that no plugin had written: (plugin, address).
    pub reads: BTreeSet<(usize, u32)>,
    /// The executable's own code bytes, when the user supplied an unpacked copy: (address, bytes).
    pub image: Vec<(u32, Vec<u8>)>,
}

pub type Shared = Rc<RefCell<Shadow>>;

impl Shadow {
    pub fn write(&mut self, addr: u32, data: &[u8]) {
        for (i, &b) in data.iter().enumerate() {
            self.bytes.insert(addr.wrapping_add(i as u32), b);
        }
        let who = self.who;
        if let Some(last) = self.writes.last_mut() {
            if last.plugin == who && last.addr.wrapping_add(last.bytes.len() as u32) == addr {
                last.bytes.extend_from_slice(data);
                return;
            }
        }
        self.writes.push(Write {
            plugin: who,
            addr,
            bytes: data.to_vec(),
        });
    }

    pub fn read(&mut self, addr: u32, out: &mut [u8]) {
        for (i, o) in out.iter_mut().enumerate() {
            let a = addr.wrapping_add(i as u32);
            let original = self
                .image
                .iter()
                .find(|(at, d)| a >= *at && ((a - *at) as usize) < d.len())
                .map(|(at, d)| d[(a - *at) as usize]);
            match self.bytes.get(&a).copied().or(original) {
                Some(b) => *o = b,
                None => {
                    *o = 0;
                    self.reads.insert((self.who, a));
                }
            }
        }
    }
}

/// Whether `addr..addr + len` lies in the executable's image.
pub fn in_image(addr: u32, len: u32) -> bool {
    GAME_IMAGE.contains(&addr) && GAME_IMAGE.contains(&addr.saturating_add(len.max(1) - 1))
}

/// One patch site of a plugin, for the table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    pub plugin: String,
    pub addr: u32,
    pub bytes: Vec<u8>,
    /// The site is the entry of a function the engine has: it can be wrapped. Otherwise it is
    /// inside a function (a changed constant or branch), which has no seam.
    pub function_entry: bool,
}

impl Site {
    pub fn describe(&self) -> String {
        let hex: Vec<String> = self
            .bytes
            .iter()
            .take(16)
            .map(|b| format!("{b:02x}"))
            .collect();
        format!(
            "{} wrote {:#010x}: {}{} ({})",
            self.plugin,
            self.addr,
            hex.join(" "),
            if self.bytes.len() > 16 { " ..." } else { "" },
            if self.function_entry {
                "function entry"
            } else {
                "inside a function"
            }
        )
    }
}
