// SPDX-License-Identifier: GPL-3.0-only
//! Tests of `DisableAlt` through its record, with stand-ins for the game's functions it calls:
//! a reference with an enable parent is queued as pending-disabled (the game's `Disable` would
//! only warn), the fade path, a reference already disabled, and the event list flag.

use super::*;
use crate::test_support::{call, engine, n, LIST};
use fork_catalogue::{covers, ids};

const DISABLE_ALT: u32 = 0x3ce3;

const REFR: u32 = 0x0400_0000;
const DISABLED_REFR: u32 = 0x0400_0400;
const BASE_FORM: u32 = 0x0400_0800;
const VTABLE: u32 = 0x0400_1000;
const GET_PROCESS: u32 = 0x0400_1800;
const PROCESS: u32 = 0x0400_2000;
const PROCESS_VTABLE: u32 = 0x0400_2400;
const GET_OBJECT: u32 = 0x0400_2800;
const OBJECT: u32 = 0x0400_3000;
/// Where the stand-ins write what they were asked: the pending reference and its fade flag, the
/// process mode, the fade flag, the enable parent asked.
const PENDING: u32 = 0x0400_4000;
const PENDING_FADE: u32 = 0x0400_4004;
const MODE: u32 = 0x0400_4008;
const FADE_FLAG: u32 = 0x0400_400c;
const PARENT_ASKED: u32 = 0x0400_4010;

fn setup() -> Engine {
    let mut e = engine();
    e.map(0x0400_0000, 0x5000);
    e.mem.set_u32(DISABLED_REFR + 8, 0x800);
    for refr in [REFR, DISABLED_REFR] {
        e.mem.set_u32(refr, VTABLE);
    }
    e.mem.set_u32(VTABLE + REFR_GET_PROCESS, GET_PROCESS);
    e.mem.set_u32(PROCESS, PROCESS_VTABLE);
    e.mem
        .set_u32(PROCESS_VTABLE + PROCESS_GET_OBJECT, GET_OBJECT);
    e.register(0x00ec_43fb, |_, a| Ret {
        eax: if matches!(a[0], REFR | DISABLED_REFR) && a[3] == rtti::TES_OBJECT_REFR {
            a[0]
        } else {
            0
        },
        ..Ret::default()
    });
    e.register(GET_PROCESS, |_, _| Ret {
        eax: PROCESS,
        ..Ret::default()
    });
    e.register(GET_OBJECT, |_, _| Ret {
        eax: OBJECT,
        ..Ret::default()
    });
    e.register(SET_PROCESS_MODE, |e, a| {
        e.mem.set_u32(MODE, a[1]);
        Ret::default()
    });
    e.register(HAS_FADE_FLAG, |e, _| Ret {
        eax: e.mem.u32(FADE_FLAG),
        ..Ret::default()
    });
    e.register(SET_FADE_FLAG, |e, a| {
        e.mem.set_u32(FADE_FLAG, a[1]);
        Ret::default()
    });
    e.register(ADD_PENDING_DISABLED, |e, a| {
        e.mem.set_u32(PENDING, a[0]);
        e.mem.set_u32(PENDING_FADE, a[1]);
        Ret::default()
    });
    // The enable parent: the reference has one; `DisableAlt` must not care.
    e.register(0x0056_a9f0, |e, _| {
        e.mem.set_u32(PARENT_ASKED, 1);
        Ret {
            eax: 0x0400_0c00,
            ..Ret::default()
        }
    });
    e
}

#[test]
fn the_record_is_at_its_opcode() {
    let e = engine();
    let record = nvse::table::record_of(&e, DISABLE_ALT);
    assert_ne!(record, 0);
    assert_eq!(
        nvse::table::guest_text(&e, e.mem.u32(record)),
        b"DisableAlt"
    );
    assert_eq!(
        nvse::table::guest_text(&e, e.mem.u32(record + 4)),
        b"SnigOff"
    );
    assert!(crate::COMMAND_NAMES.contains(&"DisableAlt"));
}

/// Without a fade: the reference (which has an enable parent) is queued as pending-disabled,
/// no fade is set up, and the script's event list gets its disabled flag.
#[test]
fn a_reference_with_an_enable_parent_is_disabled() {
    covers(&[ids::extenders::showoff::DISABLEALT]);
    let mut e = setup();
    assert_eq!(call(&mut e, DISABLE_ALT, REFR, &[]).0, 1);
    assert_eq!(e.mem.u32(PENDING), REFR);
    assert_eq!(e.mem.u32(PENDING_FADE), 0);
    assert_eq!(e.mem.u32(MODE), 0);
    assert_eq!(e.mem.u32(PARENT_ASKED), 0, "the enable parent is not asked");
    assert_eq!(e.mem.u8(LIST + 4) & 1, 1);
}

/// With a fade: the process gets mode 9, the fade flag is set, and the pending entry fades.
#[test]
fn a_fade_sets_up_the_fade_first() {
    covers(&[ids::extenders::showoff::DISABLEALT]);
    let mut e = setup();
    call(&mut e, DISABLE_ALT, REFR, &[n(1)]);
    assert_eq!(e.mem.u32(MODE), 9);
    assert_eq!(e.mem.u32(FADE_FLAG), 1);
    assert_eq!(e.mem.u32(PENDING), REFR);
    assert_eq!(e.mem.u32(PENDING_FADE), 1);
}

/// A reference already disabled, a base form and no reference queue nothing.
#[test]
fn what_is_not_an_enabled_reference_is_left_alone() {
    covers(&[ids::extenders::showoff::DISABLEALT]);
    let mut e = setup();
    for this in [DISABLED_REFR, BASE_FORM, 0] {
        call(&mut e, DISABLE_ALT, this, &[n(1)]);
        assert_eq!(e.mem.u32(PENDING), 0, "{this:08x}");
        assert_eq!(e.mem.u32(MODE), 0, "{this:08x}");
    }
}
