//! The co-save section of ShowOff xNVSE, carried: its records are read at a load as they are and written
//! back at the next save, in order, so a save brought from a real install does not lose them
//! while the plugin answers "installed" (the row `NV_COSAVE_KEEP` does this for a plugin that is
//! not built in). A new game starts with none. The records are not interpreted.

use crate::State;
use engine_core::prelude::*;
use nvse::cosave::{OutOfRange, Reader};
use nvse::plugins::{read_chunks, write_chunks, Records};

/// The load callback: the section's records, or none when the co-save has no section.
pub fn load(e: &mut Engine, r: &mut Reader) -> Result<(), OutOfRange> {
    // A section cut short keeps no half record: nothing is carried then.
    e.ext::<State>().carried = read_chunks(r).unwrap_or_default();
    Ok(())
}

/// The save callback: what was carried.
pub fn save(e: &mut Engine, out: &mut Records) {
    let carried = e.ext::<State>().carried.clone();
    write_chunks(&carried, out);
}

/// A new game carries nothing.
pub fn new_game(e: &mut Engine) {
    e.ext::<State>().carried.clear();
}

#[cfg(test)]
#[path = "cosave_tests.rs"]
mod tests;
