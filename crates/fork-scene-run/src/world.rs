// SPDX-License-Identifier: GPL-3.0-only
//! The game's side: the player's placement through `PlayerCharacter::PositionPlayerExterior`
//! (`0093cce0`), the function fast travel and `moveto` end in, which cancels pending exterior
//! loads, loads or creates the cell, and places the player on its land. No console text.

use crate::{Screen, Standing, Target, World};
use engine_core::prelude::*;
use fork_scene::Place;

/// `PlayerCharacter` (the pointer).
const PLAYER: u32 = 0x011d_ea3c;
/// `PlayerCharacter::PositionPlayerExterior`: this, position x y z, rotation x y z (radians), the
/// world space, whether to reset the weather.
const POSITION_PLAYER_EXTERIOR: u32 = 0x0093_cce0;
/// The form lookup by id (`004839c0`).
const LOOKUP_FORM: u32 = 0x0048_39c0;
/// `StartMenu::Close` (`007ce7a0`, cdecl, one byte) and the byte `011d8907` `ConfirmNewGame` sets.
const START_MENU_CLOSE: u32 = 0x007c_e7a0;
const GAME_RUNNING_FLAG: u32 = 0x011d_8907;
/// The calendar object (`011de7b8`, not a pointer): its hour and time scale globals, the global's value at +0x24.
const CALENDAR: u32 = 0x011d_e7b8;
const CALENDAR_HOUR: u32 = 0x0c;
const CALENDAR_TIME_SCALE: u32 = 0x14;
const GLOBAL_VALUE: u32 = 0x24;
/// `Sky::pInstance` (`011ccb78`), its current weather (+0x10) and `Sky::ForceWeather` (`0063d0e0`).
const SKY: u32 = 0x011c_cb78;
const SKY_WEATHER: u32 = 0x10;
const SKY_FORCE_WEATHER: u32 = 0x0063_d0e0;
/// The bytes `ToggleAI` (all AI processing) and `ToggleMenus` flip, and what `ToggleMenus` calls.
const ALL_AI_FLAG: u32 = 0x011f_1220;
const MENUS_FLAG: u32 = 0x0118_c6f0;
const MENUS_CHANGED: u32 = 0x0070_3810;
/// A cell's object list (a `tList` whose first node is inline at +0xac), its worldspace, a
/// reference's form type byte (+4; ACHR `3b`, ACRE `3c`), `NiAVObject::SetAppCulled` and
/// `TESWorldSpace::GetCell(x, y)` (`005875a0`).
const CELL_OBJECTS: u32 = 0xac;
const CELL_WORLD_SPACE: u32 = 0xc0;
const FORM_TYPE: u32 = 0x04;
const FORM_TYPE_ACHR: u8 = 0x3b;
const FORM_TYPE_ACRE: u8 = 0x3c;
const SET_APP_CULLED: u32 = 0x0045_0f90;
const WORLD_SPACE_GET_CELL: u32 = 0x0058_75a0;
/// Cells each way around the player's cell whose actors are hidden.
const GRID: i32 = 3;
/// The reference's position (+0x30) and parent cell (+0x40).
const POSITION: u32 = 0x30;
const PARENT_CELL: u32 = 0x40;

/// The data handler (`011c3f2c`, a pointer) and `DataHandler::GetWorldSpaces` (`00460140`: the list of
/// every world space of the load order, a `tList` whose first node is the pointer's value).
const DATA_HANDLER: u32 = 0x011c_3f2c;
const DATA_HANDLER_WORLD_SPACES: u32 = 0x0046_0140;
/// `TESForm::GetEditorID` (virtual, slot `0x130`): a C string.
const GET_EDITOR_ID: u32 = 0x130;

/// A hex form id in a place's `world` (`0DA726`, `0x000DA726`), or `None` for an editor id.
pub fn hex_form_id(world: &str) -> Option<u32> {
    let digits = world.strip_prefix("0x").unwrap_or(world);
    if digits.len() >= 6 && digits.chars().all(|c| c.is_ascii_hexdigit()) {
        u32::from_str_radix(digits, 16).ok()
    } else {
        None
    }
}

/// The world space named `world` (an editor id of any loaded plugin, such as `WastelandNV` or the
/// Capital Wasteland's `Wasteland`, or a form id in hex): its address.
fn find_world(e: &mut Engine, world: &str) -> Result<u32, String> {
    if let Some(id) = hex_form_id(world) {
        let form = e.call(LOOKUP_FORM, &args![id]).u32();
        return if form != 0 {
            Ok(form)
        } else {
            Err(format!("the game has no world space {id:08X}"))
        };
    }
    let handler = e.global::<u32>(DATA_HANDLER);
    let list = e.call(DATA_HANDLER_WORLD_SPACES, &args![handler]).u32();
    let mut node = list;
    let mut known = Vec::new();
    while node != 0 {
        let form = e.mem.u32(node);
        node = e.mem.u32(node + 4);
        if form == 0 {
            continue;
        }
        let name = e.vcall(form, GET_EDITOR_ID, &[]).u32();
        let name = String::from_utf8_lossy(&e.mem.cstr(name)).into_owned();
        if name.eq_ignore_ascii_case(world) {
            return Ok(form);
        }
        known.push(name);
    }
    known.truncate(12);
    Err(format!(
        "no world space `{world}` in the load order (some that are: {})",
        known.join(", ")
    ))
}

/// `GetFormByEditorID` (`00483a00`, cdecl: a C string): the form the editor id names (the editor ids
/// are kept by the fixes preset's JohnnyGuitar row), 0 for none.
const FORM_BY_EDITOR_ID: u32 = 0x0048_3a00;
/// Placed references (REFR `3a`, ACHR `3b`, ACRE `3c`) have a position; the form type byte is `FORM_TYPE`.
const FORM_TYPE_REFR: u8 = 0x3a;

/// A reference found by name.
struct Found {
    pos: [f64; 3],
    /// The world space of its cell; 0 while its cell is not loaded (the first place, from the menu).
    world: u32,
}

/// The reference `name` (an editor id, or a form id in hex) with its position and world space.
fn find_ref(e: &mut Engine, name: &str) -> Result<Found, String> {
    let form = match hex_form_id(name) {
        Some(id) => e.call(LOOKUP_FORM, &args![id]).u32(),
        None => e.with_stack((name.len() as u32 + 4) & !3, |e, text| {
            for (i, b) in name.bytes().enumerate() {
                e.mem.set_u8(text.addr() + i as u32, b);
            }
            e.mem.set_u8(text.addr() + name.len() as u32, 0);
            e.call(FORM_BY_EDITOR_ID, &args![text]).u32()
        }),
    };
    if form == 0 {
        return Err(format!(
            "no form `{name}` (an editor id of a placed reference, or its form id in hex)"
        ));
    }
    let kind = e.mem.u8(form + FORM_TYPE);
    if !(FORM_TYPE_REFR..=FORM_TYPE_REFR + 2).contains(&kind) {
        return Err(format!(
            "`{name}` is not a placed reference (form type {kind:#x})"
        ));
    }
    let cell = e.mem.u32(form + PARENT_CELL);
    let world = if cell == 0 {
        0
    } else {
        e.mem.u32(cell + CELL_WORLD_SPACE)
    };
    let at = |o: u32| f64::from(e.mem.f32(form + POSITION + o));
    Ok(Found {
        pos: [at(0), at(4), at(8)],
        world,
    })
}

/// A running engine as the runner's world.
pub struct EngineWorld<'a> {
    pub engine: &'a mut Engine,
    pub screen: Screen,
}

impl World for EngineWorld<'_> {
    fn screen(&self) -> Screen {
        self.screen
    }

    fn go_to(&mut self, p: &Place) -> Result<Target, String> {
        let e = &mut *self.engine;
        let player = e.global::<u32>(PLAYER);
        if player == 0 {
            return Err("the game has no player yet".into());
        }
        // A place by reference: stand at the reference plus the offset, in the reference's world, and
        // face the other reference when asked.
        let (world, pos) = match &p.reference {
            None => (find_world(e, &p.world)?, p.pos),
            Some(name) => {
                let r = find_ref(e, name)?;
                let world = if !p.world.is_empty() {
                    find_world(e, &p.world)?
                } else if r.world != 0 {
                    r.world
                } else {
                    return Err(format!(
                        "`{name}` has no world space yet (its cell is not loaded): give the place a `world`"
                    ));
                };
                (
                    world,
                    [
                        r.pos[0] + p.offset[0],
                        r.pos[1] + p.offset[1],
                        r.pos[2] + p.offset[2],
                    ],
                )
            }
        };
        let (mut heading, mut pitch) = (p.heading, p.pitch);
        if let Some(name) = &p.look_at {
            let t = find_ref(e, name)?;
            let (h, pi) = fork_scene::aim(pos, t.pos);
            if !p.heading_given {
                heading = h;
            }
            if !p.pitch_given {
                pitch = pi.clamp(-89.0, 89.0);
            }
        }
        let rad = |d: f64| (d.to_radians()) as f32;
        e.call(
            POSITION_PLAYER_EXTERIOR,
            &args![
                player,
                pos[0] as f32,
                pos[1] as f32,
                pos[2] as f32,
                rad(pitch),
                0.0f32,
                rad(heading),
                world,
                0u32
            ],
        );
        if self.screen == Screen::Menu {
            // The main menu is still the game's menu: close it as a new game's start does
            // (`StartMenu::Close`, then the flag `ConfirmNewGame` sets: the game is running).
            e.call(START_MENU_CLOSE, &args![0u32]);
            e.mem.set_u8(GAME_RUNNING_FLAG, 1);
        }
        Ok(Target {
            pos,
            heading,
            pitch,
        })
    }

    fn top_menu(&mut self) -> u32 {
        // `Interface::GetTopMenuID`: reads the menu stack.
        self.engine.call(0x0070_23c0, &args![]).u32()
    }

    fn freeze(&mut self, hour: f32, weather: u32) {
        let e = &mut *self.engine;
        // The clock: the hour held, the time scale 0 (the calendar's globals, value at +0x24).
        for (slot, want) in [(CALENDAR_HOUR, hour), (CALENDAR_TIME_SCALE, 0.0)] {
            let global = e.mem.u32(CALENDAR + slot);
            if global != 0 && (e.mem.f32(global + GLOBAL_VALUE) - want).abs() > 0.001 {
                e.mem.set_f32(global + GLOBAL_VALUE, want);
            }
        }
        // The weather, forced as an override (held), again after every load that changed it.
        let sky = e.mem.u32(SKY);
        if sky != 0 && e.mem.u32(sky + SKY_WEATHER) != 0 {
            let form = e.call(LOOKUP_FORM, &args![weather]).u32();
            if form != 0 && e.mem.u32(sky + SKY_WEATHER) != form {
                e.call(SKY_FORCE_WEATHER, &args![sky, form, 1u32]);
            }
        }
        // No AI processing (what `tai` toggles) and no interface (what `tm` toggles).
        if e.mem.u8(ALL_AI_FLAG) != 0 {
            e.mem.set_u8(ALL_AI_FLAG, 0);
        }
        if e.mem.u8(MENUS_FLAG) != 0 {
            e.mem.set_u8(MENUS_FLAG, 0);
            e.call(MENUS_CHANGED, &args![0u32]);
        }
    }

    fn hide_actors(&mut self) -> u32 {
        let e = &mut *self.engine;
        let player = e.mem.u32(PLAYER);
        if player == 0 {
            return 0;
        }
        let cell = e.mem.u32(player + PARENT_CELL);
        if cell == 0 {
            return 0;
        }
        let world = e.mem.u32(cell + CELL_WORLD_SPACE);
        let mut cells = vec![cell];
        if world != 0 {
            // The exterior grid around the player: the cells the game keeps loaded.
            let (cx, cy) = (
                (e.mem.f32(player + POSITION) / 4096.0).floor() as i32,
                (e.mem.f32(player + POSITION + 4) / 4096.0).floor() as i32,
            );
            for dx in -GRID..=GRID {
                for dy in -GRID..=GRID {
                    let c = e
                        .call(WORLD_SPACE_GET_CELL, &args![world, cx + dx, cy + dy])
                        .u32();
                    if c != 0 && !cells.contains(&c) {
                        cells.push(c);
                    }
                }
            }
        }
        let mut hidden = 0;
        for cell in cells {
            let mut node = cell + CELL_OBJECTS;
            while node != 0 {
                let refr = e.mem.u32(node);
                node = e.mem.u32(node + 4);
                if refr == 0 || refr == player {
                    continue;
                }
                let kind = e.mem.u8(refr + FORM_TYPE);
                if kind != FORM_TYPE_ACHR && kind != FORM_TYPE_ACRE {
                    continue;
                }
                // Slot 0x1d0: the reference's 3D (0 without one).
                let object = e.vcall(refr, 0x1d0, &args![]).u32();
                if object != 0 {
                    e.call(SET_APP_CULLED, &args![object, 1u32]);
                    hidden += 1;
                }
            }
        }
        hidden
    }

    fn dismiss_message(&mut self) -> bool {
        // 0x3e9: the message menu (xNVSE's kMenuType_Message).
        if self.engine.call(0x0070_23c0, &args![]).u32() != 0x3e9 {
            return false;
        }
        self.engine.call(0x007a_a530, &args![]);
        true
    }

    fn ask_picture(&mut self, path: &std::path::Path) {
        d3d9_wgpu::shot::ask(path.to_path_buf());
    }

    fn player(&self) -> Option<Standing> {
        let e = &*self.engine;
        let player = e.mem.u32(PLAYER);
        if player == 0 || e.mem.u32(player + PARENT_CELL) == 0 {
            return None;
        }
        let at = |o: u32| f64::from(e.mem.f32(player + POSITION + o));
        Some(Standing {
            pos: [at(0), at(4), at(8)],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_world_is_an_editor_id_or_a_form_id_of_six_or_more_hex_digits() {
        assert_eq!(hex_form_id("0x000DA726"), Some(0x000d_a726));
        assert_eq!(hex_form_id("0DA726"), Some(0x000d_a726));
        assert_eq!(hex_form_id("WastelandNV"), None);
        // Editor ids that happen to be hex letters only are still names unless six digits long.
        assert_eq!(hex_form_id("Face"), None);
    }
}
