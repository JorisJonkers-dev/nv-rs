// SPDX-License-Identifier: GPL-3.0-only
//! Graphics settings (nv-rs#592, part of #591; the design is ADR-0019,
//! `docs/fork/adr/0019-graphics-modules.md`).
//!
//! The pure part, with no engine and no game data: the schema every graphics module declares
//! ([`schema`]), the per-profile settings file with its layers by place, presets as data and the
//! resolution at a place ([`settings`]), the module trait and the live state the engine keeps between
//! frames ([`module`]), and the schema as JSON for raul ([`dump`]).
//!
//! The file is a small subset of TOML read and written by this crate ([`doc`], [`value`]; no
//! dependency): it is written back byte for byte except where a value was set, so keys another
//! program added, comments and lines this crate cannot read are all kept.

pub mod doc;
pub mod dump;
pub mod module;
pub mod schema;
pub mod settings;
pub mod value;

pub use module::{GraphicsModule, Live, Registry};
pub use schema::{Applies, Choice, Insertion, Kind, ModuleSpec, Owns, Setting, Typed};
pub use settings::{
    resolve, Inputs, Layer, ModuleSettings, Place, PlaceName, Preset, Presets, Resolved,
    SettingsFile, Source, FILE_NAME, LAYER_WORDS, ORIGINAL,
};
pub use value::Value;
