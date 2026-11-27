// SPDX-License-Identifier: GPL-3.0-only
//! The extra data node classes' functions of the extradataobjects_p3 unit (nv-rs#952, step 1).

use super::layouts::*;
use super::objects::*;
use super::{store, FAMILY};
use engine_core::prelude::*;
use engine_core::types::*;
use engine_core::AbiFn;

/// `RTTI Type Descriptor` of `ExtraSpecialRenderFlags`, the target of the
/// `dynamic_cast` in its `Compare` and `Copy`.
const EXTRA_SPECIAL_RENDER_FLAGS_TYPE: u32 = 0x0118_4078;
/// The setter of the word at +0xc (`m_flags`; `this`, the new value). The
/// engine map names the folded body `NonActorMagicCaster::SetCurrentSpell`.
const SET_FLAGS: u32 = 0x0041_fd00;
/// A string-like constructor (`this`, a pointer, 0); `00438390` forwards to it.
const STRING_INIT: u32 = 0x0040_37f0;
/// The length of the `BSStringT` that is `this` (`sLen`, or the C string
/// length when `sLen` is `0xffff`).
const STRING_LENGTH: u32 = 0x0040_48e0;
/// `sMaxLen` of the `BSStringT` that is `this`.
const STRING_CAPACITY: u32 = 0x0040_39a0;
/// Stores `sMaxLen` (`this`, the value; clamped to `0xffff`).
const STRING_SET_CAPACITY: u32 = 0x0040_3a00;
/// Stores `sLen` (`this`, the value; clamped to `0xffff`).
const STRING_SET_LENGTH: u32 = 0x0040_39c0;
/// `memcpy(destination, source, size)` (a cdecl wrapper).
const MEM_COPY: u32 = 0x0040_1460;
/// `memset(destination, value, size)` (a cdecl wrapper).
const MEM_SET: u32 = 0x0040_3d30;
/// The C runtime `memset(destination, value, size)`.
const CRT_MEMSET: u32 = 0x00ec_61c0;
/// The bucket array allocator of `NiTMapBase` (size in bytes; never null).
const BUCKETS_ALLOCATE: u32 = 0x00aa_1070;
/// The bucket array release of `NiTMapBase` (pointer, may be null).
const BUCKETS_FREE: u32 = 0x00aa_10f0;
/// The node release shared by the map destructors (`this` is the map).
const BUCKETS_RELEASE: u32 = 0x0043_8af0;
/// Constructs the `count` elements that start at the pointer (`this`,
/// pointer, count); the `CombatTimeStamp` and `unsigned int` arrays use it.
const ARRAY_CONSTRUCT_ELEMENTS: u32 = 0x0043_0000;
/// The array constructor of the `DismemberedLimb *` and `unsigned int`
/// arrays (`this`, reservation, size).
const ARRAY_CONSTRUCT: u32 = 0x006b_3eb0;
/// Makes room for one more element at the end of an array and returns its
/// index (`this`).
const ARRAY_ADD_SLOT: u32 = 0x0076_1540;

/// Vtables of the template classes of this range.
const WATER_ZONE_MAP_VTABLE: u32 = 0x0101_5fbc;
const FACTION_MAP_VTABLE: u32 = 0x0101_5fdc;
const DISMEMBERED_LIMB_ARRAY_VTABLE: u32 = 0x0101_5ffc;
const COMBAT_TIME_STAMP_ARRAY_VTABLE: u32 = 0x0101_6010;
const UINT_ARRAY_VTABLE: u32 = 0x0101_6024;
const NI_T_MAP_BASE_VTABLE: u32 = 0x0101_6038;
const BS_MAP_BASE_VTABLE: u32 = 0x0101_6058;

/// `operator delete(this)` when bit 0 of `flags` is set (the tail of every
/// scalar deleting destructor).
fn delete_when_asked(e: &mut Engine, this: Ptr, flags: u32) {
    if flags & 1 != 0 {
        e.call(OPERATOR_DELETE, &args![this]);
    }
}

/// `__RTDynamicCast(other, 0, BSExtraData, ExtraSpecialRenderFlags, 0)`.
fn cast_to_special_render_flags(e: &mut Engine, other: Ptr) -> Ptr<ExtraSpecialRenderFlags> {
    e.call(
        DYNAMIC_CAST,
        &args![
            other,
            0u32,
            BS_EXTRA_DATA_TYPE,
            EXTRA_SPECIAL_RENDER_FLAGS_TYPE,
            0u32
        ],
    )
    .ptr()
}

// Typed from 004382a0
/// `ExtraSpecialRenderFlags::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraSpecialRenderFlags`, when `BSExtraData::Compare` says so, when the
/// flags differ, or when `m_fNodeMaxFadeDistance` differs (a NaN counts as
/// different).
pub fn extra_special_render_flags_compare(
    e: &mut Engine,
    this: Ptr<ExtraSpecialRenderFlags>,
    other: Ptr,
) -> bool {
    let cast = cast_to_special_render_flags(e, other);
    if cast.is_null() {
        return true;
    }
    if e.call(BS_EXTRA_DATA_COMPARE, &args![this, other]).bool() {
        return true;
    }
    let ours = typed_call!(leaf::FAMILY, 0x0084_e3a0, leaf::word_at_c(e, this));
    let theirs = typed_call!(leaf::FAMILY, 0x0084_e3a0, leaf::word_at_c(e, cast));
    if ours != theirs {
        return true;
    }
    // `FUCOMPP`: unordered (NaN) floats count as different.
    e.get(this, ExtraSpecialRenderFlags::m_fNodeMaxFadeDistance)
        != e.get(cast, ExtraSpecialRenderFlags::m_fNodeMaxFadeDistance)
}

// Typed from 00438330
/// `ExtraSpecialRenderFlags::Copy` (Xbox PDB): when `other` is an
/// `ExtraSpecialRenderFlags`, copies its flags (through the setter) and its
/// `m_fNodeMaxFadeDistance`.
pub fn extra_special_render_flags_copy(
    e: &mut Engine,
    this: Ptr<ExtraSpecialRenderFlags>,
    other: Ptr,
) {
    let cast = cast_to_special_render_flags(e, other);
    if cast.is_null() {
        return;
    }
    let flags = typed_call!(leaf::FAMILY, 0x0084_e3a0, leaf::word_at_c(e, cast));
    e.call(SET_FLAGS, &args![this, flags]);
    let distance = e.get(cast, ExtraSpecialRenderFlags::m_fNodeMaxFadeDistance);
    e.set(
        this,
        ExtraSpecialRenderFlags::m_fNodeMaxFadeDistance,
        distance,
    );
}

// Typed from 00438390
/// A forwarding constructor (the engine map has no name for it): runs
/// `004037f0(this, arg, 0)` and returns `this`.
pub fn fn_00438390(e: &mut Engine, this: Ptr, arg: u32) -> Ptr {
    e.call(STRING_INIT, &args![this, arg, 0u32]);
    this
}

// Typed from 004383b0
/// The constructor of the map whose scalar deleting destructor the engine
/// map calls `NiTMap<WaterZone *,int>`: builds the hash map base
/// (`004386b0`, with the bucket count) and then stores the derived vtable.
pub fn fn_004383b0(e: &mut Engine, this: Ptr<NiTPointerMap>, hash_size: u32) -> Ptr<NiTPointerMap> {
    typed_call!(FAMILY, 0x0043_86b0, fn_004386b0(e, this, hash_size));
    e.mem.set_u32(this.addr(), WATER_ZONE_MAP_VTABLE);
    this
}

// Typed from 004383e0
/// The constructor of the map whose scalar deleting destructor the engine
/// map calls `BSMap<TESFaction *,bool>`: builds the base (`00438820`, with
/// the bucket count) and then stores the derived vtable.
pub fn fn_004383e0(e: &mut Engine, this: Ptr<NiTPointerMap>, hash_size: u32) -> Ptr<NiTPointerMap> {
    typed_call!(FAMILY, 0x0043_8820, fn_00438820(e, this, hash_size));
    e.mem.set_u32(this.addr(), FACTION_MAP_VTABLE);
    this
}

// Typed from 00438410
/// `NiTMap<WaterZone *,int>::scalar deleting destructor` (Xbox PDB name of the
/// folded body): runs the destructor (`00438720`) and frees the object when
/// bit 0 of `flags` is set. Returns `this`.
pub fn ni_t_map_scalar_deleting_destructor(e: &mut Engine, this: Ptr, flags: u32) -> Ptr {
    typed_call!(FAMILY, 0x0043_8720, fn_00438720(e, this.cast()));
    delete_when_asked(e, this, flags);
    this
}

// Typed from 00438440
/// `BSMap<TESFaction *,bool>::scalar deleting destructor` (Xbox PDB name of the
/// folded body): runs the destructor (`004388a0`) and frees the object when
/// bit 0 of `flags` is set. Returns `this`.
pub fn bs_map_scalar_deleting_destructor(e: &mut Engine, this: Ptr, flags: u32) -> Ptr {
    typed_call!(FAMILY, 0x0043_88a0, fn_004388a0(e, this.cast()));
    delete_when_asked(e, this, flags);
    this
}

// Typed from 00438470
/// `BSStringT<char>::Set` (Xbox PDB): copies the string `other` into `this`.
/// When it is longer than `sMaxLen` a new buffer of `len + 1` bytes replaces
/// the old one (which is freed); an empty source frees the buffer and nulls
/// it; otherwise the text is copied over the existing buffer. `sLen` is set
/// last. Returns whether the new string is non-empty.
pub fn bs_string_t_set(e: &mut Engine, this: Ptr<BSStringT>, other: Ptr) -> bool {
    let s = store(e);
    let mut length = e.call(STRING_LENGTH, &args![other]).u32();
    let capacity = e.call(STRING_CAPACITY, &args![this]).u32();
    if length > capacity {
        let old = s.get(e, this, BSStringT::pString);
        let buffer = e.call(OPERATOR_NEW, &args![length + 1]).u32();
        s.set(e, this, BSStringT::pString, buffer);
        let text = typed_call!(leaf::FAMILY, 0x0055_9450, leaf::word_at_0(e, other));
        e.call(MEM_COPY, &args![buffer, text, length + 1]);
        if old != 0 {
            e.call(OPERATOR_DELETE, &args![old]);
        }
        e.call(STRING_SET_CAPACITY, &args![this, length]);
    } else if length == 0 {
        let buffer = s.get(e, this, BSStringT::pString);
        if buffer != 0 {
            e.call(OPERATOR_DELETE, &args![buffer]);
        }
        s.set(e, this, BSStringT::pString, 0);
        length = 0;
        e.call(STRING_SET_CAPACITY, &args![this, 0u32]);
    } else {
        let text = typed_call!(leaf::FAMILY, 0x0055_9450, leaf::word_at_0(e, other));
        let buffer = s.get(e, this, BSStringT::pString);
        e.call(MEM_COPY, &args![buffer, text, length + 1]);
    }
    e.call(STRING_SET_LENGTH, &args![this, length]);
    length != 0
}

// Typed from 00438570
/// The constructor of the array whose scalar deleting destructor the engine
/// map calls `BSSimpleArray<DismemberedLimb *,1024>`: stores the vtable and
/// runs the array constructor `006b3eb0(this, 0, 0)` (an empty array).
pub fn fn_00438570(e: &mut Engine, this: Ptr) -> Ptr {
    e.mem.set_u32(this.addr(), DISMEMBERED_LIMB_ARRAY_VTABLE);
    e.call(ARRAY_CONSTRUCT, &args![this, 0u32, 0u32]);
    this
}

// Typed from 004385a0
/// The destructor of the `BSSimpleArray<DismemberedLimb *,1024>`: stores the
/// vtable and clears the array, freeing its buffer (`008454f0(this, 1)`).
pub fn fn_004385a0(e: &mut Engine, this: Ptr) {
    e.mem.set_u32(this.addr(), DISMEMBERED_LIMB_ARRAY_VTABLE);
    e.call(SIMPLE_ARRAY_CLEAR, &args![this, 1u32]);
}

// Typed from 004385c0
/// The constructor of the `BSSimpleArray<CombatTimeStamp,1024>`: stores the
/// vtable and builds an empty array (`00438a60(this, 0, 0)`).
pub fn fn_004385c0(e: &mut Engine, this: Ptr) -> Ptr {
    e.mem.set_u32(this.addr(), COMBAT_TIME_STAMP_ARRAY_VTABLE);
    typed_call!(FAMILY, 0x0043_8a60, fn_00438a60(e, this, 0, 0));
    this
}

// Typed from 004385f0
/// The destructor of the `BSSimpleArray<CombatTimeStamp,1024>`: stores the
/// vtable and clears the array, freeing its buffer.
pub fn fn_004385f0(e: &mut Engine, this: Ptr) {
    e.mem.set_u32(this.addr(), COMBAT_TIME_STAMP_ARRAY_VTABLE);
    e.call(SIMPLE_ARRAY_CLEAR, &args![this, 1u32]);
}

// Typed from 00438610
/// The append of the `BSSimpleArray<CombatTimeStamp,1024>` (the engine map
/// has no name for it): takes the next slot (`00761540`), constructs it
/// (`00430000(this, slot, 1)`), copies in the word `*value` and returns the
/// new index.
pub fn fn_00438610(e: &mut Engine, this: Ptr<BSSimpleArray>, value: Ptr) -> u32 {
    let s = store(e);
    let index = e.call(ARRAY_ADD_SLOT, &args![this]).u32();
    let buffer = s.get(e, this, BSSimpleArray::pBuffer);
    e.call(
        ARRAY_CONSTRUCT_ELEMENTS,
        &args![this, buffer + index * 4, 1u32],
    );
    let word = e.mem.u32(value.addr());
    // The buffer is read again: the slot request can have moved it.
    let buffer = s.get(e, this, BSSimpleArray::pBuffer);
    e.mem.set_u32(buffer + index * 4, word);
    index
}

// Typed from 00438660
/// The constructor of the `BSSimpleArray<unsigned int,1024>`: stores the
/// vtable and runs the array constructor `006b3eb0(this, 0, 0)`.
pub fn fn_00438660(e: &mut Engine, this: Ptr) -> Ptr {
    e.mem.set_u32(this.addr(), UINT_ARRAY_VTABLE);
    e.call(ARRAY_CONSTRUCT, &args![this, 0u32, 0u32]);
    this
}

// Typed from 00438690
/// The destructor of the `BSSimpleArray<unsigned int,1024>`: stores the
/// vtable and clears the array, freeing its buffer.
pub fn fn_00438690(e: &mut Engine, this: Ptr) {
    e.mem.set_u32(this.addr(), UINT_ARRAY_VTABLE);
    e.call(SIMPLE_ARRAY_CLEAR, &args![this, 1u32]);
}

// Typed from 004386b0
/// The `NiTMapBase` constructor (the engine map has no name for it): stores
/// the base vtable and the bucket count, zeroes the item count, allocates
/// the bucket array (`hash_size * 4` bytes, `00aa1070`) and zeroes it.
pub fn fn_004386b0(e: &mut Engine, this: Ptr<NiTPointerMap>, hash_size: u32) -> Ptr<NiTPointerMap> {
    let s = store(e);
    e.mem.set_u32(this.addr(), NI_T_MAP_BASE_VTABLE);
    s.set(e, this, NiTPointerMap::m_uiHashSize, hash_size);
    s.set(e, this, NiTPointerMap::m_uiCount, 0);
    let buckets = e.call(BUCKETS_ALLOCATE, &args![hash_size << 2]).u32();
    s.set(e, this, NiTPointerMap::m_ppkHashTable, buckets);
    let size = s.get(e, this, NiTPointerMap::m_uiHashSize) << 2;
    e.call(MEM_SET, &args![buckets, 0u32, size]);
    this
}

// Typed from 00438720
/// The destructor of the `NiTMap<WaterZone *,int>` (the engine map names the
/// body after a library function, `CMenu::~CMenu`): stores the derived
/// vtable, releases every node (`00438af0`) and runs the base destructor
/// (`00438780`). The exception-unwinding frame is not translated.
pub fn fn_00438720(e: &mut Engine, this: Ptr<NiTPointerMap>) {
    e.mem.set_u32(this.addr(), WATER_ZONE_MAP_VTABLE);
    e.call(BUCKETS_RELEASE, &args![this]);
    typed_call!(FAMILY, 0x0043_8780, fn_00438780(e, this));
}

// Typed from 00438780
/// The `NiTMapBase` destructor body: stores the base vtable, releases every
/// node (`00438af0`) and frees the bucket array (`00aa10f0`).
pub fn fn_00438780(e: &mut Engine, this: Ptr<NiTPointerMap>) {
    let s = store(e);
    e.mem.set_u32(this.addr(), NI_T_MAP_BASE_VTABLE);
    e.call(BUCKETS_RELEASE, &args![this]);
    let buckets = s.get(e, this, NiTPointerMap::m_ppkHashTable);
    e.call(BUCKETS_FREE, &args![buckets]);
}

// Typed from 004387b0
/// `BSSimpleArray<TESBoundObject *,1024>::CompareBuffer<1024>` (Xbox PDB):
/// true when `other` has the same size as `this` and every element (one
/// word) is equal.
pub fn bs_simple_array_compare_buffer(
    e: &mut Engine,
    this: Ptr<BSSimpleArray>,
    other: Ptr<BSSimpleArray>,
) -> bool {
    let s = store(e);
    let other_size = typed_call!(leaf::FAMILY, 0x0044_ddc0, leaf::word_at_8(e, other));
    let our_size = typed_call!(leaf::FAMILY, 0x0044_ddc0, leaf::word_at_8(e, this));
    if other_size != our_size {
        return false;
    }
    let mut index = 0u32;
    while index < s.get(e, this, BSSimpleArray::iSize) {
        let theirs = e.call(SIMPLE_ARRAY_AT, &args![other, index]).u32();
        let ours = e.call(SIMPLE_ARRAY_AT, &args![this, index]).u32();
        if e.mem.u32(theirs) != e.mem.u32(ours) {
            return false;
        }
        index += 1;
    }
    true
}

// Typed from 00438820
/// The `BSMapBase` constructor (the engine map has no name for it): stores
/// the base vtable and the bucket count, zeroes the item count and, for a
/// non-zero count, allocates the bucket array (`operator new`) and zeroes it
/// (`memset`). With a zero count the bucket pointer is left as it was.
pub fn fn_00438820(e: &mut Engine, this: Ptr<NiTPointerMap>, hash_size: u32) -> Ptr<NiTPointerMap> {
    let s = store(e);
    e.mem.set_u32(this.addr(), BS_MAP_BASE_VTABLE);
    s.set(e, this, NiTPointerMap::m_uiHashSize, hash_size);
    s.set(e, this, NiTPointerMap::m_uiCount, 0);
    if hash_size != 0 {
        let bytes = s.get(e, this, NiTPointerMap::m_uiHashSize) << 2;
        let buckets = e.call(OPERATOR_NEW, &args![bytes]).u32();
        s.set(e, this, NiTPointerMap::m_ppkHashTable, buckets);
        let bytes = s.get(e, this, NiTPointerMap::m_uiHashSize) << 2;
        e.call(CRT_MEMSET, &args![buckets, 0u32, bytes]);
    }
    this
}

// Typed from 004388a0
/// The destructor of the `BSMap<TESFaction *,bool>` (the engine map names the
/// body after a folded library function, `ctype<char>::~ctype<char>`):
/// stores the derived vtable, releases every node (`00438af0`) and runs the
/// base destructor (`00438900`). The exception-unwinding frame is not
/// translated.
pub fn fn_004388a0(e: &mut Engine, this: Ptr<NiTPointerMap>) {
    e.mem.set_u32(this.addr(), FACTION_MAP_VTABLE);
    e.call(BUCKETS_RELEASE, &args![this]);
    typed_call!(FAMILY, 0x0043_8900, fn_00438900(e, this));
}

// Typed from 00438900
/// The `BSMapBase` destructor body: stores the base vtable and frees the
/// bucket array when there is one.
pub fn fn_00438900(e: &mut Engine, this: Ptr<NiTPointerMap>) {
    let s = store(e);
    e.mem.set_u32(this.addr(), BS_MAP_BASE_VTABLE);
    let buckets = s.get(e, this, NiTPointerMap::m_ppkHashTable);
    if buckets != 0 {
        e.call(OPERATOR_DELETE, &args![buckets]);
    }
}

// Typed from 00438940
/// A map node release (the engine map has no name for it; `this` is not
/// used): clears the byte at +8 of `node` and frees it.
pub fn fn_00438940(e: &mut Engine, _this: Ptr, node: Ptr) {
    e.mem.set_u8(node.addr() + 8, 0);
    e.call(OPERATOR_DELETE, &args![node]);
}

// Typed from 00438970
/// `BSSimpleArray<DismemberedLimb *,1024>::scalar deleting destructor` (Xbox
/// PDB name of the folded body): runs the destructor (`004385a0`) and frees
/// the object when bit 0 of `flags` is set. Returns `this`.
pub fn bs_simple_array_dismembered_limb_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr,
    flags: u32,
) -> Ptr {
    typed_call!(FAMILY, 0x0043_85a0, fn_004385a0(e, this));
    delete_when_asked(e, this, flags);
    this
}

// Typed from 004389a0
/// `BSSimpleArray<CombatTimeStamp,1024>::scalar deleting destructor` (Xbox PDB
/// name of the folded body): runs the destructor (`004385f0`) and frees the
/// object when bit 0 of `flags` is set. Returns `this`.
pub fn bs_simple_array_combat_time_stamp_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr,
    flags: u32,
) -> Ptr {
    typed_call!(FAMILY, 0x0043_85f0, fn_004385f0(e, this));
    delete_when_asked(e, this, flags);
    this
}

// Typed from 004389d0
/// `BSSimpleArray<unsigned int,1024>::scalar deleting destructor` (Xbox PDB
/// name of the folded body): runs the destructor (`00438690`) and frees the
/// object when bit 0 of `flags` is set. Returns `this`.
pub fn bs_simple_array_uint_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr,
    flags: u32,
) -> Ptr {
    typed_call!(FAMILY, 0x0043_8690, fn_00438690(e, this));
    delete_when_asked(e, this, flags);
    this
}

// Typed from 00438a00
/// `NiTMapBase<...>::scalar deleting destructor` (Xbox PDB name of the folded
/// body): runs the base destructor (`00438780`) and frees the object when
/// bit 0 of `flags` is set. Returns `this`.
pub fn ni_t_map_base_scalar_deleting_destructor(e: &mut Engine, this: Ptr, flags: u32) -> Ptr {
    typed_call!(FAMILY, 0x0043_8780, fn_00438780(e, this.cast()));
    delete_when_asked(e, this, flags);
    this
}

// Typed from 00438a30
/// `BSMapBase<TESFaction *,bool>::scalar deleting destructor` (Xbox PDB name of
/// the folded body): runs the base destructor (`00438900`) and frees the
/// object when bit 0 of `flags` is set. Returns `this`.
pub fn bs_map_base_scalar_deleting_destructor(e: &mut Engine, this: Ptr, flags: u32) -> Ptr {
    typed_call!(FAMILY, 0x0043_8900, fn_00438900(e, this.cast()));
    delete_when_asked(e, this, flags);
    this
}

// Typed from 00438a60
/// The constructor of the `BSSimpleArray<CombatTimeStamp,1024>` body (the
/// engine map has no name for it; the vtable is stored by the caller):
/// zeroes the buffer, size and reserved words, takes the larger of the two
/// counts as the reservation, allocates it through vtable slot 4 and
/// constructs `size` elements (`00430000`).
pub fn fn_00438a60(e: &mut Engine, this: Ptr, reserve: u32, size: u32) {
    let s = store(e);
    let array: Ptr<BSSimpleArray> = this.cast();
    s.set(e, array, BSSimpleArray::pBuffer, 0);
    s.set(e, array, BSSimpleArray::iSize, 0);
    s.set(e, array, BSSimpleArray::iReservedSize, 0);
    let reserve = reserve.max(size);
    if reserve != 0 {
        let buffer = e.vcall(this.addr(), 4, &args![reserve]).u32();
        s.set(e, array, BSSimpleArray::pBuffer, buffer);
        s.set(e, array, BSSimpleArray::iReservedSize, reserve);
    }
    if size != 0 {
        let buffer = s.get(e, array, BSSimpleArray::pBuffer);
        e.call(ARRAY_CONSTRUCT_ELEMENTS, &args![this, buffer, size]);
        s.set(e, array, BSSimpleArray::iSize, size);
    }
}

/// This file's functions, by exe address.
#[rustfmt::skip]
pub fn entries() -> Vec<(u32, AbiFn)> {
    vec![
        entry!(0x0043_82a0,
            extra_special_render_flags_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_8330,
            extra_special_render_flags_copy(Ptr<_>, Ptr<_>)
        ),
        entry!(0x0043_8390, fn_00438390(Ptr<_>, u32) -> Ptr<_>),
        entry!(0x0043_83b0,
            fn_004383b0(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_83e0,
            fn_004383e0(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_8410,
            ni_t_map_scalar_deleting_destructor(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_8440,
            bs_map_scalar_deleting_destructor(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_8470, bs_string_t_set(Ptr<_>, Ptr<_>) -> bool),
        entry!(0x0043_8570, fn_00438570(Ptr<_>) -> Ptr<_>),
        entry!(0x0043_85a0, fn_004385a0(Ptr<_>)),
        entry!(0x0043_85c0, fn_004385c0(Ptr<_>) -> Ptr<_>),
        entry!(0x0043_85f0, fn_004385f0(Ptr<_>)),
        entry!(0x0043_8610, fn_00438610(Ptr<_>, Ptr<_>) -> u32),
        entry!(0x0043_8660, fn_00438660(Ptr<_>) -> Ptr<_>),
        entry!(0x0043_8690, fn_00438690(Ptr<_>)),
        entry!(0x0043_86b0,
            fn_004386b0(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_8720, fn_00438720(Ptr<_>)),
        entry!(0x0043_8780, fn_00438780(Ptr<_>)),
        entry!(0x0043_87b0,
            bs_simple_array_compare_buffer(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_8820,
            fn_00438820(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_88a0, fn_004388a0(Ptr<_>)),
        entry!(0x0043_8900, fn_00438900(Ptr<_>)),
        entry!(0x0043_8940, fn_00438940(Ptr<_>, Ptr<_>)),
        entry!(0x0043_8970,
            bs_simple_array_dismembered_limb_scalar_deleting_destructor(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_89a0,
            bs_simple_array_combat_time_stamp_scalar_deleting_destructor(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_89d0,
            bs_simple_array_uint_scalar_deleting_destructor(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_8a00,
            ni_t_map_base_scalar_deleting_destructor(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_8a30,
            bs_map_base_scalar_deleting_destructor(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_8a60, fn_00438a60(Ptr<_>, u32, u32)),
    ]
}
