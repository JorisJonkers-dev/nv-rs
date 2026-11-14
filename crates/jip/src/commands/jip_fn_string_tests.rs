//! Tests of `GetStringHash`: the hash JIP's maps use (seed 0xD39CA509, each byte subtracted at
//! 4, 12, 20 and 0 bits in turn, times 31), worked out by hand for short texts.

use crate::commands::test_support::*;

/// The hash worked out in full for `"a"` and `"AB"`: the seed minus `c << 4`, times 31.
fn by_hand(bytes: &[u8]) -> i32 {
    let mut h: u32 = 0xd39c_a509;
    for (i, &b) in bytes.iter().enumerate() {
        let shift = [4, 12, 20, 0][i % 4];
        h = h.wrapping_sub(u32::from(b) << shift).wrapping_mul(31);
    }
    h as i32
}

#[test]
fn the_hash_without_and_with_case() {
    let mut e = engine();
    let ci = call(&mut e, 0x293f, &[s(b"Hello"), n(0), n(0)]).1;
    assert_eq!(ci, f64::from(by_hand(b"hello")));
    let cs = call(&mut e, 0x293f, &[s(b"Hello"), n(0), n(1)]).1;
    assert_eq!(cs, f64::from(by_hand(b"Hello")));
    assert_ne!(ci, cs);
    // One byte: the seed less 'a' << 4, times 31, signed.
    let one = call(&mut e, 0x293f, &[s(b"A")]).1;
    assert_eq!(
        one,
        f64::from((0xd39c_a509u32.wrapping_sub(0x61 << 4)).wrapping_mul(31) as i32)
    );
    assert!(one < 0.0, "a signed answer");
}

#[test]
fn an_empty_text_and_the_split_mode_answer_nothing() {
    let mut e = engine();
    assert_eq!(call(&mut e, 0x293f, &[s(b"")]).1, 0.0);
    assert_eq!(call(&mut e, 0x293f, &[s(b"a|b"), n(1)]).1, 0.0);
}
