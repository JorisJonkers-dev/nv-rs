//! The pass begins' shared state in the detached world draw's value packet (nv-rs#992): the constant entries' enabled
//! byte and switch word and the shader objects' counted pointers are own blocks, the draw's accessors keep them in the
//! packet and main's untyped writers (`00c02f70`, `00a5f860`) say what they wrote, so that at the join main's value
//! stays where main wrote last, also when it wrote the value the cut saw. The race is the one the extra passes and the
//! sun query had with a posted draw: both set the same entries and shader objects.

use engine as _;
use engine_core::prelude::*;
use engine_core::Engine;
use engine_typed::draw::{capture, own, statics};

const VERTEX_ENTRIES: u32 = 0x011f_ebd8;
const PIXEL_ENTRIES: u32 = 0x011f_ec30;
const FIRST_MASKS: u32 = 0x011f_cc80;
const SECOND_MASKS: u32 = 0x011f_c0a0;
const SHADER_TABLE: u32 = 0x011f_9548;
/// Plain counted objects (the count at +4) the interlocked imports stand in for.
fn step(e: &mut Engine, at: u32, by: u32) -> engine_core::Ret {
    let v = e.mem.u32(at).wrapping_add(by);
    e.mem.set_u32(at, v);
    engine_core::Ret {
        eax: v,
        ..engine_core::Ret::default()
    }
}

struct World {
    e: Engine,
    vertex: Vec<u32>,
    pixel: Vec<u32>,
    shader: u32,
    /// Three counted objects: held by the shader's three counted slots at the cut, then the two the draw sets.
    objects: [u32; 3],
}

fn world() -> World {
    let mut e = Engine::new();
    if !e.mem.is_mapped(0x011f_9000) {
        e.map(0x011f_9000, 0x7000);
    }
    e.register(0x00fd_f1c0, |e, a| step(e, a[0], 1));
    e.register(0x00fd_f1c4, |e, a| step(e, a[0], u32::MAX));
    let mut tables = [Vec::new(), Vec::new()];
    for (k, base) in [VERTEX_ENTRIES, PIXEL_ENTRIES].into_iter().enumerate() {
        for i in 0..22u32 {
            let entry = e.mem.alloc(0x40);
            e.mem.set_u32(base + 4 * i, entry);
            tables[k].push(entry);
        }
    }
    let [vertex, pixel] = tables;
    let shader = e.mem.alloc(0x100);
    e.mem.set_u32(SHADER_TABLE + 4 * 3, shader);
    let objects = [0u32; 3].map(|_| e.mem.alloc(0x10));
    let [a, b, c] = objects;
    // The vtable's delete slot is never reached here: every count stays above zero.
    // A: three slots; B: the shader's +0x84; C: its +0x88.
    for (object, count) in [(a, 3), (b, 1), (c, 1)] {
        e.mem.set_u32(object + 4, count);
    }
    for offset in [0x28, 0x30, 0x34] {
        e.mem.set_u32(shader + offset, a);
    }
    e.mem.set_u32(shader + 0x84, b);
    e.mem.set_u32(shader + 0x88, c);
    World {
        e,
        vertex,
        pixel,
        shader,
        objects,
    }
}

/// A posted draw and a main pass begin on the same entries and shader object: the draw sets entry 0 on (main's
/// `00c02f70` sets it off, to the value the cut saw), entry 1 on (main sets it on too), the switch word of entry 3 (main
/// writes none); the draw sets the shader's three counted pointers (main assigns +0x28 back to the object it holds, a
/// no-op `00a5f860` the serial order makes a change). The join leaves what the serial order does: the draw first, then
/// main.
#[test]
fn main_wins_the_pass_state_it_wrote_last_and_the_counts_follow() {
    let World {
        mut e,
        vertex,
        pixel,
        shader,
        objects: [a, b, c],
    } = world();
    // Pass 5 of the masks: vertex entry 1 only (bit 2), no pixel entry.
    e.mem.set_u32(FIRST_MASKS + 4 * 5, 0b100);
    e.mem.set_u32(SECOND_MASKS + 4 * 5, 0);
    assert!(capture::begin(&e));
    statics::take_pass_state(&mut e);
    for (what, addr, len) in [
        ("entry byte", vertex[0] + 8, 1),
        ("pixel entry byte", pixel[21] + 8, 1),
        ("switch word", vertex[3] + 0x20, 4),
        ("declaration", shader + 0x28, 4),
        ("pixel map", shader + 0x30, 4),
        ("vertex map", shader + 0x34, 4),
    ] {
        assert!(
            capture::owns(&e, addr, len),
            "the cut did not take the {what}"
        );
    }
    // The drawing side.
    own::set_entry_enabled(&mut e, vertex[0], true);
    own::set_entry_enabled(&mut e, vertex[1], true);
    own::set_switch_word(&mut e, vertex[3], 7);
    own::set_counted(&mut e, shader + 0x28, b);
    own::copy_counted(&mut e, shader + 0x30, shader + 0x84);
    own::assign_counted(&mut e, shader + 0x34, shader + 0x88, true);
    assert!(
        own::entry_is_enabled(&e, vertex[0]),
        "the draw reads its own"
    );
    assert_eq!(own::constant_map(&e, shader, 0x30), b);
    assert_eq!(e.mem.u8(vertex[0] + 8), 0, "guest memory keeps main's");
    assert_eq!(e.mem.u32(shader + 0x34), a, "guest memory keeps main's");
    let packet = capture::detach(&e).expect("the packet");
    // Main, meanwhile: a pass begin by address.
    e.call(0x00c0_2f70, &args![5u32]);
    e.call(0x00a5_f860, &args![shader + 0x28, a]);
    assert!(capture::attach(packet).is_ok());
    let report = capture::end(&mut e);
    assert!(report.kept_main > 0);
    // The serial order: the draw, then main.
    assert_eq!(e.mem.u8(vertex[0] + 8), 0, "main set entry 0 off last");
    assert_eq!(e.mem.u8(vertex[1] + 8), 1);
    assert_eq!(e.mem.u8(vertex[2] + 8), 0);
    assert_eq!(e.mem.u32(vertex[3] + 0x20), 7, "the draw's alone");
    assert_eq!(e.mem.u32(shader + 0x28), a, "main assigned it last");
    assert_eq!(e.mem.u32(shader + 0x30), b);
    assert_eq!(e.mem.u32(shader + 0x34), c);
    // Serial counts: A 3 -> 3 (+0x28 gone and back) -> 1; B 1 -> 2; C 1 -> 2.
    assert_eq!(
        [a, b, c].map(|o| e.mem.u32(o + 4)),
        [1, 2, 2],
        "the counts of the serial order"
    );
}

/// With no writer on main the draw's own state is what the join leaves, and the counts moved once.
#[test]
fn the_draws_pass_state_stands_when_main_writes_nothing() {
    let World {
        mut e,
        vertex,
        shader,
        objects: [a, b, c],
        ..
    } = world();
    assert!(capture::begin(&e));
    statics::take_pass_state(&mut e);
    own::set_entry_enabled(&mut e, vertex[5], true);
    own::set_counted(&mut e, shader + 0x28, b);
    own::set_counted(&mut e, shader + 0x28, c);
    own::set_counted(&mut e, shader + 0x34, a);
    let report = capture::end(&mut e);
    assert_eq!(report.kept_main, 0);
    assert_eq!(e.mem.u8(vertex[5] + 8), 1);
    assert_eq!(e.mem.u32(shader + 0x28), c);
    assert_eq!(e.mem.u32(shader + 0x34), a, "set to what it held: no move");
    assert_eq!(
        [a, b, c].map(|o| e.mem.u32(o + 4)),
        [2, 1, 2],
        "A 3 -> 2, B untouched, C 1 -> 2"
    );
}
