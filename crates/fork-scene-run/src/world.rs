// SPDX-License-Identifier: GPL-3.0-only
//! The game's side: the player's placement through `PlayerCharacter::PositionPlayerExterior`
//! (`0093cce0`), the function fast travel and `moveto` end in, which cancels pending exterior
//! loads, loads or creates the cell, and places the player on its land. No console text.

use crate::{Screen, Standing, World};
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
/// The reference's position (+0x30) and parent cell (+0x40).
const POSITION: u32 = 0x30;
const PARENT_CELL: u32 = 0x40;

/// The world spaces whose editor id this build knows; any other is given as its form id in hex.
const KNOWN_WORLDS: [(&str, u32); 1] = [("WastelandNV", 0x000d_a726)];

/// A world space's form id from a place's `world`: a known editor id, or hex digits.
pub fn world_form_id(world: &str) -> Result<u32, String> {
    if let Some((_, id)) = KNOWN_WORLDS.iter().find(|(n, _)| *n == world) {
        return Ok(*id);
    }
    let digits = world.strip_prefix("0x").unwrap_or(world);
    u32::from_str_radix(digits, 16).map_err(|_| {
        format!("world `{world}` is not an editor id this build knows (WastelandNV) or a form id in hex")
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

    fn go_to(&mut self, p: &Place) -> Result<(), String> {
        let e = &mut *self.engine;
        let player = e.global::<u32>(PLAYER);
        if player == 0 {
            return Err("the game has no player yet".into());
        }
        let id = world_form_id(&p.world)?;
        let world = e.call(LOOKUP_FORM, &args![id]).u32();
        if world == 0 {
            return Err(format!("the game has no world space {:08X}", id));
        }
        let rad = |d: f64| (d.to_radians()) as f32;
        e.call(
            POSITION_PLAYER_EXTERIOR,
            &args![
                player,
                p.pos[0] as f32,
                p.pos[1] as f32,
                p.pos[2] as f32,
                rad(p.pitch),
                0.0f32,
                rad(p.heading),
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
        Ok(())
    }

    fn top_menu(&mut self) -> u32 {
        // `Interface::GetTopMenuID`: reads the menu stack.
        self.engine.call(0x0070_23c0, &args![]).u32()
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
    fn worlds_are_known_editor_ids_or_hex_form_ids() {
        assert_eq!(world_form_id("WastelandNV"), Ok(0x000d_a726));
        assert_eq!(world_form_id("0x000DA726"), Ok(0x000d_a726));
        assert_eq!(world_form_id("DA726"), Ok(0x000d_a726));
        assert!(world_form_id("Nowhere").unwrap_err().contains("Nowhere"));
    }
}
