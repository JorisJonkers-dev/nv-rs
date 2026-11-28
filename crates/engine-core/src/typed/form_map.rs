//! The forms family's maps (`011c54c0`, the form map; `011c54c8`, the editor id map) and the other
//! chained hash tables that share their bodies (nv-rs#951).

use super::FamilyId;

/// The map family's bit (`engine_typed::forms::map::FORM_MAP`).
pub const FAMILY: FamilyId = FamilyId::new(49);
