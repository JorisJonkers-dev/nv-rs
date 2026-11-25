// SPDX-License-Identifier: GPL-3.0-only
//! ShowOff xNVSE's `ApplyEasing` (with group 9 of the profile's missing commands). Written from
//! the command's GECK Wiki page (see `NOTICE`, `provenance/09-enable-parent-equipped-weapon.md`);
//! no plugin code was read.
//!
//! - `ApplyEasing input name mode` (alias `Ease`): `input` (meant to be 0 to 1) through an easing
//!   curve, as a number. The page names the curves by the start of their name, case-insensitive
//!   (`Sin`, `Quad`, `Cub`, `Quart`, `Quint`, `Circ`, `Expo`, `Back`, `Elastic`, `Bounce`), and
//!   says they are the curves of the easings.net reference; `mode` 0 eases in, 1 out, 2 in and
//!   out. An unknown name or mode answers -1, as the page says. The input is not clamped.
//!
//! The curves are the reference's published formulas ([`ease`]).

use crate::commands::Cmd;
use crate::identity::OPCODE_BASE;
use engine_core::prelude::*;
use nvse::expr::{self, Eval, Ty};
use nvse::table::{Param, Returns};
use nvse::{handled, CommandArgs};
use std::f64::consts::PI;

/// xNVSE's expression parameter kinds: a number, a string.
const NV_NUMBER: u32 = 0x8001;
const NV_STRING: u32 = 0x8004;

const EASING_PARAMS: &[Param] = &[
    Param {
        name: "input",
        kind: NV_NUMBER,
        optional: false,
    },
    Param {
        name: "easingFuncName",
        kind: NV_STRING,
        optional: false,
    },
    Param {
        name: "easingMode",
        kind: NV_NUMBER,
        optional: false,
    },
];

/// The commands, in the order of their opcodes.
pub static COMMANDS: [Cmd; 1] = [Cmd {
    opcode: OPCODE_BASE + 0x7f,
    name: "ApplyEasing",
    short: "Ease",
    params: EASING_PARAMS,
    execute: apply_easing,
    eval: None,
    needs_ref: false,
    expression: true,
    returns: Returns::Default,
}];

/// The curves, in the page's order of `ApplyEasingAlt`'s codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Curve {
    Sine,
    Quad,
    Cubic,
    Quart,
    Quint,
    Circ,
    Expo,
    Back,
    Elastic,
    Bounce,
}

/// In, out, or in and out (the page's modes 0, 1, 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    In,
    Out,
    InOut,
}

/// The curve a name stands for: the name starts with one of the page's prefixes, any case.
pub fn curve_named(name: &[u8]) -> Option<Curve> {
    const PREFIXES: [(&[u8], Curve); 10] = [
        (b"sin", Curve::Sine),
        (b"quad", Curve::Quad),
        (b"cub", Curve::Cubic),
        (b"quart", Curve::Quart),
        (b"quint", Curve::Quint),
        (b"circ", Curve::Circ),
        (b"expo", Curve::Expo),
        (b"back", Curve::Back),
        (b"elastic", Curve::Elastic),
        (b"bounce", Curve::Bounce),
    ];
    let lower = name.to_ascii_lowercase();
    PREFIXES
        .iter()
        .find(|(prefix, _)| lower.starts_with(prefix))
        .map(|&(_, curve)| curve)
}

/// The mode of the page's number.
pub fn mode_of(n: f64) -> Option<Mode> {
    match n as i32 {
        0 => Some(Mode::In),
        1 => Some(Mode::Out),
        2 => Some(Mode::InOut),
        _ => None,
    }
}

fn bounce_out(x: f64) -> f64 {
    const N1: f64 = 7.5625;
    const D1: f64 = 2.75;
    if x < 1.0 / D1 {
        N1 * x * x
    } else if x < 2.0 / D1 {
        let x = x - 1.5 / D1;
        N1 * x * x + 0.75
    } else if x < 2.5 / D1 {
        let x = x - 2.25 / D1;
        N1 * x * x + 0.9375
    } else {
        let x = x - 2.625 / D1;
        N1 * x * x + 0.984375
    }
}

/// A power curve of degree `n`: in, out, in and out.
fn power(x: f64, n: i32, mode: Mode) -> f64 {
    match mode {
        Mode::In => x.powi(n),
        Mode::Out => 1.0 - (1.0 - x).powi(n),
        Mode::InOut if x < 0.5 => 2f64.powi(n - 1) * x.powi(n),
        Mode::InOut => 1.0 - (-2.0 * x + 2.0).powi(n) / 2.0,
    }
}

/// `x` through the curve in the mode (the easings.net formulas).
pub fn ease(x: f64, curve: Curve, mode: Mode) -> f64 {
    const C1: f64 = 1.70158;
    const C2: f64 = C1 * 1.525;
    const C3: f64 = C1 + 1.0;
    const C4: f64 = 2.0 * PI / 3.0;
    const C5: f64 = 2.0 * PI / 4.5;
    match (curve, mode) {
        (Curve::Sine, Mode::In) => 1.0 - (x * PI / 2.0).cos(),
        (Curve::Sine, Mode::Out) => (x * PI / 2.0).sin(),
        (Curve::Sine, Mode::InOut) => -((PI * x).cos() - 1.0) / 2.0,
        (Curve::Quad, m) => power(x, 2, m),
        (Curve::Cubic, m) => power(x, 3, m),
        (Curve::Quart, m) => power(x, 4, m),
        (Curve::Quint, m) => power(x, 5, m),
        (Curve::Circ, Mode::In) => 1.0 - (1.0 - x * x).sqrt(),
        (Curve::Circ, Mode::Out) => (1.0 - (x - 1.0).powi(2)).sqrt(),
        (Curve::Circ, Mode::InOut) if x < 0.5 => (1.0 - (1.0 - (2.0 * x).powi(2)).sqrt()) / 2.0,
        (Curve::Circ, Mode::InOut) => ((1.0 - (-2.0 * x + 2.0).powi(2)).sqrt() + 1.0) / 2.0,
        (Curve::Expo, Mode::In) if x == 0.0 => 0.0,
        (Curve::Expo, Mode::In) => 2f64.powf(10.0 * x - 10.0),
        (Curve::Expo, Mode::Out) if x == 1.0 => 1.0,
        (Curve::Expo, Mode::Out) => 1.0 - 2f64.powf(-10.0 * x),
        (Curve::Expo, Mode::InOut) if x == 0.0 || x == 1.0 => x,
        (Curve::Expo, Mode::InOut) if x < 0.5 => 2f64.powf(20.0 * x - 10.0) / 2.0,
        (Curve::Expo, Mode::InOut) => (2.0 - 2f64.powf(-20.0 * x + 10.0)) / 2.0,
        (Curve::Back, Mode::In) => C3 * x.powi(3) - C1 * x * x,
        (Curve::Back, Mode::Out) => 1.0 + C3 * (x - 1.0).powi(3) + C1 * (x - 1.0).powi(2),
        (Curve::Back, Mode::InOut) if x < 0.5 => {
            (2.0 * x).powi(2) * ((C2 + 1.0) * 2.0 * x - C2) / 2.0
        }
        (Curve::Back, Mode::InOut) => {
            ((2.0 * x - 2.0).powi(2) * ((C2 + 1.0) * (x * 2.0 - 2.0) + C2) + 2.0) / 2.0
        }
        (Curve::Elastic, _) if x == 0.0 || x == 1.0 => x,
        (Curve::Elastic, Mode::In) => {
            -(2f64.powf(10.0 * x - 10.0)) * ((x * 10.0 - 10.75) * C4).sin()
        }
        (Curve::Elastic, Mode::Out) => 2f64.powf(-10.0 * x) * ((x * 10.0 - 0.75) * C4).sin() + 1.0,
        (Curve::Elastic, Mode::InOut) if x < 0.5 => {
            -(2f64.powf(20.0 * x - 10.0) * ((20.0 * x - 11.125) * C5).sin()) / 2.0
        }
        (Curve::Elastic, Mode::InOut) => {
            2f64.powf(-20.0 * x + 10.0) * ((20.0 * x - 11.125) * C5).sin() / 2.0 + 1.0
        }
        (Curve::Bounce, Mode::In) => 1.0 - bounce_out(1.0 - x),
        (Curve::Bounce, Mode::Out) => bounce_out(x),
        (Curve::Bounce, Mode::InOut) if x < 0.5 => (1.0 - bounce_out(1.0 - 2.0 * x)) / 2.0,
        (Curve::Bounce, Mode::InOut) => (1.0 + bounce_out(2.0 * x - 1.0)) / 2.0,
    }
}

/// `ApplyEasing input name mode`.
fn apply_easing(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    a.set_result(e, -1.0);
    let mut ev = Eval::new(e, &a);
    expr::with_evaluator(e, |e| {
        let Some(toks) = ev.extract_args(e) else {
            return;
        };
        let [input, name, mode] = toks.as_slice() else {
            return;
        };
        if !ev.can_convert(e, input, Ty::Number)
            || !ev.can_convert(e, name, Ty::String)
            || !ev.can_convert(e, mode, Ty::Number)
        {
            return;
        }
        let x = ev.get_number(e, input);
        let name = ev.get_string(e, name);
        let mode = ev.get_number(e, mode);
        if let (Some(curve), Some(mode)) = (curve_named(&name), mode_of(mode)) {
            a.set_result(e, ease(x, curve, mode));
        }
    });
    expr::report(e, &ev);
    handled(true)
}

#[cfg(test)]
#[path = "easing_tests.rs"]
mod tests;
