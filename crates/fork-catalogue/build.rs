// SPDX-License-Identifier: GPL-3.0-only
//! Writes the ids module from the catalogue (`.fork/catalogue`), with tally's generator: a copy that
//! `tally check` keeps equal to the pinned tally's (`tally ids --generator .fork/catalogue/tally_ids_gen.rs`).

include!("../../.fork/catalogue/tally_ids_gen.rs");

fn main() {
    tally_ids_gen::build("../../.fork/catalogue");
}
