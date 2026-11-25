// SPDX-License-Identifier: GPL-3.0-only
//! Tests of `GetItemHotkeyIconPath`: its record, the texture path without an override, the empty
//! answer when only the override is asked for, a reference for its base form, and a form with no
//! texture.

use super::*;
use crate::test_support::{call, engine, n, r, set_script_ref};
use fork_catalogue::{covers, ids};

const ICON_PATH: u32 = 0x3d4e;

/// A weapon whose texture part sits at `+0x30` with a path, a reference to it, and a form with no
/// texture part.
const WEAPON: u32 = 0x0400_0000;
const WEAPON_REF: u32 = 0x0400_1000;
const NO_TEXTURE: u32 = 0x0400_2000;
const PATH_TEXT: u32 = 0x0400_3000;
const TEXTURE_PART: u32 = WEAPON + 0x30;

fn setup() -> Engine {
    let mut e = engine();
    e.map(0x0400_0000, 0x4000);
    e.mem.set_u8(WEAPON + 4, 0x28);
    e.mem.set_u8(WEAPON_REF + 4, 0x3a);
    e.mem.set_u32(WEAPON_REF + 0x20, WEAPON);
    e.mem.set_u8(NO_TEXTURE + 4, 0x08);
    e.mem.write(
        PATH_TEXT,
        b"interface\\icons\\pipboyimages\\weapons\\weapons_frag_grenade.dds\0",
    );
    e.mem.set_u32(TEXTURE_PART + TEXTURE_PATH, PATH_TEXT);
    // DYNAMIC_CAST (`0x00EC43FB`): the weapon casts to `TESTexture` at its part, the reference
    // to `TESObjectREFR`; nothing else casts.
    e.register(0x00ec_43fb, |_, a| {
        let (form, to) = (a[0], a[3]);
        let cast = match (form, to) {
            (WEAPON, RTTI_TES_TEXTURE) => TEXTURE_PART,
            (WEAPON_REF, rtti::TES_OBJECT_REFR) => WEAPON_REF,
            _ => 0,
        };
        Ret {
            eax: cast,
            ..Ret::default()
        }
    });
    e
}

fn icon(e: &mut Engine, form: u32, only_override: Option<i32>) -> Vec<u8> {
    set_script_ref(e, 1, form);
    let mut args = vec![r(1)];
    args.extend(only_override.map(n));
    let (ok, id) = call(e, ICON_PATH, 0, &args);
    assert_eq!(ok, 1);
    nvse::vars::string(e, id as u32).expect("a string")
}

/// The record: the name at its opcode, a form and an optional integer, no reference needed, a
/// string result.
#[test]
fn the_record_is_at_its_opcode() {
    let e = engine();
    let record = nvse::table::record_of(&e, ICON_PATH);
    assert_ne!(record, 0);
    assert_eq!(e.mem.cstr(e.mem.u32(record)), b"GetItemHotkeyIconPath");
    let params = e.mem.u32(record + 0x14);
    assert_eq!(
        [e.mem.u32(params + 4), e.mem.u32(params + 8)],
        [ANY_FORM, 0]
    );
    assert_eq!([e.mem.u32(params + 16), e.mem.u32(params + 20)], [INT, 1]);
    assert!(crate::COMMAND_NAMES.contains(&"GetItemHotkeyIconPath"));
}

/// Without an override (none can be set), the item's texture path, as `GetTexturePath` reads
/// it; a reference answers for its base form; with `onlyOverride` 1 the empty string (what the
/// mods prefix with `textures\`); a form without a texture answers empty.
#[test]
fn the_icon_path_is_the_texture_path_without_an_override() {
    covers(&[ids::extenders::showoff::GETITEMHOTKEYICONPATH]);
    let mut e = setup();
    let want = b"interface\\icons\\pipboyimages\\weapons\\weapons_frag_grenade.dds".to_vec();
    assert_eq!(icon(&mut e, WEAPON, None), want);
    assert_eq!(icon(&mut e, WEAPON, Some(0)), want);
    assert_eq!(icon(&mut e, WEAPON_REF, None), want, "the base form's");
    assert_eq!(icon(&mut e, WEAPON, Some(1)), b"", "only the override");
    assert_eq!(icon(&mut e, NO_TEXTURE, None), b"");
    e.mem.set_u32(TEXTURE_PART + TEXTURE_PATH, 0);
    assert_eq!(icon(&mut e, WEAPON, None), b"", "no path text");
}
