// SPDX-License-Identifier: GPL-3.0-only
//! Tests of `ApplyEasing`: its record, the curves at their ends, middles and known points, the
//! names by their start in any case, and -1 for an unknown name or mode.

use super::*;
use crate::test_support::{call_expr, engine};
use fork_catalogue::{covers, ids};

const APPLY_EASING: u32 = 0x3d12;

const CURVES: [Curve; 10] = [
    Curve::Sine,
    Curve::Quad,
    Curve::Cubic,
    Curve::Quart,
    Curve::Quint,
    Curve::Circ,
    Curve::Expo,
    Curve::Back,
    Curve::Elastic,
    Curve::Bounce,
];

fn text(t: &str) -> Vec<u8> {
    let mut b = vec![b'S'];
    b.extend_from_slice(&(t.len() as u16).to_le_bytes());
    b.extend_from_slice(t.as_bytes());
    b
}

fn num(v: f64) -> Vec<u8> {
    let mut b = vec![b'Z'];
    b.extend_from_slice(&v.to_le_bytes());
    b
}

fn eased(e: &mut Engine, x: f64, name: &str, mode: f64) -> f64 {
    let (ok, result) = call_expr(e, APPLY_EASING, &[num(x), text(name), num(mode)]);
    assert_eq!(ok, 1);
    result
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn the_record_is_at_its_opcode() {
    let e = engine();
    let record = nvse::table::record_of(&e, APPLY_EASING);
    assert_ne!(record, 0);
    assert_eq!(
        nvse::table::guest_text(&e, e.mem.u32(record)),
        b"ApplyEasing"
    );
    assert_eq!(nvse::table::guest_text(&e, e.mem.u32(record + 4)), b"Ease");
    assert!(crate::COMMAND_NAMES.contains(&"ApplyEasing"));
}

/// Every curve starts at 0 and ends at 1 in every mode, and the in-and-out curves pass 0.5 in
/// the middle.
#[test]
fn every_curve_runs_from_zero_to_one() {
    for curve in CURVES {
        for mode in [Mode::In, Mode::Out, Mode::InOut] {
            assert!(
                close(ease(0.0, curve, mode), 0.0),
                "{curve:?} {mode:?} at 0"
            );
            assert!(
                close(ease(1.0, curve, mode), 1.0),
                "{curve:?} {mode:?} at 1"
            );
        }
        assert!(
            close(ease(0.5, curve, Mode::InOut), 0.5),
            "{curve:?} middle"
        );
    }
}

/// Known points of the reference's formulas, and out as the mirror of in.
#[test]
fn the_curves_match_the_reference_formulas() {
    assert!(close(ease(0.5, Curve::Quint, Mode::Out), 0.96875));
    assert!(close(ease(0.5, Curve::Quint, Mode::In), 0.03125));
    assert!(close(ease(0.25, Curve::Cubic, Mode::InOut), 0.0625));
    assert!(close(
        ease(0.5, Curve::Sine, Mode::In),
        1.0 - (PI / 4.0).cos()
    ));
    assert!(close(ease(0.5, Curve::Back, Mode::In), -0.0876975));
    assert!(close(ease(0.5, Curve::Expo, Mode::In), 2f64.powi(-5)));
    assert!(close(ease(0.2, Curve::Bounce, Mode::Out), 7.5625 * 0.04));
    for curve in CURVES {
        for x in [0.1, 0.3, 0.7] {
            let mirrored = 1.0 - ease(1.0 - x, curve, Mode::In);
            assert!(
                close(ease(x, curve, Mode::Out), mirrored),
                "{curve:?} at {x}"
            );
        }
    }
}

/// Through the record: Enhanced Movement's call (`"Quint" 1` on a ratio), a name matched by its
/// start in any case, and -1 for a name or mode the page does not list.
#[test]
fn apply_easing_answers_the_eased_value_or_minus_one() {
    covers(&[ids::extenders::showoff::APPLYEASING]);
    let mut e = engine();
    assert!(close(eased(&mut e, 0.5, "Quint", 1.0), 0.96875));
    assert!(close(eased(&mut e, 0.5, "QUINTIC", 0.0), 0.03125));
    assert!(close(
        eased(&mut e, 0.5, "sine", 0.0),
        1.0 - (PI / 4.0).cos()
    ));
    assert!(close(eased(&mut e, 0.25, "Cubic", 2.0), 0.0625));
    assert_eq!(eased(&mut e, 0.5, "Linear", 0.0), -1.0);
    assert_eq!(eased(&mut e, 0.5, "Qu", 0.0), -1.0);
    assert_eq!(eased(&mut e, 0.5, "Quint", 3.0), -1.0);
    assert_eq!(eased(&mut e, 0.5, "Quint", -1.0), -1.0);
}
