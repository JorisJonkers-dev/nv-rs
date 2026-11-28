// SPDX-License-Identifier: GPL-3.0-only
//! The extra land passes (nv-rs#912, `light.land_lights`): what the batch renderer sets for a land pass that is
//! not the first (the sort byte 0) while the setting is on. The pass carries the point lights past a quad's
//! third, and is drawn added to the picture (blend one, one; z-writes off, z-function equal). The land pixel
//! shader (`SLS2100` to `SLS2147`) ends every pass as `(lights + ambient) * texture` fogged, so a second
//! pass of it would add the ambient and the sun again and the fog colour once more; they are set to zero for
//! these passes only (the pixel constants 1 and 3, the sun's colour, and the vertex constant 15, the fog
//! colour), and put back when a first pass of the type is drawn after them.

use super::{capture, own};
use crate::shader::{self, state};
use engine_core::land_lights;
use engine_core::{typed_call, Engine};
use std::sync::atomic::{AtomicBool, Ordering};

/// The device's `SetPixelShaderConstantF` slot, the vertex constant of the fog colour and the pixel
/// constants of the ambient colour and the first light's colour (the sun's).
const SET_PIXEL_CONSTANTS: u32 = 0x1b4;
const FOG_COLOUR_REGISTER: u32 = 15;
const AMBIENT_REGISTER: u32 = 1;
const SUN_REGISTER: u32 = 3;
/// The fog constants (`011fa280`); the colour is the second row.
const FOG_COLOUR: u32 = 0x011f_a290;

/// Whether the states and the fog colour are those of an extra pass.
static EXTRA: AtomicBool = AtomicBool::new(false);

/// The pass constants were set up (the fog colour with them): none of the extra pass's is left.
pub fn pass_begun() {
    EXTRA.store(false, Ordering::Relaxed);
}

/// What a plain geometry of a pass type with a sort byte asks for.
#[derive(Debug, PartialEq, Eq)]
enum Action {
    /// The game's own draw.
    Nothing,
    /// An extra pass: its terms zeroed (the fog colour when `first`, the first one since the pass began) and its states set.
    Extra { first: bool },
    /// A first pass after extra ones: the fog colour and the states put back.
    Back,
}

/// What a geometry of `pass_type` with the sort byte `sort` asks for, `extra` being whether the states and fog colour
/// are an extra pass's now and `wanted` the setting.
fn decide(pass_type: u32, sort: u8, extra: bool, wanted: bool) -> Action {
    if !land_lights::is_land(pass_type) || !(extra || wanted) {
        Action::Nothing
    } else if sort == 0 && wanted {
        Action::Extra { first: !extra }
    } else if sort != 0 && extra {
        Action::Back
    } else {
        Action::Nothing
    }
}

/// Before a plain geometry of `pass_type` with the sort byte `sort` is drawn, after its set-ups.
pub fn before_draw(e: &mut Engine, pass_type: u32, sort: u8) {
    let extra = EXTRA.load(Ordering::Relaxed);
    let action = decide(pass_type, sort, extra, land_lights::wanted());
    if action == Action::Nothing {
        return;
    }
    let device = super::packet::pass_device(e);
    match action {
        Action::Extra { first } => {
            zero_terms(e, device, first);
            EXTRA.store(true, Ordering::Relaxed);
            typed_call!(
                shader::FAMILY,
                0x00b9_7fa0,
                state::set_alpha_blend_enable(e, 1u8, 0u32)
            );
            typed_call!(
                shader::FAMILY,
                0x00b9_7ff0,
                state::set_blend(e, 0u32, 0u32, 0u32)
            );
            typed_call!(
                shader::FAMILY,
                0x00b9_7e30,
                state::set_z_write_enable(e, 0u8, 0u32)
            );
            typed_call!(
                shader::FAMILY,
                0x00b9_7e80,
                state::set_z_function(e, 2u32, 0u32)
            );
        }
        Action::Back => {
            put_back_fog(e, device);
            EXTRA.store(false, Ordering::Relaxed);
            typed_call!(
                shader::FAMILY,
                0x00b9_7e30,
                state::set_z_write_enable(e, 1u8, 0u32)
            );
            typed_call!(
                shader::FAMILY,
                0x00b9_7e80,
                state::set_z_function(e, 3u32, 0u32)
            );
        }
        Action::Nothing => {}
    }
}

/// The extra pass's terms zeroed: the ambient and the sun's colour of every geometry (its set-up writes them again
/// for each), and the fog colour once.
fn zero_terms(e: &mut Engine, device: u32, first: bool) {
    if first {
        set_fog_colour(e, device, [0; 4]);
    }
    constant(e, device, AMBIENT_REGISTER, [0; 4], SET_PIXEL_CONSTANTS);
    constant(e, device, SUN_REGISTER, [0; 4], SET_PIXEL_CONSTANTS);
}

/// The fog colour the pass constants set up, uploaded again.
fn put_back_fog(e: &mut Engine, device: u32) {
    let mut colour = [0u32; 4];
    for (word, i) in colour.iter_mut().zip(0u32..) {
        *word = capture::own_at::<u32>(e, FOG_COLOUR + 4 * i);
    }
    set_fog_colour(e, device, colour);
}

fn set_fog_colour(e: &mut Engine, device: u32, colour: [u32; 4]) {
    constant(e, device, FOG_COLOUR_REGISTER, colour, 0x178);
}

/// One constant register of the device's float set `slot`.
fn constant(e: &mut Engine, device: u32, register: u32, words: [u32; 4], slot: u32) {
    e.with_stack(0x10, |e, data| {
        for (i, word) in (0u32..).zip(words) {
            e.mem.set_u32(data.addr() + 4 * i, word);
        }
        let _ = own::device_vertex_constants_of(e, device, slot, register, data.addr(), 1);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_core::prelude::*;

    const CURRENT_PASS: u32 = 0x0126_f74c;
    const VT: u32 = 0x0090_0000;
    const NOTES: u32 = 0x0300_0000;

    /// An engine whose current pass leads to a device that notes each float constant set (`slot`, register,
    /// first word) and a render state cache that notes each state set.
    fn rig() -> Engine {
        let mut e = Engine::new();
        e.map(0x0126_f000, 0x1000);
        e.map(NOTES, 0x4000);
        for page in [
            0x011a_d000u32,
            0x011a_e000,
            0x011f_4000,
            0x011f_9000,
            0x011f_a000,
            0x011f_f000,
        ] {
            e.map(page, 0x1000);
        }
        let pass = e.mem.alloc(0x100);
        let holder = e.mem.alloc(0x100);
        let device = e.mem.alloc(0x100);
        e.mem.set_u32(CURRENT_PASS, pass);
        e.mem.set_u32(pass + 0x5c, holder);
        e.mem.set_u32(holder + 0x20, device);
        e.mem.set_u32(device, VT);
        e.put_vtable(VT, &[0; 0x200 / 4]);
        e.mem.set_u32(VT + 0x178, 0x0040_0178);
        e.register(0x0040_0178, note::<0x178>);
        e.mem.set_u32(VT + 0x1b4, 0x0040_01b4);
        e.register(0x0040_01b4, note::<0x1b4>);
        e
    }

    fn note<const SLOT: u32>(e: &mut Engine, a: &[u32]) -> Ret {
        let n = e.mem.u32(NOTES);
        e.mem.set_u32(NOTES, n + 1);
        e.mem.set_u32(NOTES + 4 + 12 * n, SLOT);
        e.mem.set_u32(NOTES + 8 + 12 * n, a[1]);
        e.mem.set_u32(NOTES + 12 + 12 * n, e.mem.u32(a[2]));
        0u32.into_ret()
    }

    fn notes(e: &Engine) -> Vec<(u32, u32, u32)> {
        (0..e.mem.u32(NOTES))
            .map(|n| {
                (
                    e.mem.u32(NOTES + 4 + 12 * n),
                    e.mem.u32(NOTES + 8 + 12 * n),
                    e.mem.u32(NOTES + 12 + 12 * n),
                )
            })
            .collect()
    }

    #[test]
    fn the_choice_follows_the_sort_byte_and_the_setting() {
        let land = 0x1fc;
        assert_eq!(decide(land, 0, false, false), Action::Nothing, "off");
        assert_eq!(
            decide(land, 1, false, true),
            Action::Nothing,
            "a first pass"
        );
        assert_eq!(decide(land, 0, false, true), Action::Extra { first: true });
        assert_eq!(decide(land, 0, true, true), Action::Extra { first: false });
        assert_eq!(decide(land, 1, true, true), Action::Back);
        assert_eq!(
            decide(land, 1, true, false),
            Action::Back,
            "switched off meanwhile"
        );
        assert_eq!(decide(land, 0, true, false), Action::Nothing);
        assert_eq!(
            decide(0x10, 0, false, true),
            Action::Nothing,
            "not a land type"
        );
        assert_eq!(decide(0x10, 0, true, true), Action::Nothing);
    }

    #[test]
    fn an_extra_pass_zeroes_the_ambient_the_sun_and_once_the_fog_and_a_first_pass_puts_it_back() {
        let mut e = rig();
        let device = super::super::packet::pass_device(&e);
        e.mem.set_u32(FOG_COLOUR, 0x3f80_0000);
        zero_terms(&mut e, device, true);
        zero_terms(&mut e, device, false);
        assert_eq!(
            notes(&e),
            vec![
                (0x178, 15, 0),
                (0x1b4, 1, 0),
                (0x1b4, 3, 0),
                (0x1b4, 1, 0),
                (0x1b4, 3, 0)
            ]
        );
        put_back_fog(&mut e, device);
        assert_eq!(notes(&e).last(), Some(&(0x178, 15, 0x3f80_0000)));
    }
}
