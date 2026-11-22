// SPDX-License-Identifier: GPL-3.0-only
//! Scene capture (nv-rs#479): go to a named place in a frozen world and write one frame as a
//! picture. This crate is the pure part: the place file and the request the engine reads at start.
//! The engine's side is `fork-scene-run`; both are described in `docs/fork/SCENE.md`.

pub mod diff;
pub mod places;
pub mod png;
pub mod request;
pub mod stage;

pub use places::{Place, PlaceFile};
pub use request::Request;
