//! `functions_jip/jip_fn_utility.h` (JIP LN NVSE): the commands built so far, `GetMinOf`,
//! `GetMaxOf`, `GetRandomInRange`, `GetLoadOrderChanged` and `ValidateModIndex`.

use super::Handlers;
use crate::game::BGSSaveLoadGame;
use engine_core::prelude::*;
use nvse::commands::Read;
use nvse::{handled, CommandArgs};

#[rustfmt::skip]
pub static COMMANDS: Handlers = &[
    (0x262e, get_min_of),
    (0x262f, get_max_of),
    (0x268e, get_load_order_changed),
    (0x268f, validate_mod_index),
    (0x28de, get_random_in_range),
];

/// `NUM_ARGS`: the count of arguments the script gave (the byte at the start of the arguments).
fn num_args(e: &Engine, a: &CommandArgs) -> usize {
    usize::from(e.mem.u8(a.data.wrapping_add(e.mem.u32(a.opcode_offset))))
}

/// The values of `GetMinOf`/`GetMaxOf`: all five slots read, the script's count of them used.
fn fold(e: &mut Engine, w: &[u32], pick: fn(f64, f64) -> f64) -> Ret {
    let a = CommandArgs::new(w);
    let count = num_args(e, &a);
    let read = Read::of(e, &a);
    if read.ok {
        let value = |i: usize| read.args.get(i).and_then(|x| x.double()).unwrap_or(0.0);
        let mut best = value(0);
        let mut i = 1;
        // `iterHead: ... inc eax; cmp al, numArgs; jb`: at least one step after the first.
        loop {
            best = pick(best, value(i));
            i += 1;
            if i >= count {
                break;
            }
        }
        a.set_result(e, best);
    }
    handled(true)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_jip/jip_fn_utility.h:
// Cmd_GetMinOf_Execute.
/// The least of the arguments (`minsd`: the running value when it is less, else the next, so a
/// NaN next value wins).
fn get_min_of(e: &mut Engine, w: &[u32]) -> Ret {
    fold(e, w, |best, next| if best < next { best } else { next })
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_jip/jip_fn_utility.h:
// Cmd_GetMaxOf_Execute.
fn get_max_of(e: &mut Engine, w: &[u32]) -> Ret {
    fold(e, w, |best, next| if best > next { best } else { next })
}

/// `g_BGSSaveLoadGame` (JIP LN NVSE).
const SAVE_LOAD_GAME: u32 = 0x011d_df38;

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_jip/jip_fn_utility.h:
// Cmd_GetLoadOrderChanged_Execute.
/// 1 when some file of the save (index 254 down to 1) is loaded as another index today: the
/// table `saveMods` against the 255 bytes after it.
fn get_load_order_changed(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    let table = e.mem.u32(SAVE_LOAD_GAME) + BGSSaveLoadGame::saveMods.off;
    for idx in (1..=254u32).rev() {
        let now = e.mem.u8(table + idx);
        if now == 0xff || now == e.mem.u8(table + idx + 0xff) {
            continue;
        }
        a.set_result(e, 1.0);
        break;
    }
    handled(true)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_jip/jip_fn_utility.h:
// Cmd_ValidateModIndex_Execute.
/// The index as loaded today (`GetResolvedModIndex`), -1 for one above 0xFF or no longer loaded.
fn validate_mod_index(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    let read = Read::of(e, &a);
    let mut answer = -1.0;
    if read.ok {
        let index = read.int(0, 0);
        if index <= 0xff {
            if let Some(now) = crate::cosave::resolved_mod_index(e, index as u8) {
                answer = f64::from(now);
            }
        }
    }
    a.set_result(e, answer);
    handled(true)
}

/// `GAME_RNG` and the game's `GetRandomInt` (`ADDR_GetRandomInt`, `thiscall`: the generator and
/// the bound).
const GAME_RNG: u32 = 0x011c_4180;
const GET_RANDOM_INT: u32 = 0x00aa_5230;

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_jip/jip_fn_utility.h:
// Cmd_GetRandomInRange_Execute.
/// A number from the game's generator below `max - min`, plus `min` (signed, wrapping).
fn get_random_in_range(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    let read = Read::of(e, &a);
    if read.ok {
        let (min, max) = (read.int(0, 0), read.int(1, 0));
        let r = e
            .call(GET_RANDOM_INT, &args![GAME_RNG, max.wrapping_sub(min)])
            .u32();
        a.set_result(e, f64::from(r.wrapping_add(min) as i32));
    }
    handled(true)
}

#[cfg(test)]
#[path = "jip_fn_utility_tests.rs"]
mod tests;
