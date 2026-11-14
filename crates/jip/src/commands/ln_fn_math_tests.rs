//! Tests of the math commands. JIP's angle functions are approximations, so the answers are held
//! to the true function within the error the polynomials have, and the special cases (a zero, a
//! magnitude of 1 or more, a zero cosine) to the exact words JIP's code answers.

use super::*;
use crate::commands::test_support::*;

const A: u32 = FORMS + 0x1000;
const B: u32 = FORMS + 0x2000;
const CELL1: u32 = FORMS + 0x3000;
const CELL2: u32 = FORMS + 0x3100;
const CELL3: u32 = FORMS + 0x3200;
const WORLD: u32 = FORMS + 0x4000;
const WORLD2: u32 = FORMS + 0x4100;

fn degrees(command: u32, v: f64) -> f64 {
    let mut e = engine();
    call(&mut e, command, &[z(v)]).1
}

fn near(got: f64, want: f64, tolerance: f64) {
    assert!(
        (got - want).abs() <= tolerance,
        "{got} is not {want} within {tolerance}"
    );
}

fn setup() -> Engine {
    let mut e = engine();
    for (at, pos) in [(A, [1.0f32, 2.0, 3.0]), (B, [4.0, 6.0, 15.0])] {
        for (i, v) in pos.iter().enumerate() {
            e.mem.set_f32(at + 0x30 + 4 * i as u32, *v);
        }
    }
    form(&mut e, A, 0x3a, 0x0000_0101);
    form(&mut e, B, 0x3a, 0x0000_0102);
    e.mem.set_u32(A + 0x40, CELL1);
    e.mem.set_u32(B + 0x40, CELL1);
    e.mem.set_u32(CELL1 + 0xc0, WORLD);
    e.mem.set_u32(CELL2 + 0xc0, WORLD);
    e.mem.set_u32(CELL3 + 0xc0, WORLD2);
    set_ref(&mut e, 1, B);
    set_ref(&mut e, 2, A);
    e.register(0x0041_0220, |_, _| Ret::default());
    e
}

#[test]
fn square_root() {
    assert_eq!(degrees(0x22f0, 16.0), 4.0);
    assert_eq!(degrees(0x22f0, 2.0), 2.0f64.sqrt());
    assert_eq!(
        degrees(0x22f0, -4.0),
        0.0,
        "a negative value is 0 (maxsd with 0)"
    );
    assert_eq!(degrees(0x22f0, 0.0), 0.0);
}

#[test]
fn sine_and_cosine_in_degrees() {
    for d in [
        -720.0, -270.0, -90.0, -45.0, 0.0, 1.0, 30.0, 45.0, 90.0, 135.0, 180.0, 225.0, 270.0,
        359.0, 360.0, 1000.0,
    ] {
        let rad = (d as f32 * FLT_PI_D_180) as f64;
        near(degrees(0x22fd, d), rad.sin(), 2e-6);
        near(degrees(0x22fe, d), rad.cos(), 2e-6);
    }
}

#[test]
fn cosine_of_zero_is_one_but_the_vector_version_is_not() {
    assert_eq!(degrees(0x22fe, 0.0), 1.0);
    assert_eq!(cos(0.0), 1.0);
    // `Cos_V3` has no zero test: its polynomial's last word.
    assert_eq!(cos_v3([0.0, 0.0])[1], COS[4]);
    assert_eq!(cos(f32::NAN), 1.0, "unordered compares as 0 or less");
}

#[test]
fn tangent() {
    for d in [
        -80.0, -45.0, -10.0, 0.0, 10.0, 30.0, 45.0, 60.0, 80.0, 100.0, 135.0, 200.0,
    ] {
        let rad = (d as f32 * FLT_PI_D_180) as f64;
        let want = rad.tan();
        near(degrees(0x2893, d), want, 1e-4 * want.abs().max(1.0));
    }
    // A cosine under FLT_EPSILON is out of range: 0, not infinity.
    assert_eq!(tan(FLT_PI_D_2), 0.0);
    assert_eq!(tan(f32::NAN), 0.0);
}

#[test]
fn arcsine_and_arccosine_in_degrees() {
    for x in [-0.99, -0.75, -0.5, -0.1, 0.1, 0.25, 0.5, 0.7071, 0.9, 0.999] {
        near(
            degrees(0x230f, x),
            f64::from(x as f32).asin().to_degrees(),
            1e-3,
        );
        near(
            degrees(0x2310, x),
            f64::from(x as f32).acos().to_degrees(),
            1e-3,
        );
    }
    assert_eq!(degrees(0x230f, 0.0), 0.0);
    assert_eq!(degrees(0x2310, 0.0), f64::from(asin_word(3)) * DBL_180_D_PI);
    near(degrees(0x230f, 1.0), 90.0, 1e-5);
    near(degrees(0x230f, -7.0), -90.0, 1e-5);
    near(degrees(0x2310, 1.0), 0.0, 0.0);
    near(degrees(0x2310, 5.0), 0.0, 0.0);
    near(degrees(0x2310, -1.0), 180.0, 1e-5);
    near(degrees(0x2310, -2.0), 180.0, 1e-5);
}

#[test]
fn arctangent_and_the_two_argument_one() {
    for x in [-100.0, -2.0, -0.5, -0.01, 0.01, 0.5, 1.0, 2.0, 100.0] {
        near(
            degrees(0x230b, x),
            f64::from(x as f32).atan().to_degrees(),
            1e-3,
        );
    }
    let mut e = engine();
    for (y, x) in [
        (1.0, 1.0),
        (1.0, -1.0),
        (-1.0, -1.0),
        (-1.0, 1.0),
        (3.0, 4.0),
        (-4.0, 3.0),
        (0.5, -2.0),
    ] {
        let got = call(&mut e, 0x2894, &[z(y), z(x)]).1;
        near(
            got,
            f64::from(y as f32).atan2(f64::from(x as f32)).to_degrees(),
            1e-3,
        );
    }
    // A zero y: y itself for x of 0 or more, pi (180) for a negative x.
    assert_eq!(call(&mut e, 0x2894, &[z(0.0), z(5.0)]).1, 0.0);
    near(call(&mut e, 0x2894, &[z(0.0), z(-5.0)]).1, 180.0, 1e-5);
    // A zero x: plus or minus pi/2.
    near(call(&mut e, 0x2894, &[z(2.0), z(0.0)]).1, 90.0, 1e-5);
    near(call(&mut e, 0x2894, &[z(-2.0), z(0.0)]).1, -90.0, 1e-5);
}

#[test]
fn distances_in_one_cell() {
    let mut e = setup();
    let d2 = call_on(&mut e, 0x22e0, A, &[r(1)]).1;
    near(d2, 5.0, 1e-4);
    let d3 = call_on(&mut e, 0x22e1, A, &[r(1)]).1;
    near(d3, 13.0, 1e-4);
    // The conditions answer the same through their eval.
    for (opcode, want) in [(0x22e0, 5.0), (0x22e1, 13.0)] {
        let record = nvse::table::record_of(&e, opcode);
        let at = e.mem.u32(record + 0x20);
        assert_ne!(at, 0);
        e.mem.set_f64(RESULT, 0.5);
        e.call(at, &[A, B, 0, RESULT]);
        near(e.mem.f64(RESULT), want, 1e-4);
    }
    // The same point: 0.
    assert_eq!(call_on(&mut e, 0x22e1, A, &[r(2)]).1, 0.0);
}

#[test]
fn distance_across_cells_and_worlds() {
    let mut e = setup();
    // Cells of one worldspace.
    e.mem.set_u32(B + 0x40, CELL2);
    near(call_on(&mut e, 0x22e1, A, &[r(1)]).1, 13.0, 1e-4);
    // Another worldspace, or no worldspace: the largest float.
    e.mem.set_u32(B + 0x40, CELL3);
    assert_eq!(call_on(&mut e, 0x22e0, A, &[r(1)]).1, f64::from(f32::MAX));
    assert_eq!(call_on(&mut e, 0x22e1, A, &[r(1)]).1, f64::from(f32::MAX));
    e.mem.set_u32(CELL1 + 0xc0, 0);
    e.mem.set_u32(B + 0x40, CELL2);
    assert_eq!(call_on(&mut e, 0x22e1, A, &[r(1)]).1, f64::from(f32::MAX));
    // No cell and no persistent cell extra: false.
    e.mem.set_u32(B + 0x40, 0);
    assert_eq!(call_on(&mut e, 0x22e1, A, &[r(1)]).1, f64::from(f32::MAX));
}

#[test]
fn a_persistent_cell_stands_in_for_the_parent_cell() {
    let mut e = setup();
    e.mem.set_u32(B + 0x40, 0);
    e.register(0x0041_0220, |e, w| {
        // The extra of reference B only: its cell at +0xC.
        if w[0] == B + 0x44 && w[1] == 0x0c {
            e.mem.set_u32(FORMS + 0x5000 + 0xc, CELL1);
            Ret {
                eax: FORMS + 0x5000,
                ..Ret::default()
            }
        } else {
            Ret::default()
        }
    });
    near(call_on(&mut e, 0x22e1, A, &[r(1)]).1, 13.0, 1e-4);
}
