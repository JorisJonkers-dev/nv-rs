//! Tests of the utility commands built so far: the minimum and maximum of up to five numbers,
//! the random number through the game's generator, and the two load order commands on the save's
//! `saveMods` table (+0x44, 255 bytes, and the 255 after it).

use super::*;
use crate::commands::test_support::*;
use std::cell::RefCell;

const SL: u32 = FORMS + 0x8000;

thread_local! {
    static ASKED: RefCell<Vec<Vec<u32>>> = const { RefCell::new(Vec::new()) };
}

fn setup() -> Engine {
    let mut e = engine();
    e.map(0x011d_d000, 0x1000);
    e.mem.set_u32(SAVE_LOAD_GAME, SL);
    for i in 0..255 {
        e.mem.set_u8(SL + 0x44 + i, 0xff);
    }
    ASKED.with(|a| a.borrow_mut().clear());
    e.register(GET_RANDOM_INT, |_, w| {
        ASKED.with(|a| a.borrow_mut().push(w.to_vec()));
        Ret {
            eax: 7,
            ..Ret::default()
        }
    });
    e
}

#[test]
fn minimum_and_maximum_of_the_given_numbers() {
    let mut e = setup();
    let args = |v: &[f64]| v.iter().map(|&x| z(x)).collect::<Vec<_>>();
    assert_eq!(call(&mut e, 0x262e, &args(&[3.0, -2.5])).1, -2.5);
    assert_eq!(call(&mut e, 0x262f, &args(&[3.0, -2.5])).1, 3.0);
    assert_eq!(
        call(&mut e, 0x262e, &args(&[5.0, 4.0, 9.0, 1.5, 2.0])).1,
        1.5
    );
    assert_eq!(
        call(&mut e, 0x262f, &args(&[5.0, 4.0, 9.0, 1.5, 2.0])).1,
        9.0
    );
    // Only the given count is used.
    assert_eq!(call(&mut e, 0x262e, &args(&[5.0, 4.0, 3.0])).1, 3.0);
    assert_eq!(call(&mut e, 0x262f, &args(&[-5.0, -4.0, -30.0])).1, -4.0);
}

#[test]
fn a_random_number_is_the_games_below_the_span_plus_the_minimum() {
    let mut e = setup();
    assert_eq!(call(&mut e, 0x28de, &[n(-10), n(20)]).1, -3.0);
    assert_eq!(
        ASKED.with(|a| a.borrow().clone()),
        vec![vec![0x011c_4180, 30]],
        "the generator and the span"
    );
    assert_eq!(call(&mut e, 0x28de, &[n(5), n(5)]).1, 12.0);
}

#[test]
fn a_changed_load_order() {
    let mut e = setup();
    assert_eq!(call(&mut e, 0x268e, &[]).1, 0.0, "nothing loaded");
    // File 4 of the save is file 4 today: unchanged.
    e.mem.set_u8(SL + 0x44 + 4, 4);
    e.mem.set_u8(SL + 0x44 + 4 + 0xff, 4);
    assert_eq!(call(&mut e, 0x268e, &[]).1, 0.0);
    // File 9 of the save is file 8 today (the other table says 9).
    e.mem.set_u8(SL + 0x44 + 9, 8);
    e.mem.set_u8(SL + 0x44 + 9 + 0xff, 9);
    assert_eq!(call(&mut e, 0x268e, &[]).1, 1.0);
    // Index 0 is not looked at.
    let mut e = setup();
    e.mem.set_u8(SL + 0x44, 3);
    assert_eq!(call(&mut e, 0x268e, &[]).1, 0.0);
}

#[test]
fn a_mod_index_as_loaded_today() {
    let mut e = setup();
    e.mem.set_u8(SL + 0x44 + 7, 9);
    assert_eq!(call(&mut e, 0x268f, &[n(7)]).1, 9.0);
    assert_eq!(call(&mut e, 0x268f, &[n(8)]).1, -1.0, "no longer loaded");
    assert_eq!(call(&mut e, 0x268f, &[n(0xff)]).1, 255.0, "0xFF stays");
    assert_eq!(call(&mut e, 0x268f, &[n(0x100)]).1, -1.0, "above 0xFF");
}
