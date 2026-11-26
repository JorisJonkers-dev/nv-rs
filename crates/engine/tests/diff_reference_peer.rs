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

const HELPER: u32 = 0x00ef_2000;
const PLAIN: u32 = 0x00ef_3000;

fn plain_body(e: &mut Engine) {
    e.mem.set_u32(BLOCK + 8, 0x1234_5678);
}

/// The peer runs the translation, which calls an ignored helper by address and reaches a plain
/// callee with a plain Rust call that isn't logged. The reference comparison has to apply the same
/// call filters as the by-address one, or it reports calls that one rightly ignores.
fn calls_helper_and_plain(e: &mut Engine) {
    e.call(HELPER, &[1]);
    plain_body(e);
}

fn calls_plain_by_address(e: &mut Engine) {
    e.call(PLAIN, &[1]);
}

fn setup_with_callees(e: &mut Engine, case: &Case) -> Vec<u32> {
    let words = setup(e, case);
    e.register(HELPER, |_, _| Ret::default());
    e.register(PLAIN, |e, _| {
        plain_body(e);
        Ret::default()
    });
    words
}

#[test]
fn a_peer_uses_the_call_filters_of_the_by_address_run() {
    let report = Diff::new(entry!(0x00ef_1000, calls_plain_by_address()))
        .translation(entry!(0x00ef_1000, calls_helper_and_plain()).1)
        .reference_peer(TranslationPeer)
        .ignore_calls(&[HELPER])
        .translation_calls_plainly(&[PLAIN])
        .inputs(&[Input::Of(&[1])])
        .setup(setup_with_callees)
        .run();
    assert!(report.is_same(), "{report}");
}
