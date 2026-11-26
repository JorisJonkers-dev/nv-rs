//! The D3DX import thunks the typed families call (group E): each is the exe's `JMP dword ptr [slot]` to a
//! `d3dx9_38.dll` function, which the platform layer answers (`engine-platform`'s D3DX, bound to the
//! import slot by its name). Typed, a typed caller reaches the import in one call instead of two (the
//! thunk's own call is gone); the platform's function stays behind the slot, since part crates do not
//! depend on `engine-platform`. Every word goes to the import unchanged and its answer comes back
//! (`stdcall`: the import pops them). `D3DXMatrixMultiply` is the exception: its typed form works the
//! product out here in the measured order, so the draw path pays no by-address call for it at all.

use super::FAMILY;
use engine_core::abi::OneWord;
use engine_core::ptr::Ptr;
use engine_core::{args, typed_call, Engine};

/// IEEE single words with the SSE instructions' NaN rules, as the measured DLL's arithmetic uses
/// them: a NaN operand answers its first NaN quieted, an invalid operation the default NaN. This is
/// the part of `engine-platform`'s `d3dx::multiply` a typed caller cannot go through by address.
mod single {
    fn is_nan(x: u32) -> bool {
        x & 0x7fff_ffff > 0x7f80_0000
    }

    fn op(x: u32, y: u32, f: fn(f32, f32) -> f32) -> u32 {
        if is_nan(x) {
            return x | 0x0040_0000;
        }
        if is_nan(y) {
            return y | 0x0040_0000;
        }
        let r = f(f32::from_bits(x), f32::from_bits(y));
        if r.is_nan() {
            0xffc0_0000
        } else {
            r.to_bits()
        }
    }

    pub fn mul(x: u32, y: u32) -> u32 {
        op(x, y, |a, b| a * b)
    }

    pub fn add(x: u32, y: u32) -> u32 {
        op(x, y, |a, b| a + b)
    }
}

/// The sixteen words of a `D3DXMATRIX` at `at` (row major), read before `out` is written.
fn matrix(e: &Engine, at: u32) -> [u32; 16] {
    let mut words = [0u32; 16];
    let mut at = at;
    for word in &mut words {
        *word = e.mem.u32(at);
        at = at.wrapping_add(4);
    }
    words
}

/// `M1 * M2` in the measured DLL's operation order: rows 0, 1 and 2 multiply `m1[i][k] * m2[k][j]`,
/// row 3 `m2[k][j] * m1[i][k]` (which NaN wins); rows 0 and 3 add `(p0 + p1) + (p2 + p3)`, rows 1
/// and 2 `((p0 + p1) + p2) + p3`. Worked out in plain single precision first; a NaN anywhere in a
/// word's working sends it to `multiply_exact`, where the SSE NaN rules decide the answer. The same
/// rules and order as `engine-platform/src/d3dx.rs`, measured there against the player's DLL.
pub fn multiply(a: &[u32; 16], b: &[u32; 16]) -> [u32; 16] {
    let f = |w: u32| f32::from_bits(w);
    let mut out = [0u32; 16];
    for i in 0..4 {
        for j in 0..4 {
            let p: [f32; 4] = std::array::from_fn(|k| f(a[4 * i + k]) * f(b[4 * k + j]));
            let sum = if i == 0 || i == 3 {
                (p[0] + p[1]) + (p[2] + p[3])
            } else {
                ((p[0] + p[1]) + p[2]) + p[3]
            };
            out[4 * i + j] = sum.to_bits();
        }
    }
    if out.iter().any(|&w| w & 0x7fff_ffff > 0x7f80_0000) {
        return multiply_exact(a, b);
    }
    out
}

/// [`multiply`] with the SSE NaN rules at each product and sum.
fn multiply_exact(a: &[u32; 16], b: &[u32; 16]) -> [u32; 16] {
    let mut out = [0u32; 16];
    for i in 0..4 {
        for j in 0..4 {
            let p: [u32; 4] = std::array::from_fn(|k| {
                if i == 3 {
                    single::mul(b[4 * k + j], a[4 * i + k])
                } else {
                    single::mul(a[4 * i + k], b[4 * k + j])
                }
            });
            out[4 * i + j] = if i == 0 || i == 3 {
                single::add(single::add(p[0], p[1]), single::add(p[2], p[3]))
            } else {
                single::add(single::add(single::add(p[0], p[1]), p[2]), p[3])
            };
        }
    }
    out
}

/// The import slots: `D3DXMatrixMultiplyTranspose`, `D3DXVec3Normalize`, `D3DXVec3TransformNormal`,
/// `D3DXVec3TransformCoord`, `D3DXMatrixTranspose`, `D3DXMatrixInverse`, `D3DXPlaneNormalize` and
/// `D3DXPlaneTransform` (the platform layer's names for them). `D3DXMatrixMultiply`'s slot is used
/// by the translation only: the typed function works it out in place.
const MATRIX_MULTIPLY_TRANSPOSE: u32 = 0x00fd_f410;
const VEC3_NORMALIZE: u32 = 0x00fd_f41c;
const VEC3_TRANSFORM_NORMAL: u32 = 0x00fd_f420;
const VEC3_TRANSFORM_COORD: u32 = 0x00fd_f414;
const MATRIX_TRANSPOSE: u32 = 0x00fd_f42c;
const MATRIX_INVERSE: u32 = 0x00fd_f428;
const PLANE_NORMALIZE: u32 = 0x00fd_f430;
const PLANE_TRANSFORM: u32 = 0x00fd_f418;

// Typed from 00ee6dfe
/// `D3DXMatrixMultiplyTranspose(out, m1, m2)`'s thunk: answers the import's answer.
pub fn matrix_multiply_transpose(e: &mut Engine, out: Ptr, m1: Ptr, m2: Ptr) -> Ptr {
    e.call(MATRIX_MULTIPLY_TRANSPOSE, &args![out, m1, m2]).ptr()
}

// Typed from 00ee6dec
/// `D3DXVec3Normalize(out, v)`'s thunk.
pub fn vec3_normalize(e: &mut Engine, out: Ptr, v: Ptr) -> Ptr {
    e.call(VEC3_NORMALIZE, &args![out, v]).ptr()
}

// Typed from 00ee6df2
/// `D3DXVec3TransformNormal(out, v, m)`'s thunk.
pub fn vec3_transform_normal(e: &mut Engine, out: Ptr, v: Ptr, m: Ptr) -> Ptr {
    e.call(VEC3_TRANSFORM_NORMAL, &args![out, v, m]).ptr()
}

// Typed from 00ee6df8
/// `D3DXVec3TransformCoord(out, v, m)`'s thunk.
pub fn vec3_transform_coord(e: &mut Engine, out: Ptr, v: Ptr, m: Ptr) -> Ptr {
    e.call(VEC3_TRANSFORM_COORD, &args![out, v, m]).ptr()
}

// Typed from 00ee6dd4
/// `D3DXMatrixTranspose(out, m)`'s thunk.
pub fn matrix_transpose(e: &mut Engine, out: Ptr, m: Ptr) -> Ptr {
    e.call(MATRIX_TRANSPOSE, &args![out, m]).ptr()
}

// Typed from 00ee6dda
/// `D3DXMatrixInverse(out, determinant, m)`'s thunk (the determinant's word may be 0).
pub fn matrix_inverse(e: &mut Engine, out: Ptr, determinant: u32, m: Ptr) -> Ptr {
    e.call(MATRIX_INVERSE, &args![out, determinant, m]).ptr()
}

// Typed from 00ee6de0
/// `D3DXMatrixMultiply(out, m1, m2)`: `out = m1 * m2` in [`multiply`]'s measured order, both inputs
/// read before `out` is written (so `out` may be either input), answering `out`. The translation
/// calls the import slot with the same words and the same arithmetic; typed callers go straight here.
pub fn matrix_multiply(e: &mut Engine, out: Ptr, m1: Ptr, m2: Ptr) -> Ptr {
    let a = matrix(e, m1.addr());
    let b = matrix(e, m2.addr());
    let mut at = out.addr();
    for word in multiply(&a, &b) {
        e.mem.set_u32(at, word);
        at = at.wrapping_add(4);
    }
    out
}

// Typed from 00ee6dce
/// `D3DXPlaneNormalize(out, plane)`'s thunk.
pub fn plane_normalize(e: &mut Engine, out: Ptr, plane: Ptr) -> Ptr {
    e.call(PLANE_NORMALIZE, &args![out, plane]).ptr()
}

// Typed from 00ee6dc8
/// `D3DXPlaneTransform(out, plane, m)`'s thunk.
pub fn plane_transform(e: &mut Engine, out: Ptr, plane: Ptr, m: Ptr) -> Ptr {
    e.call(PLANE_TRANSFORM, &args![out, plane, m]).ptr()
}

// ---- The typed families' calls: through the typed thunk directly while the draw family is on. ----

/// A word as the thunks' pointer parameter.
#[inline(always)]
fn word(value: impl OneWord) -> Ptr {
    Ptr::new(value.one_word())
}

/// `D3DXMatrixMultiplyTranspose(out, m1, m2)` through its thunk (`00ee6dfe`).
#[inline(always)]
pub(crate) fn call_matrix_multiply_transpose(
    e: &mut Engine,
    out: impl OneWord,
    m1: impl OneWord,
    m2: impl OneWord,
) {
    let (out, m1, m2) = (word(out), word(m1), word(m2));
    let _: Ptr = typed_call!(
        FAMILY,
        0x00ee_6dfe,
        matrix_multiply_transpose(e, out, m1, m2)
    );
}

/// `D3DXVec3Normalize(out, v)` through its thunk (`00ee6dec`).
#[inline(always)]
pub(crate) fn call_vec3_normalize(e: &mut Engine, out: impl OneWord, v: impl OneWord) {
    let (out, v) = (word(out), word(v));
    let _: Ptr = typed_call!(FAMILY, 0x00ee_6dec, vec3_normalize(e, out, v));
}

/// `D3DXVec3TransformNormal(out, v, m)` through its thunk (`00ee6df2`).
#[inline(always)]
pub(crate) fn call_vec3_transform_normal(
    e: &mut Engine,
    out: impl OneWord,
    v: impl OneWord,
    m: impl OneWord,
) {
    let (out, v, m) = (word(out), word(v), word(m));
    let _: Ptr = typed_call!(FAMILY, 0x00ee_6df2, vec3_transform_normal(e, out, v, m));
}

/// `D3DXVec3TransformCoord(out, v, m)` through its thunk (`00ee6df8`).
#[inline(always)]
pub(crate) fn call_vec3_transform_coord(
    e: &mut Engine,
    out: impl OneWord,
    v: impl OneWord,
    m: impl OneWord,
) {
    let (out, v, m) = (word(out), word(v), word(m));
    let _: Ptr = typed_call!(FAMILY, 0x00ee_6df8, vec3_transform_coord(e, out, v, m));
}

/// `D3DXMatrixTranspose(out, m)` through its thunk (`00ee6dd4`).
#[inline(always)]
pub(crate) fn call_matrix_transpose(e: &mut Engine, out: impl OneWord, m: impl OneWord) {
    let (out, m) = (word(out), word(m));
    let _: Ptr = typed_call!(FAMILY, 0x00ee_6dd4, matrix_transpose(e, out, m));
}

/// `D3DXMatrixInverse(out, determinant, m)` through its thunk (`00ee6dda`).
#[inline(always)]
pub(crate) fn call_matrix_inverse(
    e: &mut Engine,
    out: impl OneWord,
    determinant: impl OneWord,
    m: impl OneWord,
) {
    let (out, determinant, m) = (word(out), determinant.one_word(), word(m));
    let _: Ptr = typed_call!(FAMILY, 0x00ee_6dda, matrix_inverse(e, out, determinant, m));
}

/// `D3DXMatrixMultiply(out, m1, m2)` through its thunk (`00ee6de0`).
#[inline(always)]
pub(crate) fn call_matrix_multiply(
    e: &mut Engine,
    out: impl OneWord,
    m1: impl OneWord,
    m2: impl OneWord,
) {
    let (out, m1, m2) = (word(out), word(m1), word(m2));
    let _: Ptr = typed_call!(FAMILY, 0x00ee_6de0, matrix_multiply(e, out, m1, m2));
}

/// `D3DXPlaneNormalize(out, plane)` through its thunk (`00ee6dce`).
#[inline(always)]
pub(crate) fn call_plane_normalize(e: &mut Engine, out: impl OneWord, plane: impl OneWord) {
    let (out, plane) = (word(out), word(plane));
    let _: Ptr = typed_call!(FAMILY, 0x00ee_6dce, plane_normalize(e, out, plane));
}

/// `D3DXPlaneTransform(out, plane, m)` through its thunk (`00ee6dc8`).
#[inline(always)]
pub(crate) fn call_plane_transform(
    e: &mut Engine,
    out: impl OneWord,
    plane: impl OneWord,
    m: impl OneWord,
) {
    let (out, plane, m) = (word(out), word(plane), word(m));
    let _: Ptr = typed_call!(FAMILY, 0x00ee_6dc8, plane_transform(e, out, plane, m));
}
