//! The pass begins' shared state in the detached world draw's value packet (nv-rs#992): the constant entries' enabled
//! byte and switch word and the shader objects' counted pointers are own blocks, the draw's accessors keep them in the
//! packet and main's untyped writers (`00c02f70`, `00a5f860`) say what they wrote, so that at the join main's value
//! stays where main wrote last, also when it wrote the value the cut saw. The race is the one the extra passes and the
//! sun query had with a posted draw: both set the same entries and shader objects.

use engine as _;
use engine_core::prelude::*;
use engine_core::typed::leaf;
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

// ---- The pass objects (nv-rs#992, step 2): stage words, stage state caches, render state blocks, Group C entries. ----

const PASS_TABLE: u32 = 0x011f_dff8;
/// The sampler state to slot map the exe keeps (`0126f92c`, a halfword a state).
const SLOT_MAP: u32 = 0x0126_f92c;
/// Lighting 3.0's constant entry pointers (`01201080` on), the seventh of them.
const GROUP_C_ENTRIES: u32 = 0x0120_1080;

/// One pass of the pass table (pass type 9, the sun query's) with two stages, each with a state cache, a render state
/// block holding one entry, and one Group C constant entry.
struct Passes {
    e: Engine,
    pass: u32,
    stages: [u32; 2],
    caches: [u32; 2],
    block: u32,
    entry: u32,
    group_c: u32,
}

fn passes() -> Passes {
    let mut e = Engine::new();
    for (base, len) in [
        (0x011f_9000u32, 0x7000u32),
        (0x0120_0000, 0x3000),
        (0x0126_f000, 0x1000),
    ] {
        if !e.mem.is_mapped(base) {
            e.map(base, len);
        }
    }
    // Sampler state s lives in slot s - 1.
    for state in 1..6u32 {
        e.mem.set_u16(SLOT_MAP + 2 * state, (state - 1) as u16);
    }
    let pass = e.mem.alloc(0x80);
    e.mem.set_u32(PASS_TABLE + 4 * 9, pass);
    let list = e.mem.alloc(0x10);
    let mut stages = [0u32; 2];
    let mut caches = [0u32; 2];
    for i in 0..2usize {
        let stage = e.mem.alloc(0x68);
        let cache = e.mem.alloc(0xb8);
        e.mem.set_u32(stage + 4, i as u32);
        e.mem.set_u32(stage + 8, 0x1000 + i as u32);
        e.mem.set_u32(stage + 0x10, cache);
        e.mem.set_u32(list + 4 * i as u32, stage);
        stages[i] = stage;
        caches[i] = cache;
    }
    e.mem.set_u32(pass + 0x18, 2);
    e.mem.set_u32(pass + 0x24, list);
    // A render state block (`00e7f5d0`): the second list's last entry at +0x10, the entry {state, value, previous, next}.
    let block = e.mem.alloc(0x20);
    let entry = e.mem.alloc(0x10);
    e.mem.set_u32(entry, 0x1c);
    e.mem.set_u32(entry + 4, 5);
    e.mem.set_u32(block + 0xc, 1);
    e.mem.set_u32(block + 0x10, entry);
    e.mem.set_u32(pass + 0x30, block);
    let group_c = e.mem.alloc(0x40);
    e.mem.set_u32(GROUP_C_ENTRIES + 0x1c, group_c);
    Passes {
        e,
        pass,
        stages,
        caches,
        block,
        entry,
        group_c,
    }
}

/// The cut takes every word of the pass objects and Group C's entries the pass begins write.
#[test]
fn the_cut_takes_the_pass_objects_and_group_c_entries() {
    let Passes {
        mut e,
        pass,
        stages,
        caches,
        block,
        entry,
        group_c,
    } = passes();
    assert!(capture::begin(&e));
    statics::take_pass_state(&mut e);
    statics::take_pass_objects(&mut e, None);
    for (what, addr, len) in [
        ("render state block pointer", pass + 0x30, 4),
        ("render state block header", block, 0x14),
        ("render state entry", entry, 0x10),
        ("stage 0 texture", stages[0] + 8, 4),
        ("stage 1 texture", stages[1] + 8, 4),
        ("stage 1 count", stages[1] + 0x60, 4),
        ("stage 0 state cache", caches[0], 0xb8),
        ("stage 1 state cache", caches[1], 0xb8),
        ("Group C entry byte", group_c + 8, 1),
        ("Group C entry switch word", group_c + 0x20, 4),
    ] {
        assert!(
            capture::owns(&e, addr, len),
            "the cut did not take the {what}"
        );
    }
    capture::end(&mut e);
}

/// A posted draw and the extra passes (or the sun query) beginning the same pass: main writes the stage texture, a
/// sampler state of the stage's cache, the render state's value, the block and a Group C entry while the draw has not
/// run (the stepped order, the worst case); the draw reads what the cut saw, writes its own, and the join leaves what
/// the serial order does: the draw first, then main. Fails when the cut leaves any of them in guest memory (the draw
/// would read main's writes), which is what made the joins of `006426a0` and `00b6c0d0` necessary.
#[test]
fn the_draw_reads_the_pass_objects_as_cut_while_main_begins_the_same_pass() {
    let Passes {
        mut e,
        pass,
        stages,
        caches,
        block,
        entry,
        group_c,
    } = passes();
    assert!(capture::begin(&e));
    statics::take_pass_state(&mut e);
    statics::take_pass_objects(&mut e, None);
    let packet = capture::detach(&e).expect("the packet");
    // Main, first: its pass begin through the setters it shares with the draw, and the untyped ones by address.
    capture::set_own_at(&mut e, stages[0] + 8, 0x7001u32);
    leaf::set_state_slot(&mut e, caches[0], 1u32, 0x11u32, 0u32, 0u32);
    e.mem.set_u32(entry + 4, 9);
    let other = e.mem.alloc(0x20);
    e.mem.set_u32(pass + 0x30, other);
    e.mem.set_u8(group_c + 8, 1);
    engine_core::own_view::note_write(&e, group_c + 8, 1);
    assert!(capture::attach(packet).is_ok());
    // The draw, after: its reads are the cut's.
    assert_eq!(capture::own_at::<u32>(&e, stages[0] + 8), 0x1000);
    assert_eq!(
        capture::own_at::<u8>(&e, caches[0] + 0xa4),
        0,
        "no sampler state yet"
    );
    assert_eq!(capture::own_at::<u32>(&e, entry + 4), 5);
    assert_eq!(capture::own_at::<u32>(&e, pass + 0x30), block);
    assert_eq!(capture::own_at::<u8>(&e, group_c + 8), 0);
    // And its writes stay its own until the join.
    capture::set_own_at(&mut e, stages[1] + 8, 0x7002u32);
    leaf::set_state_slot(&mut e, caches[1], 2u32, 0x22u32, 0u32, 0u32);
    assert_eq!(capture::own_at::<u32>(&e, stages[1] + 8), 0x7002);
    assert_eq!(
        capture::own_at::<u8>(&e, caches[1] + 0xa5),
        1,
        "the draw's sampler state"
    );
    assert_eq!(
        e.mem.u32(stages[1] + 8),
        0x1001,
        "guest memory keeps main's"
    );
    assert_eq!(e.mem.u8(caches[1] + 0xa5), 0, "guest memory keeps main's");
    let report = capture::end(&mut e);
    assert!(report.kept_main > 0);
    // The serial order: the draw, then main.
    assert_eq!(
        e.mem.u32(stages[0] + 8),
        0x7001,
        "main's, the draw wrote none"
    );
    assert_eq!(
        e.mem.u32(stages[1] + 8),
        0x7002,
        "the draw's, main wrote none"
    );
    assert_eq!(e.mem.u8(caches[0] + 0xa4), 1);
    assert_eq!(e.mem.u32(caches[0] + 0x90), 0x11, "main's sampler state");
    assert_eq!(e.mem.u8(caches[1] + 0xa5), 1);
    assert_eq!(
        e.mem.u32(caches[1] + 0x94),
        0x22,
        "the draw's sampler state"
    );
    assert_eq!(e.mem.u32(entry + 4), 9, "main's render state value");
    assert_eq!(e.mem.u32(pass + 0x30), other, "main's block");
    assert_eq!(e.mem.u8(group_c + 8), 1, "main's Group C entry");
}

/// The camera the draw last started with (`011f917c`) is written by the world draw's first half and by the secondary
/// scenes' draw alike, and read back by the speed tree's camera rows: it is an own global, so the extra passes' camera
/// does not replace the draw's under it, and at the join main's stays (it wrote last in the serial order).
#[test]
fn the_last_camera_is_the_draws_own_while_the_extra_passes_start() {
    const LAST_CAMERA: u32 = 0x011f_917c;
    assert!(statics::is_own_global(LAST_CAMERA));
    let mut e = Engine::new();
    if !e.mem.is_mapped(0x011f_9000) {
        e.map(0x011f_9000, 0x7000);
    }
    e.mem.set_u32(LAST_CAMERA, 0xaaaa);
    assert!(capture::begin(&e));
    for (lo, hi) in statics::GLOBAL_BLOCKS {
        capture::own(&e, lo, 0, hi - lo);
    }
    // The draw's first half noted its camera.
    capture::set_own_at(&mut e, LAST_CAMERA, 0xbbbbu32);
    let packet = capture::detach(&e).expect("the packet");
    // Main's extra passes start: their camera, written live.
    capture::set_own_at(&mut e, LAST_CAMERA, 0xccccu32);
    assert!(capture::attach(packet).is_ok());
    assert_eq!(
        capture::own_at::<u32>(&e, LAST_CAMERA),
        0xbbbb,
        "the draw reads its own camera"
    );
    capture::end(&mut e);
    assert_eq!(e.mem.u32(LAST_CAMERA), 0xcccc, "main started last");
}

/// The pass set of the cut is the passes the draw begins, not the table read by type (nv-rs#992 step 2): the pass
/// table `011fdff8` is the shadow light shader's, indexed by type; lighting 3.0 reads it at a place of its own for a
/// type (`pass_case`), and the sky, leaf and tall grass shaders read words of their own object. A word of the table that
/// is not a pass (the `0x11` that `00b78a40`'s tests keep in it) is left alone, not walked.
#[test]
fn the_pass_objects_are_those_the_listed_passes_begin_and_a_non_pointer_entry_is_left_alone() {
    let Passes {
        mut e,
        pass,
        stages,
        caches,
        ..
    } = passes();
    // A sentinel in the entry of type 0 and an unmapped pointer in type 1's, a pass of type 7's entry that is not a pass
    // (a stage count of 0x40).
    e.mem.set_u32(PASS_TABLE, 0x11);
    e.mem.set_u32(PASS_TABLE + 4, 0x7f00_0000);
    let fake = e.mem.alloc(0x80);
    e.mem.set_u32(fake + 0x18, 0x40);
    e.mem.set_u32(PASS_TABLE + 4 * 7, fake);
    assert!(capture::begin(&e));
    statics::take_pass_objects(&mut e, None);
    for (what, addr, len) in [
        ("the pass of type 9", pass + 0x30, 4),
        ("its stage 0", stages[0] + 8, 4),
        ("its cache", caches[0], 0xb8),
    ] {
        assert!(capture::owns(&e, addr, len), "{what} is not taken");
    }
    assert!(
        !capture::owns(&e, fake + 0x30, 4),
        "a word that is not a pass is walked as one"
    );
    capture::end(&mut e);
}

/// A shader's own pass word (the leaf shader's +0x384) is a pass the draw begins that the table does not name: the
/// listed passes' shaders give it, the table by type does not.
#[test]
fn a_pass_the_listed_shader_holds_is_taken_though_no_table_entry_names_it() {
    let Passes { mut e, pass, .. } = passes();
    let own_pass = e.mem.alloc(0x80);
    e.mem.set_u32(own_pass + 0x24, 0);
    let block = e.mem.alloc(0x20);
    e.mem.set_u32(own_pass + 0x30, block);
    let shader = e.mem.alloc(0x400);
    e.mem.set_u32(shader, 0x010b_9190);
    e.mem.set_u32(shader + 0x384, own_pass);
    let geometry = e.mem.alloc(0x100);
    e.mem.set_u32(geometry + 0xc0, shader);
    let render_pass = e.mem.alloc(0x40);
    // A listed pass of type 9 (the table's) with this geometry.
    e.mem.set_u32(render_pass, geometry);
    e.mem.set_u16(render_pass + 4, 9);
    assert!(capture::begin(&e));
    statics::take_pass_objects(&mut e, Some(&[render_pass]));
    assert!(
        capture::owns(&e, pass + 0x30, 4),
        "the table's pass for the type"
    );
    assert!(
        capture::owns(&e, own_pass + 0x30, 4),
        "the shader's own pass"
    );
    assert!(capture::owns(&e, block, 0x14), "its render state block");
    capture::end(&mut e);
}

/// The sun query's set-up reads the shader's counted maps through the packet: `00b7a870` (here a stand-in that sets them as
/// the typed code does, into the packet) leaves them there while the guest keeps the old ones; a bare guest read would call the
/// old maps.
#[test]
fn the_sun_query_calls_the_counted_maps_the_set_up_made() {
    let mut e = Engine::new();
    for (base, len) in [(0x011f_4000u32, 0x1000u32), (0x011f_9000, 0x7000)] {
        if !e.mem.is_mapped(base) {
            e.map(base, len);
        }
    }
    let shader = e.mem.alloc(0x100);
    let mut slots = [0u32; 0x10];
    slots[0x24 / 4] = 0x0f00_0024;
    e.put_vtable(0x0f00_3000, &slots);
    let objects = [0u32; 4].map(|_| e.mem.alloc(8));
    for o in objects {
        e.mem.set_u32(o, 0x0f00_3000);
    }
    let [old_first, old_second, new_first, new_second] = objects;
    e.mem.set_u32(shader + 0x34, old_first);
    e.mem.set_u32(shader + 0x30, old_second);
    // Non-capturing stand-ins: their data sits in guest memory (a log of the called objects at LOG).
    const DATA: u32 = 0x011f_9100;
    e.mem.set_u32(DATA, shader);
    e.mem.set_u32(DATA + 4, new_first);
    e.mem.set_u32(DATA + 8, new_second);
    e.register(0x0f00_0024, |e, a| {
        let n = e.mem.u32(DATA + 0x10);
        e.mem.set_u32(DATA + 0x14 + 4 * n, a[0]);
        e.mem.set_u32(DATA + 0x10, n + 1);
        engine_core::Ret::default()
    });
    e.register(0x00b5_5560, |e, _| engine_core::Ret {
        eax: e.mem.u32(DATA),
        ..engine_core::Ret::default()
    });
    e.register(0x00b7_a870, |e, _| {
        let shader = e.mem.u32(DATA);
        let (first, second) = (e.mem.u32(DATA + 4), e.mem.u32(DATA + 8));
        capture::set_own_at(e, shader + 0x34, first);
        capture::set_own_at(e, shader + 0x30, second);
        engine_core::Ret::default()
    });
    for f in [
        0x00c4_ed70u32,
        0x004e_9bb0,
        0x00ba_8ec0,
        0x00b9_94f0,
        0x00c4_edb0,
        0x00ba_8cd0,
    ] {
        e.register(f, |_, _| engine_core::Ret::default());
    }
    let acc = e.mem.alloc(0x200);
    let query = e.mem.alloc(8);
    e.mem.set_u32(acc + 0x48, query);
    let geometry = e.mem.alloc(0x100);
    assert!(capture::begin(&e));
    capture::own(&e, shader, 0x28, 0x38);
    e.call(0x00b6_4bf0, &[acc, geometry, 0, 0]);
    assert_eq!(
        [
            e.mem.u32(DATA + 0x10),
            e.mem.u32(DATA + 0x14),
            e.mem.u32(DATA + 0x18)
        ],
        [2, new_first, new_second],
        "the query called the maps the guest still holds"
    );
    capture::end(&mut e);
}
