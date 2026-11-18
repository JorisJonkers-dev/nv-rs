//! The fork's typed in-process facade of the engine's crates (nv-rs#353).
//!
//! A program that links the engine (the Toolbox's `raul`) depends on this crate instead of on
//! the readers directly: the shapes it relies on live in one place the fork owns, and a change
//! in the crates upstream owns does not reach the program except through here. The crate is not
//! published (`publish = false`); the release tag is its version, and [`API_VERSION`] is the
//! number a consumer checks against the one it was written for.
//!
//! This base part has no external dependency and compiles in seconds. It holds:
//! - the readers, re-exported whole: [`esm`] (plugins and load order), [`fos`] (saves),
//!   [`game_profile`] (nv-rs's own sections of `Fallout.ini` and the play settings) and
//!   [`game_folders`] (finding the game and its saves folders), with [`Found`], [`Saves`] and
//!   [`Scan`] also at the top;
//! - [`save_summary`] and [`summarize_save`]: a save's header, plugins and screenshot as owned
//!   values, so a caller does not hold the borrowed [`fos::Save`];
//! - [`CAPABILITIES`]: the checked-in capabilities file, as text;
//! - [`API_VERSION`].
//!
//! Nothing here writes a file or starts a thread, and nothing opens the game.

use std::fmt;
use std::path::{Path, PathBuf};

pub use esm;
pub use fos;
pub use game_folders;
pub use game_folders::{Found, Saves, Scan};
pub use game_profile;

/// The facade's version: bumped by hand on a change that breaks a caller (a name removed or a
/// shape changed). Additions do not bump it. The release tag and the lock file are the other
/// half of the compatibility check.
pub const API_VERSION: u32 = 1;

/// The capabilities file the release carries (`.fork/capabilities.json`), as text: which
/// commands of the built-in extenders are implemented. The file is the one in this repository
/// at the commit the crate is built from.
pub const CAPABILITIES: &str = include_str!("../../../.fork/capabilities.json");

/// A save's screenshot as the save stores it: `width` by `height` pixels, three bytes each, row
/// by row. The channel order and the row direction are the game's; nothing here converts them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Screenshot {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

/// What a caller shows about a save, as owned values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveSummary {
    pub player_name: String,
    /// The karma title the game's load menu shows.
    pub karma_title: String,
    pub level: u32,
    /// The place the game's load menu shows.
    pub location: String,
    /// Play time as the game writes it, `hhh.mm.ss`.
    pub play_time: String,
    pub save_number: u32,
    /// The game's language setting, for example `ENGLISH`.
    pub language: String,
    /// The plugins the save was made with, in its load order.
    pub plugins: Vec<String>,
    pub screenshot: Screenshot,
}

/// Why a save could not be summarized.
#[derive(Debug)]
#[non_exhaustive]
pub enum SaveError {
    /// The file could not be read.
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The bytes are not a save the reader accepts: where, and what.
    Format(fos::Error),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::Io { path, source } => {
                write!(f, "cannot read {}: {source}", path.display())
            }
            SaveError::Format(e) => write!(f, "not a readable save: {e}"),
        }
    }
}

impl std::error::Error for SaveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SaveError::Io { source, .. } => Some(source),
            SaveError::Format(e) => Some(e),
        }
    }
}

/// Reads the save at `path` and summarizes it. The file is only read.
pub fn save_summary(path: impl AsRef<Path>) -> Result<SaveSummary, SaveError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|source| SaveError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    summarize_save(&bytes).map_err(SaveError::Format)
}

/// Summarizes a save already in memory.
pub fn summarize_save(bytes: &[u8]) -> Result<SaveSummary, fos::Error> {
    let save = fos::Save::parse(bytes)?;
    let h = &save.header;
    // `parse` has checked that the screenshot lies inside the file; the checks here keep a
    // change in the reader from turning into a panic.
    let shot_err = || fos::Error::new(save.screenshot_offset, "screenshot outside the file");
    let len = (h.screenshot_width as usize)
        .checked_mul(h.screenshot_height as usize)
        .and_then(|n| n.checked_mul(3))
        .ok_or_else(shot_err)?;
    let end = save
        .screenshot_offset
        .checked_add(len)
        .ok_or_else(shot_err)?;
    let rgb = bytes
        .get(save.screenshot_offset..end)
        .ok_or_else(shot_err)?
        .to_vec();
    Ok(SaveSummary {
        player_name: h.player_name.clone(),
        karma_title: h.karma_title.clone(),
        level: h.level,
        location: h.location.clone(),
        play_time: h.play_time.clone(),
        save_number: h.save_number,
        language: h.language.clone(),
        plugins: save.plugins.clone(),
        screenshot: Screenshot {
            width: h.screenshot_width,
            height: h.screenshot_height,
            rgb,
        },
    })
}

#[cfg(test)]
mod tests;
