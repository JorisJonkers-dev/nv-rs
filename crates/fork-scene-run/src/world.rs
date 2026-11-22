// SPDX-License-Identifier: GPL-3.0-only
//! The game's side: the player's placement through `PlayerCharacter::PositionPlayerExterior`
//! (`0093cce0`), the function fast travel and `moveto` end in, which cancels pending exterior
//! loads, loads or creates the cell, and places the player on its land. No console text.

use crate::{Screen, Standing, Target, World};
use engine_core::prelude::*;
use fork_scene::Place;

/// `PlayerCharacter` (the pointer).
const PLAYER: u32 = 0x011d_ea3c;
pub(crate) const PLAYER_POINTER: u32 = PLAYER;
pub(crate) const POSITION_OFFSET: u32 = 0x30;
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
/// The byte `tgm` flips: god mode. A placement drops the player onto the land from a few hundred units, and a frozen world does
/// not heal it: after a score of far moves in one start the player dies and the game goes back to the main menu (the sweep's
/// two failures at the 22nd and the 61st place of a list, both fine alone). God mode is part of the frozen world.
const GOD_MODE: u32 = 0x011e_07ba;
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

/// The `TES` singleton (`011dea10`, a pointer): its current grid cell (`+0x24`, `+0x28`), the grid cell array (`+0x08`),
/// `GridCellArray::Get(x, y)` (`004ba490`: the address of the slot) and `uGridsToLoad:General`'s setting (`011c63cc`, the value
/// at +4).
const TES_SINGLETON: u32 = 0x011d_ea10;
const TES_GRID_X: u32 = 0x24;
const TES_GRID_Y: u32 = 0x28;
const TES_GRID_ARRAY: u32 = 0x08;
const GRID_CELL_ARRAY_GET: u32 = 0x004b_a490;
const GRIDS_TO_LOAD: u32 = 0x011c_63cc;

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
    /// The reference's form.
    form: u32,
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
        form,
        pos: [at(0), at(4), at(8)],
        world,
    })
}

impl EngineWorld<'_> {
    /// Sets the game setting `name` (the game's settings collection, looked up by name) of the type letter `kind` (`f`
    /// or `i`: the first letter of its name) to the word `bits`; `Ok(false)` while the collection does not exist yet.
    fn set_setting(&mut self, name: &str, kind: u8, bits: u32) -> Result<bool, String> {
        let e = &mut *self.engine;
        let collection = e.mem.u32(GAME_SETTINGS);
        if collection == 0 {
            return Ok(false);
        }
        let setting = e.with_stack((name.len() as u32 + 4) & !3, |e, text| {
            for (i, b) in name.bytes().enumerate() {
                e.mem.set_u8(text.addr() + i as u32, b);
            }
            e.mem.set_u8(text.addr() + name.len() as u32, 0);
            e.with_stack(4, |e, out| {
                e.mem.set_u32(out.addr(), 0);
                let found = e
                    .call(NI_TMAP_LOOKUP, &args![collection + 0x10c, text, out])
                    .u32()
                    & 0xff;
                if found != 0 {
                    e.mem.u32(out.addr())
                } else {
                    0
                }
            })
        });
        if setting == 0 {
            return Err(format!("the game has no setting {name}"));
        }
        // A setting's name (+8) starts with its type letter; the value is the word at +4.
        let kind_name = e.mem.u32(setting + 8);
        if kind_name == 0 || e.mem.u8(kind_name) != kind {
            return Err(format!(
                "the game's setting {name} is not a {}",
                if kind == b'f' {
                    "float"
                } else {
                    "whole number"
                }
            ));
        }
        e.mem.set_u32(setting + 4, bits);
        Ok(true)
    }

    /// A door reference's own way: the destination its teleport data names (an interior cell or a world space), the
    /// position and rotation it gives, placed the way the game places a door's traveller.
    fn go_through_door(&mut self, p: &Place, player: u32, name: &str) -> Result<Target, String> {
        let e = &mut *self.engine;
        let door = find_ref(e, name)?;
        let data = e.call(REFERENCE_GET_TELEPORT_DATA, &args![door.form]).u32();
        if data == 0 {
            return Err(format!(
                "`{name}` is not a door that leads anywhere (it has no teleport)"
            ));
        }
        let world = e.call(TELEPORT_GET_WORLD_SPACE, &args![data]).u32();
        let cell = e.call(TELEPORT_GET_CELL, &args![data]).u32();
        if world == 0 && cell == 0 {
            return Err(format!(
                "the teleport of `{name}` names no cell and no world space"
            ));
        }
        let at = |o: u32| f64::from(e.mem.f32(data + o));
        let pos = [at(4), at(8), at(12)];
        let rot = [at(0x10), at(0x14), at(0x18)];
        let heading = if p.heading_given {
            p.heading
        } else {
            rot[2].to_degrees()
        };
        let rad = |d: f64| (d.to_radians()) as f32;
        let request = e.call(OPERATOR_NEW, &args![0x34u32]).u32();
        for off in (0..0x34).step_by(4) {
            e.mem.set_u32(request + off, 0);
        }
        if world != 0 {
            e.mem.set_u32(request, world);
        } else {
            e.mem.set_u32(request + 0x04, cell);
        }
        for (i, v) in pos.iter().enumerate() {
            e.mem.set_f32(request + 0x08 + 4 * i as u32, *v as f32);
        }
        for (i, v) in [rad(p.pitch), 0.0f32, rad(heading)].iter().enumerate() {
            e.mem.set_f32(request + 0x14 + 4 * i as u32, *v);
        }
        if self.screen != Screen::Play {
            return Err(format!(
                "a place by door is sent from the play screen: put a place before it in the file (the door `{name}` is a form of a loaded cell)"
            ));
        }
        e.call(REQUEST_POSITION_PLAYER, &args![player, request]);
        TARGET_CELL.store(cell, std::sync::atomic::Ordering::Relaxed);
        Ok(Target {
            pos,
            heading,
            pitch: p.pitch,
        })
    }

    /// An interior place: the player is put into the cell through the game's own placement into a cell
    /// (`PlayerCharacter::PositionPlayer`, what a door and `coc` end in; from the play screen the request the game queues for
    /// a door, handled by its player update with the character controller's height).
    fn go_to_interior(&mut self, p: &Place, player: u32) -> Result<Target, String> {
        let e = &mut *self.engine;
        if let Some(name) = &p.door {
            return self.go_through_door(p, player, name);
        }
        let cell = find_interior(e, &p.cell)?;
        let (start, start_rot) = cell_start(e, cell);
        let mut pos = if p.at_start {
            [
                start[0] + p.offset[0],
                start[1] + p.offset[1],
                start[2] + p.offset[2],
            ]
        } else {
            p.pos
        };
        if let Some(name) = &p.reference {
            let r = find_ref(e, name).map_err(|m| {
                format!("{m} (a reference of an interior is a form only while its cell is loaded: use the cell's start marker, or x, y, z)")
            })?;
            pos = [
                r.pos[0] + p.offset[0],
                r.pos[1] + p.offset[1],
                r.pos[2] + p.offset[2],
            ];
        }
        let mut heading = if p.heading_given || !p.at_start {
            p.heading
        } else {
            start_rot[2].to_degrees()
        };
        let mut pitch = p.pitch;
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
        if self.screen == Screen::Play {
            let request = e.call(OPERATOR_NEW, &args![0x34u32]).u32();
            for off in (0..0x34).step_by(4) {
                e.mem.set_u32(request + off, 0);
            }
            e.mem.set_u32(request + 0x04, cell);
            for (i, v) in [pos[0] as f32, pos[1] as f32, pos[2] as f32]
                .iter()
                .enumerate()
            {
                e.mem.set_f32(request + 0x08 + 4 * i as u32, *v);
            }
            for (i, v) in [rad(pitch), 0.0f32, rad(heading)].iter().enumerate() {
                e.mem.set_f32(request + 0x14 + 4 * i as u32, *v);
            }
            e.call(REQUEST_POSITION_PLAYER, &args![player, request]);
        } else {
            e.call(
                POSITION_PLAYER,
                &args![
                    player,
                    pos[0] as f32,
                    pos[1] as f32,
                    pos[2] as f32,
                    rad(pitch),
                    0.0f32,
                    rad(heading),
                    cell,
                    0u32
                ],
            );
            accept_position_as_good(e, player);
            // The main menu is still the game's menu: close it as a new game's start does.
            e.call(START_MENU_CLOSE, &args![0u32]);
            e.mem.set_u8(GAME_RUNNING_FLAG, 1);
        }
        TARGET_CELL.store(cell, std::sync::atomic::Ordering::Relaxed);
        Ok(Target {
            pos,
            heading,
            pitch,
        })
    }
}

/// `g_GameSettingCollection` and the game's `NiTMap` lookup (`00853130`: the map, the key, an out pointer).
const GAME_SETTINGS: u32 = 0x011c_8048;
const NI_TMAP_LOOKUP: u32 = 0x0085_3130;

/// `Script::SetPosBase` and `SetAngleBase` (`005c0740`, `005c0b10`: the reference, the axis letter, the value).
const SET_POS_BASE: u32 = 0x005c_0740;
const SET_ANGLE_BASE: u32 = 0x005c_0b10;

/// `PlayerCharacter::ToggleFlyCam` (`00961e30`, what the console's `tfc` calls: the player, whether the world
/// freezes with it) and the flag it flips: the byte at +6 of the object `[011dea0c]` points to. The fly camera's
/// heading and pitch (radians) and position are the player's fields at +0x7e0, +0x7e4 and +0x7e8; every frame
/// `PlayerCharacter::UpdateUFOCamera` places the camera from them (the terrain around the camera follows).
const MAIN_POINTER: u32 = 0x011d_ea0c;
const FLY_CAMERA_FLAG: u32 = 6;
const TOGGLE_FLY_CAM: u32 = 0x0096_1e30;
const UFO_HEADING: u32 = 0x7e0;
const UFO_PITCH: u32 = 0x7e4;
const UFO_POSITION: u32 = 0x7e8;

/// Whether the game's fly camera is on.
fn fly_camera_on(e: &Engine) -> bool {
    let main = e.mem.u32(MAIN_POINTER);
    main != 0 && e.mem.u8(main + FLY_CAMERA_FLAG) != 0
}

/// The tree update's time accumulator (the sway's clock, `BSTreeManager::Update`).
const TREE_TIME_ACCUMULATOR: u32 = 0x011d_5dd4;

/// `operator new` (`00401000`), `PlayerCharacter::RequestPositionPlayer` (`0093be30`: the player, a 0x34-byte request the
/// game frees), the player's character controller (`009306d0`) and the controller's height setter (`00573f20`).
const OPERATOR_NEW: u32 = 0x0040_1000;
const REQUEST_POSITION_PLAYER: u32 = 0x0093_be30;
const GET_CHAR_CONTROLLER_OF_PLAYER: u32 = 0x0093_06d0;
const CONTROLLER_SET_HEIGHT: u32 = 0x0057_3f20;

/// The player's border-region rule (`PlayerCharacter`): outside the playable area (the world's border regions) the game
/// returns the player to `LastKnownGoodPosition` (+0x7a0, three floats) when `bReturnToLastKnownGoodPosition` (+0x79d) is
/// set. A placement outside the border (a lake's water, the far bank of a river) is undone one frame later without this
/// (nv-rs#474, #486): the position the player was placed at becomes the last good one and the return is cleared.
const RETURN_TO_LAST_GOOD: u32 = 0x79d;
const LAST_GOOD_POSITION: u32 = 0x7a0;

fn accept_position_as_good(e: &mut Engine, player: u32) {
    for i in 0..3 {
        let w = e.mem.u32(player + POSITION + 4 * i);
        e.mem.set_u32(player + LAST_GOOD_POSITION + 4 * i, w);
    }
    e.mem.set_u8(player + RETURN_TO_LAST_GOOD, 0);
}

/// `TESDataHandler::GetCellByEditorID` (`00461ae0`: the handler, a C string; interior cells first, then each world's), the
/// cell's interior test (`00425fd0`) and the cell's start placement (`0054cfd0`: the cell, a position and a rotation out,
/// what `coc` stands the player at).
const GET_CELL_BY_EDITOR_ID: u32 = 0x0046_1ae0;
const CELL_IS_INTERIOR: u32 = 0x0042_5fd0;
const CELL_START_PLACEMENT: u32 = 0x0054_cfd0;
/// `PlayerCharacter::PositionPlayer` (`0093c200`): this, position x y z, rotation x y z (radians), the cell, whether to
/// reset the weather. What a door or `coc` ends in for an interior.
const POSITION_PLAYER: u32 = 0x0093_c200;
/// A door reference's teleport data (`00568e50`; 0 for a reference that leads nowhere), its destination's world space
/// (`0043a320`) and cell (`0043a2b0`; the linked door's), and the destination position (+4) and rotation (+0x10).
const REFERENCE_GET_TELEPORT_DATA: u32 = 0x0056_8e50;
const TELEPORT_GET_WORLD_SPACE: u32 = 0x0043_a320;
const TELEPORT_GET_CELL: u32 = 0x0043_a2b0;

/// The interior cell named `name` (an editor id): its address.
fn find_interior(e: &mut Engine, name: &str) -> Result<u32, String> {
    let handler = e.global::<u32>(DATA_HANDLER);
    let cell = e.with_stack((name.len() as u32 + 4) & !3, |e, text| {
        for (i, b) in name.bytes().enumerate() {
            e.mem.set_u8(text.addr() + i as u32, b);
        }
        e.mem.set_u8(text.addr() + name.len() as u32, 0);
        e.call(GET_CELL_BY_EDITOR_ID, &args![handler, text]).u32()
    });
    if cell == 0 {
        return Err(format!(
            "no cell `{name}` in the load order (an editor id of an interior cell)"
        ));
    }
    if e.call(CELL_IS_INTERIOR, &args![cell]).u32() & 0xff == 0 {
        return Err(format!(
            "`{name}` is an exterior cell: a place in a world space takes `world` and x, y, z"
        ));
    }
    Ok(cell)
}

/// The cell's start placement: position and rotation (radians).
fn cell_start(e: &mut Engine, cell: u32) -> ([f64; 3], [f64; 3]) {
    e.with_stack(0x18, |e, words| {
        let (pos, rot) = (words.addr(), words.addr() + 12);
        for i in 0..6 {
            e.mem.set_u32(pos + 4 * i, 0);
        }
        e.call(CELL_START_PLACEMENT, &args![cell, pos, rot]);
        let at = |a: u32| f64::from(e.mem.f32(a));
        (
            [at(pos), at(pos + 4), at(pos + 8)],
            [at(rot), at(rot + 4), at(rot + 8)],
        )
    })
}

/// A running engine as the runner's world.
pub struct EngineWorld<'a> {
    pub engine: &'a mut Engine,
    pub screen: Screen,
}

/// The interior cell the last `go_to` sent the player to (0: an exterior), for the arrival check.
static TARGET_CELL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

impl World for EngineWorld<'_> {
    fn loaded_cells(&mut self) -> Option<String> {
        let e = &mut *self.engine;
        let tes = e.mem.u32(TES_SINGLETON);
        if tes == 0 {
            return None;
        }
        let grids = e.mem.u32(GRIDS_TO_LOAD + 4);
        let array = e.mem.u32(tes + TES_GRID_ARRAY);
        if array == 0 || grids == 0 || grids > 64 {
            return None;
        }
        let origin = (
            e.mem.u32(tes + TES_GRID_X) as i32 - (grids / 2) as i32,
            e.mem.u32(tes + TES_GRID_Y) as i32 - (grids / 2) as i32,
        );
        let mut present = 0;
        for x in 0..grids {
            for y in 0..grids {
                let slot = e.call(GRID_CELL_ARRAY_GET, &args![array, x, y]).u32();
                if slot != 0 && e.mem.u32(slot) != 0 {
                    present += 1;
                }
            }
        }
        let n = grids as i32;
        Some(format!(
            "uGridsToLoad {grids}: cells x {}..{} y {}..{}, {present} of {} hold a cell",
            origin.0,
            origin.0 + n - 1,
            origin.1,
            origin.1 + n - 1,
            grids * grids
        ))
    }

    fn screen(&self) -> Screen {
        self.screen
    }

    fn go_to(&mut self, p: &Place) -> Result<Target, String> {
        crate::view::set(p);
        let e = &mut *self.engine;
        let player = e.global::<u32>(PLAYER);
        if player == 0 {
            return Err("the game has no player yet".into());
        }
        if fly_camera_on(e) {
            // The previous place's free camera: back to the player's own eyes.
            e.call(TOGGLE_FLY_CAM, &args![player, 0u32]);
        }
        TARGET_CELL.store(0, std::sync::atomic::Ordering::Relaxed);
        if !p.cell.is_empty() || p.door.is_some() {
            return self.go_to_interior(p, player);
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
        if p.setpos && self.screen == Screen::Play {
            // The console's `player.setpos x/y/z` and `setangle`: the three coordinates and the heading
            // and pitch set one by one on the player reference (`Script::SetPosBase`, `SetAngleBase`), no
            // cell is asked for (nv-rs#474: the land may then not load).
            for (axis, v) in [(b'X', pos[0]), (b'Y', pos[1]), (b'Z', pos[2])] {
                e.call(SET_POS_BASE, &args![player, u32::from(axis), v as f32]);
                fork_log::debug!(target: "scene", "scene: setpos {} {v:.1}: the player's position is now {:.1} {:.1} {:.1}, parent cell {:#x}", axis as char, e.mem.f32(player + POSITION), e.mem.f32(player + POSITION + 4), e.mem.f32(player + POSITION + 8), e.mem.u32(player + PARENT_CELL));
            }
            e.call(
                SET_ANGLE_BASE,
                &args![player, u32::from(b'Z'), heading as f32],
            );
            e.call(
                SET_ANGLE_BASE,
                &args![player, u32::from(b'X'), pitch as f32],
            );
            return Ok(Target {
                pos,
                heading,
                pitch,
            });
        }
        if p.request && self.screen == Screen::Play {
            // The game's own route for a far move: a queued request the player update carries out (the exterior
            // placement, then the character controller's height, then the callback).
            let request = e.call(OPERATOR_NEW, &args![0x34u32]).u32();
            for off in (0..0x34).step_by(4) {
                e.mem.set_u32(request + off, 0);
            }
            e.mem.set_u32(request, world);
            for (i, v) in [pos[0] as f32, pos[1] as f32, pos[2] as f32]
                .iter()
                .enumerate()
            {
                e.mem.set_f32(request + 0x08 + 4 * i as u32, *v);
            }
            for (i, v) in [rad(pitch), 0.0f32, rad(heading)].iter().enumerate() {
                e.mem.set_f32(request + 0x14 + 4 * i as u32, *v);
            }
            e.call(REQUEST_POSITION_PLAYER, &args![player, request]);
            return Ok(Target {
                pos,
                heading,
                pitch,
            });
        }
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
        accept_position_as_good(e, player);
        if std::env::var_os("NV_SCENE_CONTROLLER_HEIGHT").is_some() {
            // What the game's request handler does after the placement: the character controller gets the target's
            // height (`PlayerCharacter::GetCharacterController` `009306d0`, then `00573f20`).
            let controller = e.call(GET_CHAR_CONTROLLER_OF_PLAYER, &args![player]).u32();
            e.call(CONTROLLER_SET_HEIGHT, &args![controller, pos[2] as f32]);
        }
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

    fn target_cell(&self) -> u32 {
        TARGET_CELL.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn player_cell(&self) -> u32 {
        let e = &*self.engine;
        let player = e.mem.u32(PLAYER);
        if player == 0 {
            0
        } else {
            e.mem.u32(player + PARENT_CELL)
        }
    }

    fn camera(&mut self, p: &Place, standing: [f64; 3]) -> Result<Target, String> {
        let e = &mut *self.engine;
        let player = e.global::<u32>(PLAYER);
        let Some(camera) = p.camera else {
            return Err("the place has no camera".into());
        };
        if player == 0 || e.mem.u32(MAIN_POINTER) == 0 {
            return Err("the game has no player yet".into());
        }
        if !fly_camera_on(e) {
            e.call(TOGGLE_FLY_CAM, &args![player, 0u32]);
        }
        let pos = match camera {
            fork_scene::Camera::At(at) => at,
            fork_scene::Camera::Above(d) => {
                [standing[0] + d[0], standing[1] + d[1], standing[2] + d[2]]
            }
        };
        let (mut heading, mut pitch) = (p.heading, p.pitch);
        let target = match (&p.look, &p.look_at) {
            (Some(point), _) => Some(*point),
            (None, Some(name)) => Some(find_ref(e, name)?.pos),
            _ => None,
        };
        if let Some(t) = target {
            let (h, pi) = fork_scene::aim(pos, t);
            if !p.heading_given {
                heading = h;
            }
            if !p.pitch_given {
                pitch = pi.clamp(-89.0, 89.0);
            }
        }
        crate::view::write_fov(e);
        crate::view::write_fog(e);
        e.mem
            .set_f32(player + UFO_HEADING, heading.to_radians() as f32);
        e.mem.set_f32(player + UFO_PITCH, pitch.to_radians() as f32);
        for (i, v) in pos.iter().enumerate() {
            e.mem
                .set_f32(player + UFO_POSITION + 4 * i as u32, *v as f32);
        }
        Ok(Target {
            pos,
            heading,
            pitch,
        })
    }

    fn move_player(&mut self, pos: [f64; 3]) -> Result<(), String> {
        let e = &mut *self.engine;
        let player = e.global::<u32>(PLAYER);
        if player == 0 {
            return Err("the game has no player yet".into());
        }
        for (axis, v) in [(b'X', pos[0]), (b'Y', pos[1]), (b'Z', pos[2])] {
            e.call(SET_POS_BASE, &args![player, u32::from(axis), v as f32]);
        }
        Ok(())
    }

    fn pin_sway_time(&mut self) {
        // `BSTreeManager::Update` (`006652e0`) adds each frame's time to the float at `011d5dd4` and hands it to the
        // tree library: the sway's clock. Zero at arrival, the same in every run.
        self.engine.mem.set_f32(TREE_TIME_ACCUMULATOR, 0.0);
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
        if e.mem.u8(GOD_MODE) == 0 {
            e.mem.set_u8(GOD_MODE, 1);
        }
        // Not sent back to the last good position (the border regions' rule).
        let player = e.mem.u32(PLAYER);
        if player != 0 && e.mem.u32(player + PARENT_CELL) != 0 {
            accept_position_as_good(e, player);
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

    fn set_float_setting(&mut self, name: &str, value: f32) -> Result<bool, String> {
        self.set_setting(name, b'f', value.to_bits())
    }

    fn set_int_setting(&mut self, name: &str, value: i32) -> Result<bool, String> {
        self.set_setting(name, b'i', value as u32)
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

/// Debug (`NV_SCENE_TRACE=1`): logs every word of the player, its process and its character controller that holds the old
/// position's x (and a y within 16 bytes after it), with the frames still to scan: who keeps the old position, who writes
/// it back (nv-rs#474, #486).
pub(crate) fn scan_for_position(e: &mut Engine, old: [u32; 3], left: u32) {
    let player = e.mem.u32(PLAYER);
    if player == 0 || old[0] == 0 {
        return;
    }
    let controller = e.call(GET_CHAR_CONTROLLER_OF_PLAYER, &args![player]).u32();
    let process = e.mem.u32(player + 0x68);
    for (name, base, len) in [
        ("player", player, 0x900u32),
        ("process", process, 0x600),
        ("controller", controller, 0x600),
    ] {
        if base == 0 {
            continue;
        }
        let mut off = 0;
        while off < len {
            if e.mem.u32(base + off) == old[0] {
                let y = (1..5).any(|k| e.mem.u32(base + off + 4 * k) == old[1]);
                if y {
                    fork_log::debug!(target: "scenetrace", "scan {left}: {name}+{off:#x} holds the old position ({:.1} {:.1} {:.1} at the three words after it)", f32::from_bits(old[0]), f32::from_bits(old[1]), f32::from_bits(e.mem.u32(base + off + 8)));
                }
            }
            off += 4;
        }
    }
    let _ = left;
}

/// Debug: the player's `LastKnownGoodPosition` and whether the game will return the player to it.
pub(crate) fn last_good_position(e: &Engine) -> Option<(f32, f32, f32, u8)> {
    let player = e.mem.u32(PLAYER);
    (player != 0).then(|| {
        (
            e.mem.f32(player + LAST_GOOD_POSITION),
            e.mem.f32(player + LAST_GOOD_POSITION + 4),
            e.mem.f32(player + LAST_GOOD_POSITION + 8),
            e.mem.u8(player + RETURN_TO_LAST_GOOD),
        )
    })
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
