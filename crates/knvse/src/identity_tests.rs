// SPDX-License-Identifier: GPL-3.0-only
//! kNVSE's identity: what the plugin list answers with the flag on and off.

use super::*;
use engine_core::prelude::Engine;
use nvse::plugins::{built_ins, listed};

fn engine() -> Engine {
    let mut e = Engine::new();
    e.map(0x0118_e000, 0x9000);
    nvse::table::install(&mut e);
    e
}

/// The flag is off (the default): nothing is registered and the plugin is not listed, so the
/// game is as it was.
#[test]
fn off_registers_nothing() {
    let mut e = engine();
    crate::install(&mut e);
    assert!(built_ins(&e).is_empty());
    assert_eq!(listed(&e, NAME.as_bytes()), None);
    assert_eq!(
        nvse::table::record_of(&e, crate::commands::SET_ACTOR_ANIMATION_PATH),
        0
    );
}

/// On: the plugin answers by its name, without case, with its version and base.
#[test]
fn on_answers_by_name() {
    let mut e = engine();
    crate::install_as(&mut e, true);
    assert_eq!(listed(&e, NAME.as_bytes()), Some((NAME, VERSION_INT)));
    assert_eq!(listed(&e, b"KNVSE"), Some((NAME, VERSION_INT)));
    let list = built_ins(&e);
    assert_eq!(list.len(), 1);
    assert_eq!((list[0].file, list[0].opcode_base), (DLL, OPCODE_BASE));
}
