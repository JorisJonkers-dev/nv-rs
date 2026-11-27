//! Native float maths for typed code that runs on main and on the AI threads (ADR-0014's float rule):
//! the running thread's x87 precision decides what a step of the exe's code rounds to, so a function
//! is written once over [`Real`] and run as `f32` on main (24 bits: every operation rounds to a float)
//! or as `f64` on the AI threads (53 bits: every operation rounds to a double, and only the exe's float
//! stores round to a float). Each is the exe's arithmetic on its thread, without the x87 helpers.
//!
//! This is the one shared place for that rule: typed code of any family that needs the per-thread
//! precision choice uses these, so there is a single module for it. Keep it small.

use std::ops::{Add, Div, Mul, Neg, Sub};

/// Whether the running game thread computes at a float's precision: its x87 control word's precision
/// bits are 00 (main, once Direct3D set them).
#[inline]
pub(crate) fn float_precision() -> bool {
    engine_core::x87::control_word() & 0x0300 == 0
}

/// A float store of a double (`FSTP dword`): rounded to the nearest float.
#[allow(clippy::cast_possible_truncation)]
#[inline]
pub(crate) fn to_float(value: f64) -> f32 {
    value as f32
}

/// The type one function computes in on the running thread ([`float_precision`]): [`Single`] or
/// [`Double`].
///
/// An operation on a NaN gives the NaN of the x87 (`engine_core::x87`, Intel SDM vol. 1 table 4-7:
/// the one NaN, quieted; of two the quiet one, else the larger significand), not the one the host
/// instruction or the optimiser's operand order gives, so a payload is the exe's at every
/// optimisation level (nv-rs#1009). That is why the two precisions are types of their own and not
/// `f32` and `f64`, whose operators keep the first operand's NaN.
pub(crate) trait Real:
    Copy
    + PartialOrd
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<Output = Self>
    + Div<Output = Self>
    + Neg<Output = Self>
{
    const ZERO: Self;
    const ONE: Self;
    /// `FLD dword` of a float's bits (or one as a memory operand).
    fn load(bits: u32) -> Self;
    /// `FSTP dword`: rounded to a float, as its bits.
    fn store(self) -> u32;
    /// `FSQRT` at this precision.
    fn sqrt(self) -> Self;
    /// The value as a double, for a callee that takes one.
    fn wide(self) -> f64;
    /// A callee's double result at this precision (rounded to a float on main).
    fn narrow(value: f64) -> Self;
}

/// The NaN an x87 operation on `a` and `b` gives, when either is a NaN.
#[inline]
fn x87_nan(a: f64, b: f64) -> Option<f64> {
    (a.is_nan() || b.is_nan()).then(|| engine_core::x87::x87_add(a, b))
}

/// Main's precision: every operation rounds to a float.
#[derive(Clone, Copy, PartialEq, PartialOrd, Debug)]
pub(crate) struct Single(pub(crate) f32);

/// The AI threads' precision: every operation rounds to a double.
#[derive(Clone, Copy, PartialEq, PartialOrd, Debug)]
pub(crate) struct Double(pub(crate) f64);

macro_rules! operator {
    ($ty:ident, $inner:ty, $trait:ident, $method:ident, $op:tt) => {
        impl $trait for $ty {
            type Output = $ty;
            #[inline]
            fn $method(self, other: $ty) -> $ty {
                $ty::with_x87_nan(self.0, other.0, |a, b| a $op b)
            }
        }
    };
}

macro_rules! precision {
    ($ty:ident, $inner:ty, $from_wide:expr) => {
        impl $ty {
            /// `f(a, b)`, or the x87's NaN when an operand is one.
            #[inline]
            fn with_x87_nan(a: $inner, b: $inner, f: impl FnOnce($inner, $inner) -> $inner) -> $ty {
                match x87_nan(f64::from(a), f64::from(b)) {
                    Some(nan) => $ty($from_wide(nan)),
                    None => $ty(f(a, b)),
                }
            }
        }
        operator!($ty, $inner, Add, add, +);
        operator!($ty, $inner, Sub, sub, -);
        operator!($ty, $inner, Mul, mul, *);
        operator!($ty, $inner, Div, div, /);
        impl Neg for $ty {
            type Output = $ty;
            #[inline]
            fn neg(self) -> $ty {
                $ty(-self.0)
            }
        }
    };
}

precision!(Single, f32, |nan: f64| f32::from_bits(
    engine_core::x87::x87_store32(nan)
));
precision!(Double, f64, |nan: f64| nan);

impl Real for Single {
    const ZERO: Single = Single(0.0);
    const ONE: Single = Single(1.0);
    #[inline]
    fn load(bits: u32) -> Single {
        Single(f32::from_bits(bits))
    }
    #[inline]
    fn store(self) -> u32 {
        self.0.to_bits()
    }
    #[inline]
    fn sqrt(self) -> Single {
        Single(f32::sqrt(self.0))
    }
    #[inline]
    fn wide(self) -> f64 {
        f64::from(self.0)
    }
    #[inline]
    fn narrow(value: f64) -> Single {
        Single(to_float(value))
    }
}

impl Real for Double {
    const ZERO: Double = Double(0.0);
    const ONE: Double = Double(1.0);
    #[inline]
    fn load(bits: u32) -> Double {
        Double(f64::from(f32::from_bits(bits)))
    }
    #[inline]
    fn store(self) -> u32 {
        to_float(self.0).to_bits()
    }
    #[inline]
    fn sqrt(self) -> Double {
        Double(f64::sqrt(self.0))
    }
    #[inline]
    fn wide(self) -> f64 {
        self.0
    }
    #[inline]
    fn narrow(value: f64) -> Double {
        Double(value)
    }
}
