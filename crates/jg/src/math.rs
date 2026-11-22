// SPDX-License-Identifier: GPL-3.0-only
//! JohnnyGuitar NVSE's pure math commands: `Clamp`, `Lerp` and `Sign` (group 1 of the profile's
//! missing commands). Written from the commands' GECK Wiki pages (see `NOTICE`); no plugin code
//! was read. Each takes plain numbers and touches no game state.
//!
//! - `Clamp value min max`: "clamps the value to a specified range". The page's three examples
//!   fix the order of the tests: `Clamp 5 10 20` is 10, `Clamp 75 100 50` is 100 and
//!   `Clamp 1000 500 50` is 50. So `value < min` gives `min`, otherwise `value > max` gives `max`,
//!   otherwise the value; a range whose `min` is above its `max` answers by that order.
//! - `Lerp v0 v1 t`: the interpolation between `v0` and `v1` for `t`. Written as
//!   `v0 * (1 - t) + v1 * t`, so `t = 0` and `t = 1` give the ends exactly. The page documents
//!   `t` in `[0, 1]` only: outside it the line goes on (extrapolates), which the page neither
//!   promises nor excludes.
//! - `Sign value`: 1 for a positive value, -1 for a negative one (the page lists those two
//!   answers). For 0 the page says nothing: it answers 0, the mathematical sign.
//!
//! Numbers are read as `float`, as every `kParamType_Float` argument is, and the answer is that
//! float widened to the result double.

use crate::commands::Cmd;
use crate::identity::OPCODE_BASE;
use engine_core::prelude::*;
use nvse::commands::Read;
use nvse::table::Param;
use nvse::{handled, CommandArgs};

const fn float(name: &'static str) -> Param {
    Param {
        name,
        kind: 0x02,
        optional: false,
    }
}

const THREE_FLOATS: &[Param] = &[float("float"), float("float"), float("float")];
const ONE_FLOAT: &[Param] = &[float("float")];

/// The commands, in the order of their opcodes.
pub static COMMANDS: [Cmd; 3] = [
    Cmd {
        opcode: OPCODE_BASE + 0x55,
        name: "Clamp",
        short: "",
        params: THREE_FLOATS,
        execute: clamp,
        eval: None,
    },
    Cmd {
        opcode: OPCODE_BASE + 0x57,
        name: "Lerp",
        short: "",
        params: THREE_FLOATS,
        execute: lerp,
        eval: None,
    },
    Cmd {
        opcode: OPCODE_BASE + 0x5a,
        name: "Sign",
        short: "",
        params: ONE_FLOAT,
        execute: sign,
        eval: None,
    },
];

/// The value limited to `min..=max` in the order the page's examples show.
pub fn clamped(value: f32, min: f32, max: f32) -> f32 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// `v0 * (1 - t) + v1 * t`.
pub fn lerped(v0: f32, v1: f32, t: f32) -> f32 {
    v0 * (1.0 - t) + v1 * t
}

/// 1, -1, or 0 for a zero (and for NaN, which is neither).
pub fn signed(value: f32) -> f32 {
    if value > 0.0 {
        1.0
    } else if value < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// Reads the numbers, answers `f` of them; 0 when the arguments cannot be read.
fn answer(e: &mut Engine, w: &[u32], f: fn(&Read) -> f32) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, 0.0);
    let read = Read::of(e, &a);
    if read.ok {
        a.set_result(e, f64::from(f(&read)));
    }
    handled(true)
}

fn clamp(e: &mut Engine, w: &[u32]) -> Ret {
    answer(e, w, |r| {
        clamped(r.float(0, 0.0), r.float(1, 0.0), r.float(2, 0.0))
    })
}

fn lerp(e: &mut Engine, w: &[u32]) -> Ret {
    answer(e, w, |r| {
        lerped(r.float(0, 0.0), r.float(1, 0.0), r.float(2, 0.0))
    })
}

fn sign(e: &mut Engine, w: &[u32]) -> Ret {
    answer(e, w, |r| signed(r.float(0, 0.0)))
}

#[cfg(test)]
#[path = "math_tests.rs"]
mod tests;
