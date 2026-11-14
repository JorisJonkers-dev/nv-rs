//! Tests of the math commands: the routines against the C library within the error JIP's
//! polynomials allow, and the cases the SSE routines treat on their own (zeros, the ends of the
//! range, NaN), then the commands through the table.

use super::*;
use crate::commands::test_support::*;

fn value(e: &mut Engine, opcode: u32, args: &[Vec<u8>]) -> f64 {
    call(e, opcode, args).1
}

#[test]
fn the_routines_stay_close_to_the_library() {
    let mut worst: f32 = 0.0;
    for i in -4000..=4000 {
        let angle = i as f32 * 0.01;
        worst = worst.max((cos(angle) - angle.cos()).abs());
        worst = worst.max((sin(angle) - angle.sin()).abs());
    }
    assert!(worst < 1e-4, "sine and cosine, worst error {worst}");
    for i in -99..=99 {
        let x = i as f32 * 0.01;
        assert!((asin(x) - x.asin()).abs() < 1e-3, "asin {x}");
        assert!((acos(x) - x.acos()).abs() < 1e-3, "acos {x}");
    }
    for y in [-3.0f32, -1.0, -0.2, 0.2, 1.0, 3.0] {
        for x in [-3.0f32, -1.0, -0.2, 0.2, 1.0, 3.0] {
            assert!((atan2(y, x) - y.atan2(x)).abs() < 1e-3, "atan2 {y} {x}");
        }
    }
    for i in -85..=85 {
        let angle = i as f32 * 0.02;
        if angle.cos().abs() > 0.2 {
            assert!(
                (tan(angle) - angle.tan()).abs() < 1e-2 * (1.0 + angle.tan().abs()),
                "tan {angle}"
            );
        }
    }
}

#[test]
fn the_special_cases() {
    assert_eq!(cos(0.0), 1.0);
    assert_eq!(cos(f32::NAN), 1.0, "a NaN fails `x > 0`: the shortcut");
    assert_eq!(asin(0.0).to_bits(), 0.0f32.to_bits());
    assert_eq!(asin(-0.0).to_bits(), (-0.0f32).to_bits());
    assert_eq!(asin(1.0), pi_d2());
    assert_eq!(asin(-7.0), -pi_d2());
    assert_eq!(acos(0.0), pi_d2());
    assert_eq!(acos(1.0), 0.0);
    assert_eq!(acos(2.0), 0.0);
    assert_eq!(acos(-1.0), pi());
    assert_eq!(atan2(0.0, 5.0), 0.0);
    assert_eq!(atan2(0.0, -5.0), pi());
    assert_eq!(atan2(2.0, 0.0), pi_d2());
    assert_eq!(atan2(-2.0, 0.0), -pi_d2());
    assert_eq!(tan(pi_d2()), 0.0, "a cosine under FLT_EPSILON: 0");
}

#[test]
fn the_commands() {
    let mut e = engine();
    assert_eq!(value(&mut e, 0x22f0, &[z(2.25)]), 1.5);
    assert_eq!(
        value(&mut e, 0x22f0, &[z(-4.0)]),
        0.0,
        "a negative value counts as 0"
    );
    assert!((value(&mut e, 0x22fd, &[z(30.0)]) - 0.5).abs() < 1e-4);
    assert!((value(&mut e, 0x22fe, &[z(60.0)]) - 0.5).abs() < 1e-4);
    assert_eq!(value(&mut e, 0x22fe, &[z(0.0)]), 1.0);
    assert!((value(&mut e, 0x2893, &[z(45.0)]) - 1.0).abs() < 1e-3);
    assert!((value(&mut e, 0x230f, &[z(0.5)]) - 30.0).abs() < 0.1);
    assert!((value(&mut e, 0x2310, &[z(0.5)]) - 60.0).abs() < 0.1);
    assert!((value(&mut e, 0x230b, &[z(1.0)]) - 45.0).abs() < 0.1);
    assert!((value(&mut e, 0x2894, &[z(1.0), z(-1.0)]) - 135.0).abs() < 0.1);
    assert_eq!(
        value(&mut e, 0x2894, &[z(0.0), z(-1.0)]),
        f64::from(pi()) * D180_DPI
    );
}
