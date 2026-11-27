//! The form registry array (`011c54c4`, an `NiTLargeArray<TESForm *>`) and the large-array bodies it
//! shares with every other `NiTLargeArray` of words: construction, `Add`, `SetAtGrow`, `SetSize`,
//! `SetAt`, `RemoveAt`, the cleanup and the destructor body. Stage 2 of the forms family
//! (nv-rs#951): the array's element block lives in the slab where the array is moved.
//!
//! The array object is [`STUB`]'s K1 stub (`engine_core::typed::form_array`): the vtable, capacity,
//! used size and count stay in the guest block, the element block (+4) is a slab block of the
//! family's own, and the grow-by (+0x14, the spare word) lives in the record. Only the registry's
//! array is moved ([`attach`], from `create_registries`); every other array these functions are
//! handed has no record and is the guest array the translation worked on, word for word.
//!
//! Each function makes the calls by address its translation makes, in its order, so the stepped
//! threads' time slice is stage 1's (a Rust call between functions of the translation is no step,
//! and none here); what a moved block skips is the allocator's call (`00aa1070`, the large arrays'
//! `0096afc0` over it) and the release (`004ede70`): K1 makes them for the block's guest shadow, K2
//! charges them as steps.

use engine_core::typed::form_array::FAMILY;
use engine_core::typed::form_array::{tag, BASE, CAPACITY, COUNT, GROW_BY, SIZE, STUB};
use engine_core::typed::moved::{self, Store};
use engine_core::typed::{Family, Threads};
use engine_core::{args, Engine};
use engine_core::{entry, AbiFn};

/// `NiTLargeArray<TESForm *>`'s vtable (the base's) and the one `004701d0` stores after it.
const BASE_VTABLE: u32 = 0x0101_9a34;
const FORM_ARRAY_VTABLE: u32 = 0x0101_9a3c;
/// The allocator the base constructor calls (`cdecl(bytes)`) and the large arrays' allocation over
/// it (`cdecl(count)`); the release (`cdecl(block)`); `SetAtGrow` and `SetAt` (`thiscall(index,
/// element pointer)`).
const ALLOCATE_BYTES: u32 = 0x00aa_1070;
const ALLOCATE_WORDS: u32 = 0x0096_afc0;
const FREE_BLOCK: u32 = 0x004e_de70;
const SET_AT_GROW: u32 = 0x0086_4240;
const SET_AT: u32 = 0x0061_bd20;

/// The array's store: the family's when `array` is a moved one (its record attached by
/// [`attach`]), stage 1's otherwise.
pub(crate) fn array_store(e: &Engine, array: u32) -> Store {
    let s = e.typed_store(FAMILY);
    let moved = match s {
        Store::Guest => false,
        Store::Moved { .. } => STUB.record(e, array).is_some(),
        Store::Dual { moved, .. } => moved.dual.mirror_of(array.wrapping_add(BASE)).is_some(),
    };
    if moved {
        s
    } else {
        Store::Guest
    }
}

/// Gives the registry array at `array` (a block made, not yet constructed) its record, in the
/// forms family's stage-2 forms; nothing in stage 1.
pub(crate) fn attach(e: &Engine, array: u32) {
    STUB.attach(e, e.typed_store(FAMILY), array, tag::RECORD);
}

fn base_get(e: &Engine, s: Store, array: u32) -> u32 {
    s.load(e, STUB.place(e, s, array, BASE))
}

fn base_set(e: &Engine, s: Store, array: u32, v: u32) {
    s.store(e, STUB.place(e, s, array, BASE), v)
}

/// The grow-by: the record's word where the array is moved (the guest word is the record's handle),
/// the guest word otherwise.
fn grow_get(e: &Engine, s: Store, array: u32) -> u32 {
    match (s, STUB.record(e, array)) {
        (Store::Moved { moved, .. }, Some(r)) => moved.slab.load(r.wrapping_add(GROW_BY)),
        _ => e.mem.u32(array.wrapping_add(GROW_BY)),
    }
}

fn grow_set(e: &Engine, s: Store, array: u32, v: u32) {
    match (s, STUB.record(e, array)) {
        (Store::Moved { moved, .. }, Some(r)) => moved.slab.store(r.wrapping_add(GROW_BY), v),
        _ => e.mem.set_u32(array.wrapping_add(GROW_BY), v),
    }
}

/// The element block's allocation, `count` words, in the array's store. `by_address` is what the
/// caller's translation calls (`00aa1070` with the byte count under the base constructor,
/// `0096afc0` with the count under `SetSize`), which a K2 block does not make: its steps are
/// charged.
fn allocate(
    e: &mut Engine,
    s: Store,
    count: u32,
    steps: &[u32],
    by_address: impl FnOnce(&mut Engine) -> u32,
) -> u32 {
    if matches!(s, Store::Moved { shadows: false, .. }) {
        e.typed_charge(steps);
    }
    s.alloc(e, count.wrapping_shl(2), tag::ELEMENTS, by_address)
}

/// The element block's release (`004ede70`), whichever block `block` is.
fn release(e: &mut Engine, s: Store, block: u32) {
    if matches!(s, Store::Moved { shadows: false, .. }) && moved::is_handle(block) {
        e.typed_charge(&[FREE_BLOCK]);
    }
    s.free(e, block, |e, b| {
        e.call(FREE_BLOCK, &args![b]);
    });
}

// Typed from 004705e0
/// The base constructor `(capacity, grow-by)`: the base's vtable, capacity (+8), grow-by (+0x14),
/// no items (+0xc) and no count (+0x10), and the element block of `capacity` words made only when
/// the capacity is not zero (`00aa1070`).
pub fn construct_base(e: &mut Engine, this: u32, capacity: u32, grow_by: u32) -> u32 {
    base_body(e, this, capacity, grow_by)
}

/// [`construct_base`]'s body, which the translation's constructor runs as a Rust call (no step).
fn base_body(e: &mut Engine, this: u32, capacity: u32, grow_by: u32) -> u32 {
    let s = array_store(e, this);
    e.mem.set_u32(this, BASE_VTABLE);
    e.mem.set_u32(this.wrapping_add(CAPACITY), capacity);
    grow_set(e, s, this, grow_by);
    e.mem.set_u32(this.wrapping_add(SIZE), 0);
    e.mem.set_u32(this.wrapping_add(COUNT), 0);
    let block = if capacity > 0 {
        allocate(e, s, capacity, &[ALLOCATE_BYTES], |e| {
            e.call(ALLOCATE_BYTES, &args![capacity.wrapping_shl(2)])
                .u32()
        })
    } else {
        0
    };
    base_set(e, s, this, block);
    this
}

// Typed from 004701d0
/// The registry array's constructor: the base's, then `01019a3c`.
pub fn construct(e: &mut Engine, this: u32, capacity: u32, grow_by: u32) -> u32 {
    base_body(e, this, capacity, grow_by);
    e.mem.set_u32(this, FORM_ARRAY_VTABLE);
    this
}

// Typed from 00470110
/// The destructor body: the base's vtable stored again, the element block released (`004ede70`),
/// and, last, the moved array's record taken away.
pub fn destructor(e: &mut Engine, this: u32) {
    let s = array_store(e, this);
    e.mem.set_u32(this, BASE_VTABLE);
    let block = base_get(e, s, this);
    release(e, s, block);
    STUB.detach(e, s, this);
}

// Typed from 00470140
/// `Add(&item)`: -1 for a null item; else the word goes in the first empty slot below the used size
/// (the count goes up, the slot's index is the answer); with none, `SetAtGrow(size, &item)`
/// (`00864240`) appends it.
pub fn add(e: &mut Engine, this: u32, item: u32) -> u32 {
    let s = array_store(e, this);
    if e.mem.u32(item) == 0 {
        return u32::MAX;
    }
    let mut index = 0u32;
    while index < e.mem.u32(this.wrapping_add(SIZE)) {
        let slot = base_get(e, s, this).wrapping_add(index.wrapping_mul(4));
        if s.load(e, slot) == 0 {
            let value = e.mem.u32(item);
            s.store(e, slot, value);
            let used = e.mem.u32(this.wrapping_add(COUNT)).wrapping_add(1);
            e.mem.set_u32(this.wrapping_add(COUNT), used);
            return index;
        }
        index = index.wrapping_add(1);
    }
    let size = e.mem.u32(this.wrapping_add(SIZE));
    e.call(SET_AT_GROW, &args![this, size, item]).u32()
}

// Typed from 00864240
/// `SetAtGrow(index, &element)`: from the capacity (+8) up, the array grows to `index + grow-by`
/// first (`SetSize`, a Rust call: no step); then `SetAt` (`0061bd20`). Answers `index`.
pub fn set_at_grow(e: &mut Engine, this: u32, index: u32, element: u32) -> u32 {
    if index >= e.mem.u32(this.wrapping_add(CAPACITY)) {
        let s = array_store(e, this);
        let grow_by = grow_get(e, s, this);
        size_body(e, this, index.wrapping_add(grow_by));
    }
    e.call(SET_AT, &args![this, index, element]);
    index
}

// Typed from 00864280
/// `SetSize(new_max)`: nothing at the current capacity. Below the used size the elements from
/// `new_max` on are cleared (the count down for each non-null one) and the used size is `new_max`.
/// Then a new block of `new_max` words (`0096afc0`; null for 0) takes the used elements, the others
/// are zeroed, the old block is released (`004ede70`) and the capacity is `new_max`.
pub fn set_size(e: &mut Engine, this: u32, new_max: u32) {
    size_body(e, this, new_max)
}

/// [`set_size`]'s body, which `SetAtGrow` runs as a Rust call (no step).
fn size_body(e: &mut Engine, this: u32, new_max: u32) {
    if new_max == e.mem.u32(this.wrapping_add(CAPACITY)) {
        return;
    }
    let s = array_store(e, this);
    if new_max < e.mem.u32(this.wrapping_add(SIZE)) {
        let mut index = new_max;
        while index < e.mem.u32(this.wrapping_add(SIZE)) {
            let slot = base_get(e, s, this).wrapping_add(index.wrapping_mul(4));
            if s.load(e, slot) != 0 {
                s.store(e, slot, 0);
                let present = e.mem.u32(this.wrapping_add(COUNT));
                e.mem
                    .set_u32(this.wrapping_add(COUNT), present.wrapping_sub(1));
            }
            index = index.wrapping_add(1);
        }
        e.mem.set_u32(this.wrapping_add(SIZE), new_max);
    }
    let old = base_get(e, s, this);
    e.mem.set_u32(this.wrapping_add(CAPACITY), new_max);
    if new_max == 0 {
        base_set(e, s, this, 0);
    } else {
        let max = e.mem.u32(this.wrapping_add(CAPACITY));
        let block = allocate(e, s, max, &[ALLOCATE_WORDS, ALLOCATE_BYTES], |e| {
            e.call(ALLOCATE_WORDS, &args![max]).u32()
        });
        base_set(e, s, this, block);
        let mut index = 0u32;
        while index < e.mem.u32(this.wrapping_add(SIZE)) {
            let to = base_get(e, s, this).wrapping_add(index.wrapping_mul(4));
            let value = s.load(e, old.wrapping_add(index.wrapping_mul(4)));
            s.store(e, to, value);
            index = index.wrapping_add(1);
        }
        let mut index = e.mem.u32(this.wrapping_add(SIZE));
        while index < e.mem.u32(this.wrapping_add(CAPACITY)) {
            let to = base_get(e, s, this).wrapping_add(index.wrapping_mul(4));
            s.store(e, to, 0);
            index = index.wrapping_add(1);
        }
    }
    release(e, s, old);
}

// Typed from 0061bd20
/// `SetAt(index, &element)`: the word `*element` stored at `index` with no capacity check. Below
/// the used size the count goes up when an empty slot gets a word and down when a word's slot gets
/// zero; at or above it the used size becomes `index + 1` and the count goes up for a word.
pub fn set_at(e: &mut Engine, this: u32, index: u32, element: u32) {
    let s = array_store(e, this);
    let slot_of = |e: &Engine| base_get(e, s, this).wrapping_add(index.wrapping_mul(4));
    if index < e.mem.u32(this.wrapping_add(SIZE)) {
        if e.mem.u32(element) == 0 {
            if s.load(e, slot_of(e)) != 0 {
                let present = e.mem.u32(this.wrapping_add(COUNT));
                e.mem
                    .set_u32(this.wrapping_add(COUNT), present.wrapping_sub(1));
            }
        } else if s.load(e, slot_of(e)) == 0 {
            let present = e.mem.u32(this.wrapping_add(COUNT));
            e.mem
                .set_u32(this.wrapping_add(COUNT), present.wrapping_add(1));
        }
    } else {
        e.mem
            .set_u32(this.wrapping_add(SIZE), index.wrapping_add(1));
        if e.mem.u32(element) != 0 {
            let present = e.mem.u32(this.wrapping_add(COUNT));
            e.mem
                .set_u32(this.wrapping_add(COUNT), present.wrapping_add(1));
        }
    }
    let value = e.mem.u32(element);
    let slot = slot_of(e);
    s.store(e, slot, value);
}

// Typed from 00486b80
/// `RemoveAt(index)`: the element at `index` answered and cleared when the index is below the used
/// size (else 0); the count down when it was not null, and the used size down by one when it was
/// the last used slot.
pub fn remove_at(e: &mut Engine, this: u32, index: u32) -> u32 {
    let s = array_store(e, this);
    let size = e.mem.u32(this.wrapping_add(SIZE));
    if index >= size {
        return 0;
    }
    let slot = base_get(e, s, this).wrapping_add(index.wrapping_mul(4));
    let element = s.load(e, slot);
    s.store(e, slot, 0);
    if element != 0 {
        let count = e.mem.u32(this.wrapping_add(COUNT)).wrapping_sub(1);
        e.mem.set_u32(this.wrapping_add(COUNT), count);
    }
    let size = e.mem.u32(this.wrapping_add(SIZE));
    if index == size.wrapping_sub(1) {
        e.mem.set_u32(this.wrapping_add(SIZE), size.wrapping_sub(1));
    }
    element
}

// Typed from 00863db0
/// The cleanup before the array is deleted: the first used-size slots set to null, then the used
/// size and the count zero.
pub fn clear(e: &mut Engine, this: u32) {
    let s = array_store(e, this);
    let mut index = 0u32;
    while index < e.mem.u32(this.wrapping_add(SIZE)) {
        let slot = base_get(e, s, this).wrapping_add(index.wrapping_mul(4));
        s.store(e, slot, 0);
        index += 1;
    }
    e.mem.set_u32(this.wrapping_add(SIZE), 0);
    e.mem.set_u32(this.wrapping_add(COUNT), 0);
}

/// `NV_TYPED=formarray`: the registry array's functions (a family of its own beside `forms`, so a
/// test of the forms' unit may keep its doubles at these addresses). On unless `NV_TYPED` says
/// otherwise; stage 2 for the one array: `formarray:dual`, `formarray:moved` (K1), `formarray:freed` (K2).
pub static FORM_ARRAY: Family = Family {
    name: "formarray",
    id: FAMILY,
    past_gate: true,
    threads: Threads::Both,
    stage2: true,
    funcs,
};

/// The family's functions, by exe address.
fn funcs() -> Vec<(u32, AbiFn)> {
    vec![
        entry!(0x0047_0110, destructor(u32)),
        entry!(0x0047_0140, add(u32, u32) -> u32),
        entry!(0x0047_01d0, construct(u32, u32, u32) -> u32),
        entry!(0x0047_05e0, construct_base(u32, u32, u32) -> u32),
        entry!(0x0048_6b80, remove_at(u32, u32) -> u32),
        entry!(0x0061_bd20, set_at(u32, u32, u32)),
        entry!(0x0086_3db0, clear(u32)),
        entry!(0x0086_4240, set_at_grow(u32, u32, u32) -> u32),
        entry!(0x0086_4280, set_size(u32, u32)),
    ]
}
