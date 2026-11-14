//! `functions_ln/ln_fn_math.h` (JIP LN NVSE): the degree-based trigonometry, `fsqrt` and the two
//! distances. All 10 of its commands.
//!
//! JIP does not call the C library for its angles: `Cos`, `Tan`, `ASin`, `ACos` and `ATan2`
//! (`internal/utility.cpp`) are polynomial approximations written in SSE, in single precision.
//! They are translated here operation by operation (every `f32` step rounds as the instruction
//! does), so the answers match JIP's bit for bit, not the C library's.

use super::{DBL_180_D_PI, FLT_PI_D_180};
use engine_core::prelude::*;
use nvse::commands::Read;
use nvse::{handled, CommandArgs, EvalArgs};

#[rustfmt::skip]
pub static COMMANDS: super::Handlers = &[
    (0x22f0, fsqrt),
    (0x22fd, fsin),
    (0x22fe, fcos),
    (0x230b, fatan),
    (0x230f, fasin),
    (0x2310, facos),
    (0x2893, ftan),
    (0x2894, fatan2),
];

/// The two conditions: each opcode with its execute and its eval.
#[rustfmt::skip]
pub static CONDITIONS: super::Conditions = &[
    (0x22e0, get_distance_2d, get_distance_2d_eval),
    (0x22e1, get_distance_3d, get_distance_3d_eval),
];

/// `FltPId2`, `FltPI`, `FltPIx2` (`internal/utility.h`), the floats the packed constants hold.
#[allow(clippy::excessive_precision)]
const FLT_PI_D_2: f32 = 1.570_796_371;
#[allow(clippy::excessive_precision)]
const FLT_PI: f32 = 3.141_592_741;
#[allow(clippy::excessive_precision)]
const FLT_PI_X_2: f32 = 6.283_185_482;
/// `FLT_EPSILON`, `PS_Epsilon`.
const EPSILON: f32 = f32::EPSILON;
/// `kFlt2dPI` (`0x3F22F983`).
fn flt_2_d_pi() -> f32 {
    f32::from_bits(0x3f22_f983)
}
/// `kCosConsts`, the polynomial of `Cos`.
const COS_BITS: [u32; 5] = [
    0x37c2_3ab1,
    0x3ab5_9551,
    0x3d2a_a76f,
    0x3eff_ffe0,
    0x3f7f_ffff,
];
/// `kATanConsts`, the polynomial of `ATan2`.
const ATAN_BITS: [u32; 6] = [
    0xbc5c_dd30,
    0x3d6b_6d55,
    0x3df8_4c31,
    0x3e48_54c9,
    0x3eaa_7e45,
    0x3f7f_ffb7,
];
/// The first eight words of `kASinConsts`: three polynomial terms and pi/2, then `2^-23`
/// (`0x34000000`, unused), 1.0, pi and the exponent step `0x03800000` (a multiplication by 128
/// done on the bits).
const ASIN_HEAD: [u32; 8] = [
    0xbc99_6e30,
    0x3d98_1627,
    0x3e59_3484,
    0x3fc9_0fdb,
    0x3400_0000,
    0x3f80_0000,
    0x4049_0fdb,
    0x0380_0000,
];
/// The rest of `kASinConsts` (words 8 to 135): the correction added by the table's slot, the
/// argument times 128 truncated.
#[rustfmt::skip]
const ASIN_TABLE: [u32; 128] = [
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
    0x37af0000, 0x37930000, 0x376c0000, 0x37340000, 0x37020000, 0x36a00000, 0x36180000, 0x35000000,
];

fn bits(v: f32) -> u32 {
    v.to_bits()
}

/// `maxss dest, src`: `dest` when it is greater, else `src` (so a NaN gives `src`).
fn maxss(dest: f32, src: f32) -> f32 {
    if dest > src {
        dest
    } else {
        src
    }
}

/// `minss dest, src`: `dest` when it is smaller, else `src`.
fn minss(dest: f32, src: f32) -> f32 {
    if dest < src {
        dest
    } else {
        src
    }
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), internal/utility.cpp: Cos_V3, the two lanes
// GetSinCos uses (the third and fourth lanes hold 0 and stay out of the answer).
/// `Cos_V3` on two angles: both reduced together (a lane past three quarters of a turn makes
/// every lane subtract its whole turns), then each by its own quadrant. No test for a zero
/// angle: 0 gives `kCosConsts`' last word, not 1.
fn cos_v3(angles: [f32; 2]) -> [f32; 2] {
    let a = angles.map(f32::abs);
    // `cvttps2dq`: the integer indefinite for NaN and past the range.
    let mut q = a.map(|v| super::cvttss2si(flt_2_d_pi() * v));
    let mut a = a;
    if q.iter().any(|&v| v > 3) {
        let turns = q.map(|v| ((v as u32) >> 2) as i32 as f32);
        for i in 0..2 {
            a[i] -= turns[i] * FLT_PI_X_2;
            q[i] &= 3;
        }
    }
    let mut out = [0.0; 2];
    for i in 0..2 {
        let third = q[i] > 2;
        let flip = !third && q[i] > 0;
        let sub = if third {
            FLT_PI_X_2
        } else if flip {
            FLT_PI
        } else {
            0.0
        };
        let x = a[i] - sub;
        let x2 = x * x;
        let mut t = f32::from_bits(COS_BITS[0]) * x2;
        t -= f32::from_bits(COS_BITS[1]);
        t *= x2;
        t += f32::from_bits(COS_BITS[2]);
        t *= x2;
        t -= f32::from_bits(COS_BITS[3]);
        let r = x2 * t + f32::from_bits(COS_BITS[4]);
        out[i] = if flip { -r } else { r };
    }
    out
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), internal/utility.cpp: Cos.
/// `Cos` (radians): 1 for an angle whose magnitude reduces to 0 or less, or is NaN.
pub fn cos(angle: f32) -> f32 {
    let mut a = angle.abs();
    let mut q = super::cvttss2si(flt_2_d_pi() * a);
    let mut flip = false;
    let mut sub = FLT_PI_X_2;
    if q > 3 {
        let turns = ((q as u32) >> 2) as i32 as f32;
        q &= 3;
        a -= turns * FLT_PI_X_2;
    }
    // `comiss a, 0; jbe`: 0 or less, and unordered.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    let not_positive = !(a > 0.0);
    if not_positive {
        return 1.0;
    }
    if q != 0 {
        // `test al, al; jp`: three has an even number of bits and keeps the whole turn.
        if q != 3 {
            sub = FLT_PI;
            flip = true;
        }
        a -= sub;
    }
    let x2 = a * a;
    let mut t = x2 * f32::from_bits(COS_BITS[0]);
    t -= f32::from_bits(COS_BITS[1]);
    t *= x2;
    t += f32::from_bits(COS_BITS[2]);
    t *= x2;
    t -= f32::from_bits(COS_BITS[3]);
    let r = x2 * t + f32::from_bits(COS_BITS[4]);
    if flip {
        -r
    } else {
        r
    }
}

/// `Sin` (radians): `Cos(FltPId2 - angle)`.
pub fn sin(angle: f32) -> f32 {
    cos(FLT_PI_D_2 - angle)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), internal/utility.cpp: Tan (through GetSinCos).
/// `Tan` (radians): the sine over the cosine, 0 when the cosine's magnitude is under
/// `FLT_EPSILON` (or NaN).
pub fn tan(angle: f32) -> f32 {
    let [s, c] = cos_v3([FLT_PI_D_2 - angle, angle]);
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    let small = !(c.abs() >= EPSILON);
    if small {
        return 0.0;
    }
    s / c
}

/// A word of `kASinConsts` as a float.
fn asin_word(i: usize) -> f32 {
    f32::from_bits(if i < 8 {
        ASIN_HEAD[i]
    } else {
        ASIN_TABLE[i - 8]
    })
}

/// `kASinConsts[eax*4+0x20]` for the slot `eax` of an argument below 1: the argument's bits plus
/// `0x03800000` read as a float and truncated (the argument times 128).
fn asin_slot(x: f32) -> usize {
    let scaled = f32::from_bits(bits(x).wrapping_add(ASIN_HEAD[7]));
    super::cvttss2si(scaled) as usize + 8
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), internal/utility.cpp: ASin.
/// `ASin` (radians): 0 (and NaN) as it is, a magnitude of 1 or more is plus or minus pi/2.
pub fn asin(x: f32) -> f32 {
    // `comiss x, 0; jz`: equal to 0, or unordered.
    if x == 0.0 || x.is_nan() {
        return x;
    }
    let sign = bits(x) & 0x8000_0000;
    let a = f32::from_bits(bits(x) ^ sign);
    if a >= 1.0 {
        return f32::from_bits(bits(asin_word(3)) ^ sign);
    }
    let slot = asin_slot(a);
    let mut t = asin_word(0) * a;
    t += asin_word(1);
    t *= a;
    t -= asin_word(2);
    t *= a;
    let root = (1.0 - a).sqrt();
    t += asin_word(3);
    let r = asin_word(3) - root * t + asin_word(slot);
    f32::from_bits(bits(r) ^ sign)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), internal/utility.cpp: ACos.
/// `ACos` (radians): pi/2 for 0 (and NaN), pi or 0 for a magnitude of 1 or more by its sign.
pub fn acos(x: f32) -> f32 {
    if x == 0.0 || x.is_nan() {
        return asin_word(3);
    }
    let sign = bits(x) & 0x8000_0000;
    let a = f32::from_bits(bits(x) ^ sign);
    if a >= 1.0 {
        return if sign != 0 { asin_word(6) } else { 0.0 };
    }
    let slot = asin_slot(a);
    let mut t = asin_word(0) * a;
    t += asin_word(1);
    t *= a;
    t -= asin_word(2);
    t *= a;
    t += asin_word(3);
    let root = (1.0 - a).sqrt();
    let r = root * t - asin_word(slot);
    if sign == 0 {
        r
    } else {
        f32::from_bits(bits(r) ^ sign) + asin_word(6)
    }
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), internal/utility.cpp: ATan2.
/// `ATan2(y, x)` (radians): the ratio of the smaller to the larger magnitude through the
/// polynomial, then moved to the right octant; a zero `y` answers `y` or pi, a zero `x` plus or
/// minus pi/2, and a NaN is taken as a zero.
pub fn atan2(y: f32, x: f32) -> f32 {
    let sign_bit = 0x8000_0000;
    if y == 0.0 || y.is_nan() {
        // `comiss x, 0; jnb`: x is 0 or more.
        return if x >= 0.0 { y } else { FLT_PI };
    }
    if x == 0.0 || x.is_nan() {
        return f32::from_bits((bits(y) & sign_bit) | bits(FLT_PI_D_2));
    }
    let (ay, ax) = (y.abs(), x.abs());
    let mx = maxss(ay, ax);
    let mn = minss(ax, ay);
    let r = mn / mx;
    let r2 = r * r;
    let mut t = r2 * f32::from_bits(ATAN_BITS[0]);
    t += f32::from_bits(ATAN_BITS[1]);
    t *= r2;
    t -= f32::from_bits(ATAN_BITS[2]);
    t *= r2;
    t += f32::from_bits(ATAN_BITS[3]);
    t *= r2;
    t -= f32::from_bits(ATAN_BITS[4]);
    t *= r2;
    t += f32::from_bits(ATAN_BITS[5]);
    let mut a = r * t;
    if ay > ax {
        a = f32::from_bits(bits(a) ^ sign_bit) + FLT_PI_D_2;
    }
    if x < 0.0 {
        a = f32::from_bits(bits(a) ^ sign_bit) + FLT_PI;
    }
    f32::from_bits((bits(y) & sign_bit) ^ bits(a))
}

/// `ATan(x)`: `ATan2(x, 1)`.
pub fn atan(x: f32) -> f32 {
    atan2(x, 1.0)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_ln/ln_fn_math.h: Cmd_fsqrt_Execute.
/// The square root of the value, 0 for a negative one (`maxsd` with 0 first).
fn fsqrt(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    let read = Read::of(e, &a);
    if read.ok {
        let v = read.args.first().and_then(|x| x.double()).unwrap_or(0.0);
        let m = if 0.0 > v { 0.0 } else { v };
        a.set_result(e, m.sqrt());
    }
    handled(true)
}

/// A command that reads one float and answers a function of it.
fn float_command(e: &mut Engine, w: &[u32], f: impl Fn(f32) -> f64) -> Ret {
    let a = CommandArgs::new(w);
    let read = Read::of(e, &a);
    if read.ok {
        a.set_result(e, f(read.float(0, 0.0)));
    }
    handled(true)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_ln/ln_fn_math.h: Cmd_fSin_Execute.
fn fsin(e: &mut Engine, w: &[u32]) -> Ret {
    float_command(e, w, |v| f64::from(sin(v * FLT_PI_D_180)))
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_ln/ln_fn_math.h: Cmd_fCos_Execute.
fn fcos(e: &mut Engine, w: &[u32]) -> Ret {
    float_command(e, w, |v| f64::from(cos(v * FLT_PI_D_180)))
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_ln/ln_fn_math.h: Cmd_fTan_Execute.
fn ftan(e: &mut Engine, w: &[u32]) -> Ret {
    float_command(e, w, |v| f64::from(tan(v * FLT_PI_D_180)))
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_ln/ln_fn_math.h: Cmd_fAsin_Execute.
/// The angle in degrees: the float answer times the double `Dbl180dPI`.
fn fasin(e: &mut Engine, w: &[u32]) -> Ret {
    float_command(e, w, |v| f64::from(asin(v)) * DBL_180_D_PI)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_ln/ln_fn_math.h: Cmd_fAcos_Execute.
fn facos(e: &mut Engine, w: &[u32]) -> Ret {
    float_command(e, w, |v| f64::from(acos(v)) * DBL_180_D_PI)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_ln/ln_fn_math.h: Cmd_fAtan_Execute.
fn fatan(e: &mut Engine, w: &[u32]) -> Ret {
    float_command(e, w, |v| f64::from(atan(v)) * DBL_180_D_PI)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_ln/ln_fn_math.h: Cmd_fAtan2_Execute.
/// `ATan2(y, x)` in degrees; the first argument is y.
fn fatan2(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    let read = Read::of(e, &a);
    if read.ok {
        let (y, x) = (read.float(0, 0.0), read.float(1, 0.0));
        a.set_result(e, f64::from(atan2(y, x)) * DBL_180_D_PI);
    }
    handled(true)
}

// ---- distances ----

/// `TESObjectREFR::position` (+0x30), `parentCell` (+0x40), the extra data list (+0x44).
const REFR_POS: u32 = 0x30;
const REFR_PARENT_CELL: u32 = 0x40;
const REFR_EXTRA: u32 = 0x44;
/// `TESObjectCELL::worldSpace` (+0xC0).
const CELL_WORLD: u32 = 0xc0;
/// `kXData_ExtraPersistentCell` (JIP LN NVSE), the cell at +0xC of the extra.
const EXTRA_PERSISTENT_CELL: u32 = 0x0c;
const EXTRA_VALUE: u32 = 0xc;
/// The game's `BaseExtraList::GetByType` (`00410220`, `thiscall`: the list, the type).
const EXTRA_GET_BY_TYPE: u32 = 0x0041_0220;

// Translated from JIP LN NVSE 57.30 (5a30ac4), nvse/GameObjects.cpp:
// TESObjectREFR::GetInSameCellOrWorld (asm).
/// Whether two references are in the same cell, or in cells of one worldspace; a reference with
/// no cell of its own uses its persistent cell (none: false), and a cell with no worldspace
/// matches only the same cell.
pub fn same_cell_or_world(e: &mut Engine, a: u32, b: u32) -> bool {
    let cell_of = |e: &mut Engine, r: u32| {
        let cell = e.mem.u32(r + REFR_PARENT_CELL);
        if cell != 0 {
            return Some(cell);
        }
        let x = e
            .call(
                EXTRA_GET_BY_TYPE,
                &args![r + REFR_EXTRA, EXTRA_PERSISTENT_CELL],
            )
            .u32();
        (x != 0).then(|| e.mem.u32(x + EXTRA_VALUE))
    };
    let Some(first) = cell_of(e, a) else {
        return false;
    };
    let Some(second) = cell_of(e, b) else {
        return false;
    };
    if first == second {
        return true;
    }
    let world = e.mem.u32(first + CELL_WORLD);
    world != 0 && world == e.mem.u32(second + CELL_WORLD)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), internal/Ni_types.cpp: Point2Distance and
// Point3Distance.
/// The distance of two points as JIP's SSE code makes it (`dims` 2 or 3): the squares added
/// pairwise, the hardware's reciprocal square root estimate and one Newton step; 0 and NaN stay.
fn point_distance(e: &Engine, a: u32, b: u32, dims: u32) -> f32 {
    let d = |i: u32| e.mem.f32(a + 4 * i) - e.mem.f32(b + 4 * i);
    let mut s = d(0) * d(0) + d(1) * d(1);
    if dims == 3 {
        s += d(2) * d(2);
    }
    if s == 0.0 || s.is_nan() {
        return s;
    }
    let r = engine_core::x87::rsqrt_estimate(s);
    let t = s * r * r;
    let k = (3.0 - t) * r * 0.5;
    s * k
}

/// The distance of two references: the largest float for another cell and worldspace.
fn distance(e: &mut Engine, this: u32, other: u32, dims: u32) -> f32 {
    if !same_cell_or_world(e, this, other) {
        return f32::MAX;
    }
    point_distance(e, this + REFR_POS, other + REFR_POS, dims)
}

fn distance_command(e: &mut Engine, w: &[u32], dims: u32) -> Ret {
    let a = CommandArgs::new(w);
    let read = Read::of(e, &a);
    let d = if read.ok {
        distance(e, a.this_obj, read.form(0), dims)
    } else {
        f32::MAX
    };
    a.set_result(e, f64::from(d));
    handled(true)
}

fn distance_eval(e: &mut Engine, w: &[u32], dims: u32) -> Ret {
    let a = EvalArgs::new(w);
    let d = distance(e, a.this_obj, a.arg1, dims);
    e.mem.set_f64(a.result, f64::from(d));
    handled(true)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_ln/ln_fn_math.h:
// Cmd_GetDistance2D_Execute.
fn get_distance_2d(e: &mut Engine, w: &[u32]) -> Ret {
    distance_command(e, w, 2)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_ln/ln_fn_math.h:
// Cmd_GetDistance2D_Eval.
fn get_distance_2d_eval(e: &mut Engine, w: &[u32]) -> Ret {
    distance_eval(e, w, 2)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_ln/ln_fn_math.h:
// Cmd_GetDistance3D_Execute.
fn get_distance_3d(e: &mut Engine, w: &[u32]) -> Ret {
    distance_command(e, w, 3)
}

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_ln/ln_fn_math.h:
// Cmd_GetDistance3D_Eval.
fn get_distance_3d_eval(e: &mut Engine, w: &[u32]) -> Ret {
    distance_eval(e, w, 3)
}

#[cfg(test)]
#[path = "ln_fn_math_tests.rs"]
mod tests;
