//! `functions_ln/ln_fn_math.h` (JIP LN NVSE): `fsqrt` and the float trigonometry in degrees
//! (`fSin`, `fCos`, `fTan`, `fAsin`, `fAcos`, `fAtan`, `fAtan2`): 8 of its 10 commands. The
//! distance conditions (`GetDistance2D`, `GetDistance3D`) are not built yet.
//!
//! JIP does not call the C library: its `Cos`, `Tan`, `ASin`, `ACos` and `ATan2`
//! (`internal/utility.cpp`) are SSE routines with their own polynomial coefficients and, for
//! the inverse sine and cosine, a table of 128 corrections. They are translated step by step in
//! single precision, so the answers match JIP's to the last bit, not the C library's.

use super::Handlers;
use engine_core::prelude::*;
use nvse::args::Arg;
use nvse::commands::Read;
use nvse::{handled, CommandArgs};

#[rustfmt::skip]
pub static COMMANDS: Handlers = &[
    (0x22f0, fsqrt),
    (0x22fd, f_sin),
    (0x22fe, f_cos),
    (0x230b, f_atan),
    (0x230f, f_asin),
    (0x2310, f_acos),
    (0x2893, f_tan),
    (0x2894, f_atan2),
];

/// `FltPId180`, `FltPId2`, `FltPI`, `FltPIx2` (`internal/utility.h`), as `float`.
const PI_D180: f32 = 0.017_453_292_38;
const PI_D2: f32 = 1.570_796_371;
const PI: f32 = 3.141_592_741;
const PI_X2: f32 = 6.283_185_482;
/// `Dbl180dPI`, the one constant JIP multiplies in `double`.
const D180_DPI: f64 = 57.295_779_513_082_32;
const SIGN: u32 = 0x8000_0000;

/// `cvttss2si`: truncation; what the instruction answers when the value does not fit is
/// `0x80000000`, the "integer indefinite".
fn cvtt(v: f32) -> i32 {
    if v.is_nan() || v >= 2_147_483_648.0 || v < -2_147_483_648.0 {
        i32::MIN
    } else {
        v as i32
    }
}

fn flip(v: f32) -> f32 {
    f32::from_bits(v.to_bits() ^ SIGN)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), internal/utility.cpp: Cos (the scalar routine).
/// `Cos(angle)`: the angle's quadrant by `angle * 2/pi` truncated, its remainder's polynomial.
/// An angle of 0 (or one that reduces to at or below 0, or NaN) answers exactly 1.
fn cos(angle: f32) -> f32 {
    const TWO_D_PI: f32 = f32::from_bits(0x3f22_f983);
    let mut x = angle.abs();
    let mut quadrant = cvtt(TWO_D_PI * x);
    if quadrant > 3 {
        let periods = (quadrant as u32) >> 2;
        quadrant &= 3;
        x -= (periods as i32 as f32) * PI_X2;
    }
    if !(x > 0.0) {
        return 1.0;
    }
    let mut sign = 0;
    let low = quadrant as u8;
    // `test al,al; jz doCalc; jp fourthQ`: a quadrant of 1 or 2 turns the half-period and the
    // sign; 3 takes the whole period.
    if low != 0 {
        let mut sub = PI_X2;
        if low.count_ones() % 2 != 0 {
            sub = PI;
            sign = SIGN;
        }
        x -= sub;
    }
    cos_poly(x, sign)
}

/// The polynomial of both `Cos` routines, on the reduced `x`.
fn cos_poly(x: f32, sign: u32) -> f32 {
    const C: [f32; 5] = [
        f32::from_bits(0x37c2_3ab1),
        f32::from_bits(0x3ab5_9551),
        f32::from_bits(0x3d2a_a76f),
        f32::from_bits(0x3eff_ffe0),
        f32::from_bits(0x3f7f_ffff),
    ];
    let x2 = x * x;
    let mut p = x2 * C[0];
    p -= C[1];
    p *= x2;
    p += C[2];
    p *= x2;
    p -= C[3];
    let r = x2 * p + C[4];
    // The product and the sum are separate instructions (`mulss`, `addss`).
    f32::from_bits(r.to_bits() ^ sign)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), internal/utility.cpp: Cos_V3 (the lanes
// `GetSinCos` uses).
/// `Cos_V3` on two lanes: it has no shortcut for a remainder at or below 0, and one reduction
/// step for all lanes when any lane's quadrant is above 3.
fn cos_lanes(angles: [f32; 2]) -> [f32; 2] {
    const TWO_D_PI: f32 = f32::from_bits(0x3f22_f983);
    let x = angles.map(f32::abs);
    let q = x.map(|v| cvtt(TWO_D_PI * v));
    let any = q.iter().any(|&q| q > 3);
    let mut out = [0.0; 2];
    for i in 0..2 {
        let mut xi = x[i];
        let mut qi = q[i];
        if any {
            let periods = (qi as u32) >> 2;
            qi &= 3;
            xi -= (periods as i32 as f32) * PI_X2;
        }
        let whole = if qi > 2 { PI_X2 } else { 0.0 };
        let (half, sign) = if qi > 0 && qi <= 2 {
            (PI, SIGN)
        } else {
            (0.0, 0)
        };
        // `orps` of the two: PI for a quadrant of 1 or 2, the whole period for 3.
        let sub = f32::from_bits(half.to_bits() | whole.to_bits());
        out[i] = cos_poly(xi - sub, sign);
    }
    out
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), internal/utility.h: Sin.
/// `Sin(angle) = Cos(pi/2 - angle)`.
fn sin(angle: f32) -> f32 {
    cos(PI_D2 - angle)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), internal/utility.cpp: GetSinCos, Tan.
/// `Tan(angle)`: the sine over the cosine, 0 when the cosine is below `FLT_EPSILON`.
fn tan(angle: f32) -> f32 {
    let [sin, cos] = cos_lanes([PI_D2 - angle, angle]);
    if cos.abs() < f32::EPSILON || cos.is_nan() {
        return 0.0;
    }
    sin / cos
}

/// The inverse sine's constants and corrections (`kASinConsts`, `internal/utility.cpp`): the
/// polynomial's three coefficients, pi/2, a step, one, pi and the exponent offset, then 128
/// corrections.
#[rustfmt::skip]
const ASIN: [u32; 136] = [
    0xbc996e30, 0x3d981627, 0x3e593484, 0x3fc90fdb, 0x34000000, 0x3f800000, 0x40490fdb, 0x03800000,
    0x37202a00, 0x37e24000, 0x38333600, 0x386e4c00, 0x38913200, 0x38a84000, 0x38bc5200, 0x38cdac00,
    0x38dc5000, 0x38e8c000, 0x38f2c800, 0x38faa800, 0x39005600, 0x39029000, 0x3903c800, 0x39046400,
    0x39042c00, 0x39036800, 0x39022800, 0x39003400, 0x38fb7000, 0x38f60800, 0x38efd800, 0x38e8b800,
    0x38e15800, 0x38d96000, 0x38d0c000, 0x38c83000, 0x38bf2000, 0x38b59800, 0x38ac5800, 0x38a2e000,
    0x38998000, 0x388fd000, 0x38866000, 0x387aa000, 0x38678000, 0x3855e000, 0x3844a000, 0x38338000,
    0x38234000, 0x3812e000, 0x38042000, 0x37eb4000, 0x37cf4000, 0x37b64000, 0x379d4000, 0x37874000,
    0x37658000, 0x373f8000, 0x371a8000, 0x36f90000, 0x36c30000, 0x36900000, 0x364e0000, 0x360e0000,
    0x35b40000, 0x35500000, 0x34d00000, 0x33c00000, 0x34000000, 0x34800000, 0x35400000, 0x35a80000,
    0x36040000, 0x36400000, 0x367c0000, 0x36a60000, 0x36d60000, 0x37040000, 0x371d0000, 0x37390000,
    0x37560000, 0x37750000, 0x378b0000, 0x379c0000, 0x37ac8000, 0x37be8000, 0x37d08000, 0x37e30000,
    0x37f58000, 0x3803c000, 0x380d0000, 0x3815c000, 0x381ec000, 0x38274000, 0x382f8000, 0x38380000,
    0x383f8000, 0x38470000, 0x384e8000, 0x3854c000, 0x385b0000, 0x38604000, 0x38658000, 0x3869c000,
    0x386d4000, 0x38708000, 0x3872c000, 0x38744000, 0x38750000, 0x38750000, 0x38740000, 0x38724000,
    0x38700000, 0x386c0000, 0x3867c000, 0x3862c000, 0x385c8000, 0x38558000, 0x384d8000, 0x38450000,
    0x383b0000, 0x38318000, 0x38260000, 0x381a0000, 0x380d8000, 0x38010000, 0x37e70000, 0x37cb0000,
    0x37af0000, 0x37930000, 0x376c0000, 0x37340000, 0x37020000, 0x36a00000, 0x36180000, 0x35000000
];

fn asin_const(i: usize) -> f32 {
    f32::from_bits(ASIN[i])
}

/// The polynomial and table index the inverse sine and cosine share, for `x` in `0 < x < 1`.
fn asin_core(x: f32) -> (f32, usize) {
    let index = cvtt(f32::from_bits(x.to_bits().wrapping_add(ASIN[7]))) as usize;
    let mut p = asin_const(0) * x;
    p += asin_const(1);
    p *= x;
    p -= asin_const(2);
    p *= x;
    (p, index)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), internal/utility.cpp: ASin.
/// `ASin(x)`: 0 (and NaN) as they are; a magnitude of 1 or more is pi/2 with the sign.
fn asin(x: f32) -> f32 {
    if x == 0.0 || x.is_nan() {
        return x;
    }
    let sign = x.to_bits() & SIGN;
    let x = f32::from_bits(x.to_bits() ^ sign);
    let one = asin_const(5);
    if x >= one {
        return f32::from_bits(ASIN[3] ^ sign);
    }
    let (mut p, index) = asin_core(x);
    p += asin_const(3);
    let root = (one - x).sqrt();
    let r = asin_const(3) - root * p + f32::from_bits(ASIN[8 + index]);
    f32::from_bits(r.to_bits() ^ sign)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), internal/utility.cpp: ACos.
/// `ACos(x)`: 0 (and NaN) give pi/2; a magnitude of 1 or more gives pi for a negative `x`, 0
/// for a positive one.
fn acos(x: f32) -> f32 {
    if x == 0.0 || x.is_nan() {
        return asin_const(3);
    }
    let sign = x.to_bits() & SIGN;
    let x = f32::from_bits(x.to_bits() ^ sign);
    let one = asin_const(5);
    if x >= one {
        return if sign != 0 { asin_const(6) } else { 0.0 };
    }
    let (mut p, index) = asin_core(x);
    p += asin_const(3);
    let root = (one - x).sqrt();
    let r = root * p - f32::from_bits(ASIN[8 + index]);
    if sign != 0 {
        flip(r) + asin_const(6)
    } else {
        r
    }
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), internal/utility.cpp: ATan2.
/// `ATan2(y, x)`: a zero `y` (or NaN) answers `y` for a `x` at or above 0, else pi; a zero `x`
/// answers pi/2 with `y`'s sign; otherwise the polynomial of the smaller over the larger
/// magnitude, turned into the right octant.
fn atan2(y: f32, x: f32) -> f32 {
    const C: [f32; 6] = [
        f32::from_bits(0xbc5c_dd30),
        f32::from_bits(0x3d6b_6d55),
        f32::from_bits(0x3df8_4c31),
        f32::from_bits(0x3e48_54c9),
        f32::from_bits(0x3eaa_7e45),
        f32::from_bits(0x3f7f_ffb7),
    ];
    if y == 0.0 || y.is_nan() {
        return if x >= 0.0 { y } else { PI };
    }
    if x == 0.0 || x.is_nan() {
        return f32::from_bits((y.to_bits() & SIGN) | PI_D2.to_bits());
    }
    let (ay, ax) = (y.abs(), x.abs());
    // `maxss`/`minss` answer their second operand when the two are equal or unordered.
    let max = if ay > ax { ay } else { ax };
    let min = if ax < ay { ax } else { ay };
    let r = min / max;
    let s = r * r;
    let mut p = s * C[0];
    p += C[1];
    p *= s;
    p -= C[2];
    p *= s;
    p += C[3];
    p *= s;
    p -= C[4];
    p *= s;
    p += C[5];
    let mut t = r * p;
    if ay > ax {
        t = flip(t) + PI_D2;
    }
    if x < 0.0 {
        t = flip(t) + PI;
    }
    f32::from_bits((y.to_bits() & SIGN) ^ t.to_bits())
}

fn one_float(e: &mut Engine, w: &[u32], f: fn(f32) -> f64) -> Ret {
    let a = CommandArgs::new(w);
    let read = Read::of(e, &a);
    if read.ok {
        let v = f(read.float(0, 0.0));
        a.set_result(e, v);
    }
    handled(true)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_ln/ln_fn_math.h: Cmd_fsqrt_Execute.
/// The square root of the value, a negative one counted as 0 (`maxsd` with 0, then `sqrtsd`).
fn fsqrt(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    let read = Read::of(e, &a);
    if read.ok {
        let value = read.args.first().and_then(Arg::double).unwrap_or(0.0);
        let clamped = if 0.0 > value { 0.0 } else { value };
        a.set_result(e, clamped.sqrt());
    }
    handled(true)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_ln/ln_fn_math.h: Cmd_fSin_Execute.
fn f_sin(e: &mut Engine, w: &[u32]) -> Ret {
    one_float(e, w, |v| f64::from(sin(v * PI_D180)))
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_ln/ln_fn_math.h: Cmd_fCos_Execute.
fn f_cos(e: &mut Engine, w: &[u32]) -> Ret {
    one_float(e, w, |v| f64::from(cos(v * PI_D180)))
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_ln/ln_fn_math.h: Cmd_fTan_Execute.
fn f_tan(e: &mut Engine, w: &[u32]) -> Ret {
    one_float(e, w, |v| f64::from(tan(v * PI_D180)))
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_ln/ln_fn_math.h: Cmd_fAsin_Execute.
/// The result is the `float` answer times `Dbl180dPI` in `double`.
fn f_asin(e: &mut Engine, w: &[u32]) -> Ret {
    one_float(e, w, |v| f64::from(asin(v)) * D180_DPI)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_ln/ln_fn_math.h: Cmd_fAcos_Execute.
fn f_acos(e: &mut Engine, w: &[u32]) -> Ret {
    one_float(e, w, |v| f64::from(acos(v)) * D180_DPI)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_ln/ln_fn_math.h: Cmd_fAtan_Execute.
/// `ATan(x) = ATan2(x, 1)`.
fn f_atan(e: &mut Engine, w: &[u32]) -> Ret {
    one_float(e, w, |v| f64::from(atan2(v, 1.0)) * D180_DPI)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_ln/ln_fn_math.h: Cmd_fAtan2_Execute.
fn f_atan2(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    let read = Read::of(e, &a);
    if read.ok {
        let (y, x) = (read.float(0, 0.0), read.float(1, 0.0));
        a.set_result(e, f64::from(atan2(y, x)) * D180_DPI);
    }
    handled(true)
}

#[cfg(test)]
#[path = "ln_fn_math_tests.rs"]
mod tests;
