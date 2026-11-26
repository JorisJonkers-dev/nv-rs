// SPDX-License-Identifier: GPL-3.0-only
//! Controls for the differential harness's optional reference peer (JorisJonkers-dev/nv-rs#924):
//! with the translation as the peer, a byte-matching version must pass and a one-token change must
//! fail with its address and both bytes named. No game data or exe bytes are used.

use engine_core::prelude::*;
use engine_core::typed::diff::{Branch, Case, Diff, Found, Input, ReferencePeer};

engine_core::assemble!();

const BLOCK: u32 = 0x0200_1000;

fn stores(e: &mut Engine, block: u32, value: u32) {
    e.mem.set_u32(block + 8, value);
}

fn stores_again(e: &mut Engine, block: u32, value: u32) {
    e.mem.set_u32(block + 8, value);
}

fn stores_a_wrong_byte(e: &mut Engine, block: u32, value: u32) {
    e.mem.set_u32(block + 8, value);
    e.mem.set_u8(block + 9, e.mem.u8(block + 9) ^ 0x5a);
}

/// The translation is the only peer available without the decomp backend: it runs on the same
/// engine the harness set up, so its outgoing calls reach the same doubles by address.
struct TranslationPeer;

impl ReferencePeer for TranslationPeer {
    fn run(&self, e: &mut Engine, addr: u32, args: &[u32]) -> Ret {
        e.call(addr, args)
    }
}

fn setup(e: &mut Engine, case: &Case) -> Vec<u32> {
    e.map(BLOCK, 0x40);
    e.mem.set_u32(BLOCK + 8, 0xaaaa_aaaa);
    vec![BLOCK, case.word(0)]
}

#[test]
fn a_byte_matching_version_passes_with_the_translation_as_peer() {
    let report = Diff::new(entry!(0x00ef_1000, stores_again(u32, u32)))
        .translation(entry!(0x00ef_1000, stores(u32, u32)).1)
        .reference_peer(TranslationPeer)
        .inputs(&[Input::Of(&[0, 0x1234])])
        .setup(setup)
        .run();
    assert_eq!(report.reference_runs, report.runs, "{report}");
    report.assert_same();
}

#[test]
fn a_planted_one_token_bug_fails_and_names_the_bytes() {
    let report = Diff::new(entry!(0x00ef_1000, stores_a_wrong_byte(u32, u32)))
        .translation(entry!(0x00ef_1000, stores(u32, u32)).1)
        .reference_peer(TranslationPeer)
        .inputs(&[Input::Of(&[0x1234])])
        .setup(setup)
        .run();
    assert!(!report.is_same(), "{report}");
    let difference = report
        .first
        .iter()
        .find(|d| d.branch == Branch::Reference && matches!(d.what, Found::Memory { .. }))
        .unwrap_or_else(|| panic!("the reference run did not name the written bytes: {report}"));
    let Found::Memory {
        addr,
        translation,
        typed,
    } = &difference.what
    else {
        panic!("{report}");
    };
    assert_eq!(*addr, BLOCK + 9);
    assert_eq!(&translation[..], &[0x12]);
    assert_eq!(&typed[..], &[0x48]);
}

#[test]
fn without_a_peer_the_harness_runs_as_before() {
    let report = Diff::new(entry!(0x00ef_1000, stores_again(u32, u32)))
        .translation(entry!(0x00ef_1000, stores(u32, u32)).1)
        .inputs(&[Input::Of(&[0x1234])])
        .setup(setup)
        .run();
    assert_eq!(report.reference_runs, 0, "{report}");
    report.assert_same();
}
