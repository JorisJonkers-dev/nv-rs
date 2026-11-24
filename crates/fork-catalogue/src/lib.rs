// SPDX-License-Identifier: GPL-3.0-only
//! The coverage catalogue's ids as constants (docs/fork/CATALOGUE.md). A test says which catalogue
//! items it proves by calling [`covers`] with their full paths:
//!
//! ```
//! use fork_catalogue::{covers, ids};
//! // In a test of the setting that builds ambient occlusion:
//! covers(&[ids::gfx::features::AO]);
//! ```
//!
//! An id that is not in the catalogue does not compile. tally reads these calls (`tally check`,
//! `tally report`); [`covers`] itself does nothing at run time unless `TALLY_COVERS_LOG` names a
//! file. Tests in files upstream owns are tagged from `.fork/catalogue/covers.d/` instead.

include!(concat!(env!("OUT_DIR"), "/tally_ids.rs"));

#[cfg(test)]
mod tests {
    use super::ids;

    #[test]
    fn an_id_is_its_dotted_catalogue_path() {
        assert_eq!(ids::gfx::features::AO.as_str(), "gfx.features.ao");
        assert_eq!(
            ids::extenders::jip_ln::SETLINKEDREFERENCE.as_str(),
            "extenders.jip_ln.setlinkedreference"
        );
        assert_eq!(
            ids::engine::functions::F00401000.as_str(),
            "engine.functions.f00401000"
        );
    }
}
