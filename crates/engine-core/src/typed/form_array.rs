//! The forms family's registry array (`011c54c4`), the large array of every form that carries flag 2
//! (nv-rs#951, stage 2 of the forms family): what the family and the translations that read the array
//! share ([`moved`]'s machinery, one array, one K1 stub).
//!
//! The array is an `NiTLargeArray<TESForm *>` (+0 vtable, +4 element block, +8 capacity, +0xc used
//! size, +0x10 count of non-null elements, +0x14 grow-by). Only the element block moves: it is one
//! slab block of the family's own, and the array object stays a K1 stub whose +4 holds poison while
//! the array lives, its record (the block's handle at +4) named by the spare word +0x14. The grow-by
//! is the one field of the object nothing but the array's own functions read, so it is the spare
//! word: its value lives in the record at +0x14 while the array is moved.
//!
//! Untyped code that read the block (`fn_006b6580`, the nav mesh info map's count of nav meshes in
//! the array) reads an element through [`slot`].

use super::moved::{K1, POISON_BASE};
use super::FamilyId;
use crate::Engine;

/// The registry array family's bit (`engine_typed::forms::array::FAMILY`).
pub const FAMILY: FamilyId = FamilyId::new(48);

/// The array object's K1 stub: the record is the array's layout in the slab, the moved field the
/// element block (+4), the spare word the grow-by (+0x14).
pub const STUB: K1 = K1 {
    size: 0x18,
    spare: 0x14,
    moved: &[BASE],
};

/// The element block (+4), capacity (+8), used size (+0xc), count (+0x10) and grow-by (+0x14).
pub const BASE: u32 = 0x04;
pub const CAPACITY: u32 = 0x08;
pub const SIZE: u32 = 0x0c;
pub const COUNT: u32 = 0x10;
pub const GROW_BY: u32 = 0x14;

/// The tags of the family's slab blocks, for the heap census.
pub mod tag {
    pub const RECORD: u16 = 1;
    pub const ELEMENTS: u16 = 2;
}

/// The word in the slot `element` of `array`'s element block, the slot being index `index` of it
/// (`element` what `array_element` (`00877a30`) answered for `index`): the word at `element` where
/// the block is in guest memory; where the array is moved, `element` is the poison of its moved
/// field plus the offset, and the word is read from the block in the slab.
pub fn slot(e: &Engine, array: u32, index: u32, element: u32) -> u32 {
    if element < POISON_BASE {
        return e.mem.u32(element);
    }
    let moved = e
        .moved()
        .expect("a poisoned element address needs the slab");
    let record = STUB
        .record(e, array)
        .expect("a poisoned element address names a moved array");
    let block = moved.slab.load(record.wrapping_add(BASE));
    moved.slab.load(block.wrapping_add(index.wrapping_mul(4)))
}
