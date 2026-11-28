// SPDX-License-Identifier: GPL-3.0-only
//! The differential test of the `s36e3734a7b8ae1e9` family's five one-word getters against their
//! translations: `004b7210` (the platform swap word), `0043c4b0` (the default texture setting),
//! `008781f0` (the memory limit), `00453a70` (`BSAudio::QInstance`) and `004537b0` (the task queue
//! object). The translations live in three part crates, so the suite is here, where the engine
//! facade assembles them all.
//!
//! Each case maps the exe's data page and writes one word there before the call, so the getter
//! reads a value that varies by case rather than the zero of an unmapped word; the setup is the
//! same on both sides. The getters make no calls and write no memory, so no callee needs standing
//! in and the leaves family is named only as the other family whose words the run could reach.

use engine as _;
use engine_core::prelude::*;
use engine_core::typed::diff::{Case, Diff, Input};
use engine_core::typed::leaf;
use engine_typed::s36e3734a7b8ae1e9::{self, globals};

/// The words written to the global under test: zero, small values, both signs and the bit patterns
/// a pointer or a flag might hold.
const WORDS: &[u32] = &[
    0,
    1,
    2,
    0x7f,
    0x100,
    0x5a5a_5a5a,
    0x7fff_ffff,
    0x8000_0000,
    0xffff_ffff,
];

/// Maps the page holding `at` and stores the case's word there, identically on both sides. The
/// call's own words are empty: these getters take no arguments.
fn global(e: &mut Engine, case: &Case, at: u32) {
    e.map(at & !0xfff, 0x1000);
    e.mem.set_u32(at, case.word(0));
}

/// `004b7210`: the platform swap word at `011d8a80`.
#[test]
fn platform_swap_word() {
    Diff::new(entry!(0x004b_7210, globals::platform_swap_word() -> u32))
        .family(&s36e3734a7b8ae1e9::S36E3734A7B8AE1E9)
        .family(&leaf::LEAVES)
        .inputs(&[Input::Of(WORDS)])
        .setup(|e, case| {
            global(e, case, 0x011d_8a80);
            vec![]
        })
        .run()
        .assert_same();
}

/// `0043c4b0`: the default texture setting at `011f4748`.
#[test]
fn default_texture_setting() {
    Diff::new(entry!(0x0043_c4b0, globals::default_texture_setting() -> u32))
        .family(&s36e3734a7b8ae1e9::S36E3734A7B8AE1E9)
        .family(&leaf::LEAVES)
        .inputs(&[Input::Of(WORDS)])
        .setup(|e, case| {
            global(e, case, 0x011f_4748);
            vec![]
        })
        .run()
        .assert_same();
}

/// `008781f0`: the memory manager's global limit word at `011a95fc`.
#[test]
fn limit_word() {
    Diff::new(entry!(0x0087_81f0, globals::limit_word() -> u32))
        .family(&s36e3734a7b8ae1e9::S36E3734A7B8AE1E9)
        .family(&leaf::LEAVES)
        .inputs(&[Input::Of(WORDS)])
        .setup(|e, case| {
            global(e, case, 0x011a_95fc);
            vec![]
        })
        .run()
        .assert_same();
}

/// `00453a70`: `BSAudio::QInstance`, the audio instance at `011f6d98`.
#[test]
fn bs_audio_q_instance() {
    Diff::new(entry!(0x0045_3a70, globals::bs_audio_q_instance() -> u32))
        .family(&s36e3734a7b8ae1e9::S36E3734A7B8AE1E9)
        .family(&leaf::LEAVES)
        .inputs(&[Input::Of(WORDS)])
        .setup(|e, case| {
            global(e, case, 0x011f_6d98);
            vec![]
        })
        .run()
        .assert_same();
}

/// `004537b0`: the task queue object at `011df1a8`.
#[test]
fn task_queue_object() {
    Diff::new(entry!(0x0045_37b0, globals::task_queue_object() -> u32))
        .family(&s36e3734a7b8ae1e9::S36E3734A7B8AE1E9)
        .family(&leaf::LEAVES)
        .inputs(&[Input::Of(WORDS)])
        .setup(|e, case| {
            global(e, case, 0x011d_f1a8);
            vec![]
        })
        .run()
        .assert_same();
}
