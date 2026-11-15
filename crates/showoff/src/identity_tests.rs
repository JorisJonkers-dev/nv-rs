//! ShowOff xNVSE's identity: what the plugin list answers with the flag on and off.

use super::*;
use engine_core::prelude::Engine;
use nvse::plugins::{built_ins, listed};

fn engine() -> Engine {
    let mut e = Engine::new();
    e.map(0x0118_e000, 0x9000);
    nvse::table::install(&mut e);
    e
}

/// The flag is off: nothing is registered, the plugin is not listed and the co-save has no
/// section of it, so the game is as it was.
#[test]
fn off_registers_nothing() {
    let mut e = engine();
    crate::install(&mut e);
    assert!(built_ins(&e).is_empty());
    assert_eq!(listed(&e, NAME.as_bytes()), None);
}

/// On: the plugin answers by its name, without case, with its version and base.
#[test]
fn on_answers_by_name() {
    let mut e = engine();
    crate::install_as(&mut e, true);
    assert_eq!(listed(&e, NAME.as_bytes()), Some((NAME, VERSION_INT)));
    assert_eq!(
        listed(&e, NAME.to_ascii_uppercase().as_bytes()),
        Some((NAME, VERSION_INT))
    );
    assert_eq!(listed(&e, b"nvse"), None);
    let list = built_ins(&e);
    let b = &list[0];
    assert_eq!((b.file, b.opcode_base), (DLL, OPCODE_BASE));
}

/// A second install does nothing (the base would be refused twice).
#[test]
fn installs_once() {
    let mut e = engine();
    crate::install_as(&mut e, true);
    crate::install_as(&mut e, true);
    assert_eq!(built_ins(&e).len(), 1);
}
