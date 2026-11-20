//! The co-save section of JohnnyGuitar NVSE: the extra miscellaneous statistics are written in a record of
//! nv-rs's own ([`STATS_KIND`]), and every other record is carried: read at a load as it is and
//! written back at the next save, in order, so a save brought from a real install does not lose them
//! while the plugin answers "installed" (the row `NV_COSAVE_KEEP` does this for a plugin that is
//! not built in). A new game starts with none. The carried records are not interpreted.
//!
//! The statistics record (type `NVXS`, version 1): a count (u32), then for each statistic the length of its
//! name (u32), the name's bytes and its value (i32), little-endian, in the order of the names. The
//! record type is not one the real plugin is known to use (its record formats are in no public
//! document read), so a section of a real install is carried untouched and cannot collide with it;
//! a record of this type that is cut short or inconsistent is dropped and the statistics start at 0.

use crate::State;
use engine_core::prelude::*;
use nvse::cosave::{tag, Chunk, OutOfRange, Reader};
use nvse::plugins::{read_chunks, write_chunks, Records};

/// The record type of the extra statistics (`'NVXS'`).
pub const STATS_KIND: u32 = tag(b"NVXS");
/// Its version.
pub const STATS_VERSION: u32 = 1;

/// The statistics as a record's bytes.
pub fn encode_stats(stats: &std::collections::BTreeMap<Vec<u8>, i32>) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(stats.len() as u32).to_le_bytes());
    for (name, value) in stats {
        out.extend_from_slice(&(name.len() as u32).to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

/// A record's bytes as statistics; `None` for bytes that are not a whole record of this format.
pub fn decode_stats(bytes: &[u8]) -> Option<std::collections::BTreeMap<Vec<u8>, i32>> {
    fn word(bytes: &[u8], at: &mut usize) -> Option<[u8; 4]> {
        let w = bytes.get(*at..at.checked_add(4)?)?.try_into().ok()?;
        *at += 4;
        Some(w)
    }
    let mut at = 0;
    let count = u32::from_le_bytes(word(bytes, &mut at)?);
    let mut stats = std::collections::BTreeMap::new();
    for _ in 0..count {
        let len = u32::from_le_bytes(word(bytes, &mut at)?) as usize;
        let name = bytes.get(at..at.checked_add(len)?)?.to_vec();
        at += len;
        let value = i32::from_le_bytes(word(bytes, &mut at)?);
        stats.insert(name, value);
    }
    (at == bytes.len()).then_some(stats)
}

/// The load callback: the section's records, or none when the co-save has no section. The
/// statistics record is read; the others are carried.
pub fn load(e: &mut Engine, r: &mut Reader) -> Result<(), OutOfRange> {
    // A section cut short keeps no half record: nothing is carried then.
    let chunks = read_chunks(r).unwrap_or_default();
    let (ours, carried): (Vec<Chunk>, Vec<Chunk>) = chunks
        .into_iter()
        .partition(|c| c.kind == STATS_KIND && c.version == STATS_VERSION);
    let state = e.ext::<State>();
    state.carried = carried;
    // The statistics of the game played before this load are never kept: a save without the
    // record starts at 0.
    state.stats = ours
        .first()
        .and_then(|c| decode_stats(&c.data))
        .unwrap_or_default();
    Ok(())
}

/// The save callback: what was carried, then the statistics when there are any.
pub fn save(e: &mut Engine, out: &mut Records) {
    let state = e.ext::<State>();
    write_chunks(&state.carried, out);
    if !state.stats.is_empty() {
        out.record(STATS_KIND, STATS_VERSION, &encode_stats(&state.stats));
    }
}

/// A new game carries nothing and has no extra statistics.
pub fn new_game(e: &mut Engine) {
    let state = e.ext::<State>();
    state.carried.clear();
    state.stats.clear();
}

#[cfg(test)]
#[path = "cosave_tests.rs"]
mod tests;
