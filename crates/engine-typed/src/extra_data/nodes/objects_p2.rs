// SPDX-License-Identifier: GPL-3.0-only
//! The extra data node classes' functions of the extradataobjects_p2 unit (nv-rs#952, step 1).

use super::layouts::*;
use super::objects::*;
use super::{round_to_f32, store, FAMILY};
use crate::dispatch;
use engine_core::prelude::*;
use engine_core::types::*;
use engine_core::AbiFn;

/// `operator delete(pointer)` (cdecl, one stack argument).
const OPERATOR_DELETE: u32 = 0x0040_1030;
/// `BSExtraData::BSExtraData(type)` (the base constructor; `this`, then the
/// extra-data type byte).
const BS_EXTRA_DATA_CONSTRUCT: u32 = 0x0040_ec80;
/// `BSExtraData::~BSExtraData` (the base destructor body).
const BS_EXTRA_DATA_DESTRUCT: u32 = 0x0040_ecb0;
/// `__RTDynamicCast(object, vfDelta, sourceType, targetType, isReference)`.
const DYNAMIC_CAST: u32 = 0x00ec_43fb;
/// `RTTI Type Descriptor` of `BSExtraData`, the source type of the
/// `dynamic_cast`s in the `Compare` methods.
const BS_EXTRA_DATA_TYPE: u32 = 0x0118_3b2c;
/// `BSSimpleArray<T,1024>::operator[]` of the folded body: the address of
/// the element slot.
const SIMPLE_ARRAY_AT: u32 = 0x006a_7ad0;
/// The array element removal `RemoveOldHits` uses (`this` is the array; the
/// arguments are the index and the flag 1). The engine map names the folded
/// body `BSSimpleArray<BGSBodyPart_P_1024>::_MoveItems`.
const SIMPLE_ARRAY_REMOVE_AT: u32 = 0x009a_4320;
/// The array append `AddHit` uses (`this` is the array; the argument is the
/// address of the element to copy in; returns the new index).
const SIMPLE_ARRAY_APPEND: u32 = 0x0043_8610;
/// The `ExtraFriendHits::Hits` array's constructor (`this` is the array).
const HITS_ARRAY_CONSTRUCT: u32 = 0x0043_85c0;
/// The `ExtraFriendHits::Hits` array's destructor (`this` is the array).
const HITS_ARRAY_DESTRUCT: u32 = 0x0043_85f0;
/// `MultiBoundMarkerData::Compare` (Xbox PDB; `this`, other): true when
/// `other` is null or the two differ.
const MULTI_BOUND_MARKER_DATA_COMPARE: u32 = 0x0043_9060;
/// `NiPointer<T>::NiPointer(pointer)` (`this` is the smart pointer): stores
/// the pointer and takes a reference when it is not null.
const NI_POINTER_CONSTRUCT: u32 = 0x0063_3c90;
/// `NiPointer<T>::operator=(pointer)` (`this` is the smart pointer).
const NI_POINTER_ASSIGN: u32 = 0x0066_b0d0;
/// `NiPointer<T>::~NiPointer` (`this` is the smart pointer): releases the
/// reference when it holds one.
const NI_POINTER_DESTRUCT: u32 = 0x0045_cec0;
/// The constructor of the `BSSimpleList<TESObjectREFR *>` inside
/// `ExtraDroppedItemList` (`this` is the list; empties it).
const SIMPLE_LIST_CONSTRUCT: u32 = 0x0096_a2d0;
/// The list's node release (`this` is the list; walks the nodes and frees
/// them).
const SIMPLE_LIST_CLEAR: u32 = 0x0047_0470;
/// The list's destructor body (`this` is the list; it runs the node release
/// again).
const SIMPLE_LIST_DESTRUCT: u32 = 0x0046_ffb0;
/// The word store `ExtraSavedAnimation::DeleteBuffer` builds its holder with
/// (`this` is the holder, the argument the buffer pointer; returns `this`).
/// The engine map names the folded body `BGSChangeFlags::IsForcedChange`.
const HOLDER_CONSTRUCT: u32 = 0x008c_71b0;
/// The destructor of `ExtraSavedAnimation` (not in Ghidra's function list).
const EXTRA_SAVED_ANIMATION_DESTRUCT: u32 = 0x0043_5ac0;
/// The destructor of `ExtraSavedHavokData` (not in Ghidra's function list).
const EXTRA_SAVED_HAVOK_DATA_DESTRUCT: u32 = 0x0043_5bb0;
/// `ExtraRefractionProperty`'s destructor body (reached by
/// address from its scalar deleting destructor).
const EXTRA_REFRACTION_PROPERTY_DESTRUCT: u32 = 0x0043_5f80;
/// A float game-setting accessor: `this` is the setting (an exe global), the
/// result is the address of its value.
const SETTING_FLOAT_VALUE: u32 = 0x0040_3e20;
/// The float setting `AddHit` compares the age of the newest hit with.
const HIT_SETTING_ADD: u32 = 0x011c_d78c;
/// The float setting `RemoveOldHits` compares the age of each hit with.
const HIT_SETTING_REMOVE: u32 = 0x011c_d580;
/// The global float `00435dd0` returns (the time the hit ages are measured
/// against).
const HIT_CLOCK: u32 = 0x011f_1bf0;

/// Extra-data type bytes, as passed to the base constructor.
const TYPE_EMITTANCE_SOURCE: u32 = 0x67;
const TYPE_MULTI_BOUND_REF: u32 = 0x63;
const TYPE_MULTI_BOUND_DATA: u32 = 0x62;
const TYPE_MULTI_BOUND: u32 = 0x61;
const TYPE_OCCLUSION_PLANE: u32 = 0x71;
const TYPE_PORTAL: u32 = 0x78;
const TYPE_ROOM: u32 = 0x79;
const TYPE_ITEM_DROPPER: u32 = 0x39;
const TYPE_DROPPED_ITEM_LIST: u32 = 0x3a;
const TYPE_SAVED_ANIMATION: u32 = 0x42;
const TYPE_SAVED_HAVOK_DATA: u32 = 0x3d;
const TYPE_FRIEND_HITS: u32 = 0x45;
const TYPE_HEADING_TARGET: u32 = 0x46;
const TYPE_REFRACTION_PROPERTY: u32 = 0x48;

/// The vtables.
const EXTRA_EMITTANCE_SOURCE_VTABLE: u32 = 0x0101_5e34;
const EXTRA_MULTI_BOUND_REF_VTABLE: u32 = 0x0101_5e40;
const EXTRA_MULTI_BOUND_DATA_VTABLE: u32 = 0x0101_5e4c;
const EXTRA_MULTI_BOUND_VTABLE: u32 = 0x0101_5e58;
const EXTRA_OCCLUSION_PLANE_VTABLE: u32 = 0x0101_5e64;
const EXTRA_PORTAL_VTABLE: u32 = 0x0101_5e70;
const EXTRA_ROOM_VTABLE: u32 = 0x0101_5e7c;
const EXTRA_ITEM_DROPPER_VTABLE: u32 = 0x0101_5e88;
const EXTRA_DROPPED_ITEM_LIST_VTABLE: u32 = 0x0101_5e94;
const EXTRA_SAVED_ANIMATION_VTABLE: u32 = 0x0101_5ea0;
const EXTRA_SAVED_HAVOK_DATA_VTABLE: u32 = 0x0101_5eac;
const EXTRA_FRIEND_HITS_VTABLE: u32 = 0x0101_5eb8;
const EXTRA_HEADING_TARGET_VTABLE: u32 = 0x0101_5968;
const EXTRA_REFRACTION_PROPERTY_VTABLE: u32 = 0x0101_5ec4;

/// `RTTI Type Descriptor`s of the classes whose `Compare` casts `other`.
const EXTRA_X_TARGET_TYPE: u32 = 0x0118_4f10;
const EXTRA_EMITTANCE_SOURCE_TYPE: u32 = 0x0118_4f2c;
const EXTRA_MULTI_BOUND_REF_TYPE: u32 = 0x0118_4f50;
const EXTRA_MULTI_BOUND_DATA_TYPE: u32 = 0x0118_4f74;

/// `operator new(size)` (cdecl, one stack argument).
const OPERATOR_NEW: u32 = 0x0040_1000;
/// `BSExtraData::Compare` (Xbox PDB; `this`, other).
const BS_EXTRA_DATA_COMPARE: u32 = 0x0040_f700;
/// `memcmp(a, b, size)`.
const MEMCMP: u32 = 0x00ec_4835;
/// The list's insertion of the item whose pointer is at the address given
/// (`this` is the list).
const SIMPLE_LIST_ADD_HEAD: u32 = 0x005a_e3d0;
/// The list's scalar deleting destructor (`this` is the list, the argument
/// is the flags; with bit 0 it frees the list).
const SIMPLE_LIST_DELETE: u32 = 0x0047_02f0;
/// `BSSoundHandle`'s default constructor (the id -1, the byte 0, the state
/// 0; `this` is the handle).
const SOUND_HANDLE_CONSTRUCT: u32 = 0x0041_a250;
/// `BSSoundHandle`'s copy (`this` is the destination, the argument the
/// source handle).
const SOUND_HANDLE_ASSIGN: u32 = 0x0041_8900;
/// `BSSoundHandle`'s destructor (it does nothing; `this` is the handle).
const SOUND_HANDLE_DESTRUCT: u32 = 0x0048_3710;
/// `MobileObject::MobileObject` (Xbox PDB; `this` is the 0x88-byte block).
const MOBILE_OBJECT_CONSTRUCT: u32 = 0x0092_eb50;
/// `TESObjectREFR::SetObjectReference` (Xbox PDB; `this`, base object).
const SET_OBJECT_REFERENCE: u32 = 0x0057_5690;
/// `TESObjectREFR::GetRefPersists` (Xbox PDB; the flag is returned in AL).
const GET_REF_PERSISTS: u32 = 0x0056_53d0;
/// `TESObjectREFR::SetRefPersists` (Xbox PDB; `this`, the flag).
const SET_REF_PERSISTS: u32 = 0x0056_5480;
/// `TESObjectREFR::SetLocationOnReference` (Xbox PDB; `this`, the address of
/// a 12-byte vector).
const SET_LOCATION_ON_REFERENCE: u32 = 0x0057_5830;
/// A `TESObjectREFR` method (`this`, three float words), the rotation.
const SET_ROTATION: u32 = 0x0057_5700;
/// A method of the new object that stores its argument in the word at +0x6c.
const SET_SOURCE_REFERENCE: u32 = 0x004f_bf00;
/// A method that stores its byte argument at +0x81.
const SET_FLAG_BYTE_81: u32 = 0x0089_8280;
/// A `MobileObject` initialisation method (`this` only).
const OBJECT_INIT: u32 = 0x0093_5ae0;
/// An accessor: the word at +0x40 (`TESObjectREFR`'s parent cell).
const GET_PARENT_CELL: u32 = 0x008d_6f30;
/// A predicate on a cell: bit 0 of the byte at +0x24.
const CELL_CHECK: u32 = 0x0042_5fd0;
/// An accessor: the address `this + 0x44` (the embedded extra data list).
const GET_EXTRA_LIST: u32 = 0x005d_43c0;
/// `ExtraDataList::SetPersistentCell` (Xbox PDB; `this`, the cell).
const SET_PERSISTENT_CELL: u32 = 0x0041_d390;
/// The method that stores its argument in the word at +0x68
/// (`MiddleHighProcess::SetSavedAcquireObject` in the engine map).
const SET_PROCESS: u32 = 0x0040_7800;
/// `HighProcess::HighProcess` (Xbox PDB), process kind 0, 0x46c bytes.
const HIGH_PROCESS_CONSTRUCT: u32 = 0x008d_7510;
/// The constructor of the 0x25c-byte process, kind 1.
const PROCESS_CONSTRUCT_25C: u32 = 0x0091_3fe0;
/// The constructor of the 200-byte process, kind 2.
const PROCESS_CONSTRUCT_0C8: u32 = 0x0092_c950;
/// The constructor of the 0xb4-byte process, kind 3.
const PROCESS_CONSTRUCT_0B4: u32 = 0x0090_6dc0;
/// `ProcessLists::AddReference` (Xbox PDB; `this` is [`PROCESS_LISTS`],
/// then the object, the process kind and three zero words).
const PROCESS_LISTS_ADD_REFERENCE: u32 = 0x0096_d450;
/// The `ProcessLists` singleton object.
const PROCESS_LISTS: u32 = 0x011e_0e80;
/// The global holding the `PlayerCharacter` pointer.
const PLAYER: u32 = 0x011d_ea3c;
/// The constructor of the faction map `ExtraFactionChanges` builds on its
/// stack (`this` is the 0x10-byte map, the argument the bucket count).
const FACTION_MAP_CONSTRUCT: u32 = 0x0043_83e0;
/// The faction map's destructor (`this` is the map).
const FACTION_MAP_DESTRUCT: u32 = 0x0043_88a0;
/// The faction map's insertion or update (`this`, key, value byte).
const FACTION_MAP_SET_AT: u32 = 0x0055_94e0;
/// The faction map's lookup (`this`, key, address of the value byte;
/// returns whether the key was found).
const FACTION_MAP_LOOKUP: u32 = 0x0057_c850;
/// An accessor: the address `this + 0x2c` (the faction list embedded in a
/// base actor record).
const FACTION_LIST_OF: u32 = 0x005d_8a70;

/// Extra-data type bytes of this session's classes.
const TYPE_EDITOR_REF_MOVE_DATA: u32 = 0x4c;
const TYPE_HAS_NO_RUMORS: u32 = 0x4e;
const TYPE_SOUND: u32 = 0x4f;
const TYPE_CREATURE_AWAKE_SOUND: u32 = 0x7d;
const TYPE_CREATURE_MOVEMENT_SOUND: u32 = 0x8a;
const TYPE_WEAPON_IDLE_SOUND: u32 = 0x83;
const TYPE_WEAPON_ATTACK_SOUND: u32 = 0x86;
const TYPE_ACTIVATE_LOOP_SOUND: u32 = 0x87;
const TYPE_TALKING_ACTOR: u32 = 0x55;
const TYPE_FACTION_CHANGES: u32 = 0x5e;

/// The vtables of this session's classes.
const EXTRA_EDITOR_REF_MOVE_DATA_VTABLE: u32 = 0x0101_5ed0;
const EXTRA_HAS_NO_RUMORS_VTABLE: u32 = 0x0101_5974;
const EXTRA_SOUND_VTABLE: u32 = 0x0101_5edc;
const EXTRA_CREATURE_AWAKE_SOUND_VTABLE: u32 = 0x0101_5ee8;
const EXTRA_CREATURE_MOVEMENT_SOUND_VTABLE: u32 = 0x0101_5ef4;
const EXTRA_WEAPON_IDLE_SOUND_VTABLE: u32 = 0x0101_5f00;
const EXTRA_WEAPON_ATTACK_SOUND_VTABLE: u32 = 0x0101_5f0c;
const EXTRA_ACTIVATE_LOOP_SOUND_VTABLE: u32 = 0x0101_5f18;
const EXTRA_TALKING_ACTOR_VTABLE: u32 = 0x0101_5f24;
const EXTRA_FACTION_CHANGES_VTABLE: u32 = 0x0101_5f30;

/// `RTTI Type Descriptor`s of the classes whose `Compare` casts `other`.
const EXTRA_RADIUS_TYPE: u32 = 0x0118_43a8;
const EXTRA_RADIATION_TYPE: u32 = 0x0118_43c4;
const EXTRA_RADIO_DATA_TYPE: u32 = 0x0118_41e8;

/// `operator delete(this)` when bit 0 of `flags` is set (the tail of every
/// scalar deleting destructor).
fn delete_when_asked(e: &mut Engine, this: Ptr, flags: u32) {
    if flags & 1 != 0 {
        e.call(OPERATOR_DELETE, &args![this]);
    }
}

/// The base constructor, then the vtable store that follows it.
fn construct_base(e: &mut Engine, this: Ptr, extra_type: u32, vtable: u32) {
    e.call(BS_EXTRA_DATA_CONSTRUCT, &args![this, extra_type]);
    e.mem.set_u32(this.addr(), vtable);
}

/// The address of the payload word at +0xc.
fn payload<T>(this: Ptr<T>) -> Ptr {
    Ptr::new(this.addr() + 0xc)
}

/// A constructor that stores a null pointer in the payload word.
fn construct_null_payload(e: &mut Engine, this: Ptr, extra_type: u32, vtable: u32) {
    construct_base(e, this, extra_type, vtable);
    e.mem.set_u32(this.addr() + 0xc, 0);
}

/// A constructor of the classes that hold a `NiPointer` at +0xc: the smart
/// pointer is constructed from null, then assigned null. The
/// exception-unwinding frame is not translated.
fn construct_ni_pointer_holder(e: &mut Engine, this: Ptr, extra_type: u32, vtable: u32) {
    construct_base(e, this, extra_type, vtable);
    e.call(NI_POINTER_CONSTRUCT, &args![payload(this), 0u32]);
    e.call(NI_POINTER_ASSIGN, &args![payload(this), 0u32]);
}

/// The compare of the classes whose only payload is one word at +0xc: casts
/// `other` to the class with the RTTI descriptor `target_type` (true when it
/// is not one), then reports whether the words differ.
fn compare_payload_word<T>(e: &mut Engine, this: Ptr<T>, other: Ptr, target_type: u32) -> bool {
    let cast: Ptr = e
        .call(
            DYNAMIC_CAST,
            &args![other, 0u32, BS_EXTRA_DATA_TYPE, target_type, 0u32],
        )
        .ptr();
    if cast.is_null() {
        return true;
    }
    e.mem.u32(this.addr() + 0xc) != e.mem.u32(cast.addr() + 0xc)
}

// Typed from 004353f0
/// `ExtraXTarget::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraXTarget` or when `pTarget` differs (the base compare is not asked).
pub fn extra_x_target_compare(e: &mut Engine, this: Ptr<ExtraXTarget>, other: Ptr) -> bool {
    compare_payload_word(e, this, other, EXTRA_X_TARGET_TYPE)
}

// Typed from 00435440
/// `ExtraEmittanceSource`'s constructor (the engine map has no name for it):
/// extra-data type 0x67, no source. Returns `this`.
pub fn fn_00435440(e: &mut Engine, this: Ptr<ExtraEmittanceSource>) -> Ptr<ExtraEmittanceSource> {
    construct_null_payload(
        e,
        this.cast(),
        TYPE_EMITTANCE_SOURCE,
        EXTRA_EMITTANCE_SOURCE_VTABLE,
    );
    this
}

// Typed from 00435470
/// `ExtraEmittanceSource`'s scalar deleting destructor (the engine map has no
/// name for it): the destructor, then `operator delete` when `flags & 1`.
/// Returns `this`.
pub fn fn_00435470(
    e: &mut Engine,
    this: Ptr<ExtraEmittanceSource>,
    flags: u32,
) -> Ptr<ExtraEmittanceSource> {
    typed_call!(FAMILY, 0x0043_54a0, fn_004354a0(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 004354a0
/// `ExtraEmittanceSource`'s destructor body (the engine map has no name for
/// it): resets the vtable, then runs the base destructor. The source is not
/// owned.
pub fn fn_004354a0(e: &mut Engine, this: Ptr<ExtraEmittanceSource>) {
    e.mem.set_u32(this.addr(), EXTRA_EMITTANCE_SOURCE_VTABLE);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 004354c0
/// `ExtraEmittanceSource::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraEmittanceSource` or when `pSource` differs.
pub fn extra_emittance_source_compare(
    e: &mut Engine,
    this: Ptr<ExtraEmittanceSource>,
    other: Ptr,
) -> bool {
    compare_payload_word(e, this, other, EXTRA_EMITTANCE_SOURCE_TYPE)
}

// Typed from 00435510
/// `ExtraMultiBoundRef`'s constructor (the engine map has no name for it):
/// extra-data type 0x63, no reference. Returns `this`.
pub fn fn_00435510(e: &mut Engine, this: Ptr<ExtraMultiBoundRef>) -> Ptr<ExtraMultiBoundRef> {
    construct_null_payload(
        e,
        this.cast(),
        TYPE_MULTI_BOUND_REF,
        EXTRA_MULTI_BOUND_REF_VTABLE,
    );
    this
}

// Typed from 00435540
/// `ExtraMultiBoundRef::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraMultiBoundRef` or when `pBoundRef` differs.
pub fn extra_multi_bound_ref_compare(
    e: &mut Engine,
    this: Ptr<ExtraMultiBoundRef>,
    other: Ptr,
) -> bool {
    compare_payload_word(e, this, other, EXTRA_MULTI_BOUND_REF_TYPE)
}

// Typed from 00435590
/// `ExtraMultiBoundData`'s constructor (the engine map has no name for it):
/// extra-data type 0x62, no bound data. Returns `this`.
pub fn fn_00435590(e: &mut Engine, this: Ptr<ExtraMultiBoundData>) -> Ptr<ExtraMultiBoundData> {
    construct_null_payload(
        e,
        this.cast(),
        TYPE_MULTI_BOUND_DATA,
        EXTRA_MULTI_BOUND_DATA_VTABLE,
    );
    this
}

// Typed from 004355c0
/// `ExtraMultiBoundData::_scalar_deleting_destructor_` (Xbox PDB): the
/// destructor, then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_multi_bound_data_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraMultiBoundData>,
    flags: u32,
) -> Ptr<ExtraMultiBoundData> {
    typed_call!(FAMILY, 0x0043_55f0, fn_004355f0(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 004355f0
/// `ExtraMultiBoundData`'s destructor body (the engine map has no name for
/// it): resets the vtable, `operator delete`s the owned bound data (without a
/// null test), then runs the base destructor.
pub fn fn_004355f0(e: &mut Engine, this: Ptr<ExtraMultiBoundData>) {
    let s = store(e);
    e.mem.set_u32(this.addr(), EXTRA_MULTI_BOUND_DATA_VTABLE);
    let bound = s.get(e, this, ExtraMultiBoundData::pBound);
    e.call(OPERATOR_DELETE, &args![bound]);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 00435630
/// `ExtraMultiBoundData::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraMultiBoundData`, or when `MultiBoundMarkerData::Compare` (called on
/// this one's `pBound` with the other's) says they differ.
pub fn extra_multi_bound_data_compare(
    e: &mut Engine,
    this: Ptr<ExtraMultiBoundData>,
    other: Ptr,
) -> bool {
    let s = store(e);
    let cast: Ptr<ExtraMultiBoundData> = e
        .call(
            DYNAMIC_CAST,
            &args![
                other,
                0u32,
                BS_EXTRA_DATA_TYPE,
                EXTRA_MULTI_BOUND_DATA_TYPE,
                0u32
            ],
        )
        .ptr();
    if cast.is_null() {
        return true;
    }
    let other_bound = s.get(e, cast, ExtraMultiBoundData::pBound);
    let bound = s.get(e, this, ExtraMultiBoundData::pBound);
    e.call(MULTI_BOUND_MARKER_DATA_COMPARE, &args![bound, other_bound])
        .bool()
}

// Typed from 00435690
/// `ExtraMultiBound`'s constructor (the engine map has no name for it):
/// extra-data type 0x61, an empty `NiPointer` (constructed from null, then
/// assigned null). Returns `this`.
pub fn fn_00435690(e: &mut Engine, this: Ptr<ExtraMultiBound>) -> Ptr<ExtraMultiBound> {
    construct_ni_pointer_holder(e, this.cast(), TYPE_MULTI_BOUND, EXTRA_MULTI_BOUND_VTABLE);
    this
}

// Typed from 00435710
/// `ExtraMultiBound::_scalar_deleting_destructor_` (Xbox PDB): the
/// destructor, then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_multi_bound_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraMultiBound>,
    flags: u32,
) -> Ptr<ExtraMultiBound> {
    typed_call!(FAMILY, 0x0043_5740, fn_00435740(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00435740
/// `ExtraMultiBound`'s destructor body (the engine map has no name for it):
/// destroys the `NiPointer` at +0xc, then runs the base destructor (the
/// vtable is not stored again). The exception-unwinding frame is not
/// translated.
pub fn fn_00435740(e: &mut Engine, this: Ptr<ExtraMultiBound>) {
    e.call(NI_POINTER_DESTRUCT, &args![payload(this)]);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 004357a0
/// `ExtraOcclusionPlane`'s constructor (the engine map has no name for it):
/// extra-data type 0x71, an empty `NiPointer`. Returns `this`.
pub fn fn_004357a0(e: &mut Engine, this: Ptr<ExtraOcclusionPlane>) -> Ptr<ExtraOcclusionPlane> {
    construct_ni_pointer_holder(
        e,
        this.cast(),
        TYPE_OCCLUSION_PLANE,
        EXTRA_OCCLUSION_PLANE_VTABLE,
    );
    this
}

// Typed from 00435820
/// `ExtraPortal`'s constructor (the engine map has no name for it):
/// extra-data type 0x78, an empty `NiPointer`. Returns `this`.
pub fn fn_00435820(e: &mut Engine, this: Ptr<ExtraPortal>) -> Ptr<ExtraPortal> {
    construct_ni_pointer_holder(e, this.cast(), TYPE_PORTAL, EXTRA_PORTAL_VTABLE);
    this
}

// Typed from 004358a0
/// `ExtraRoom`'s constructor (the engine map has no name for it):
/// extra-data type 0x79, an empty `NiPointer`. Returns `this`.
pub fn fn_004358a0(e: &mut Engine, this: Ptr<ExtraRoom>) -> Ptr<ExtraRoom> {
    construct_ni_pointer_holder(e, this.cast(), TYPE_ROOM, EXTRA_ROOM_VTABLE);
    this
}

// Typed from 00435920
/// `ExtraItemDropper`'s constructor (the engine map has no name for it):
/// extra-data type 0x39, no dropper. Returns `this`.
pub fn fn_00435920(e: &mut Engine, this: Ptr<ExtraItemDropper>) -> Ptr<ExtraItemDropper> {
    construct_null_payload(e, this.cast(), TYPE_ITEM_DROPPER, EXTRA_ITEM_DROPPER_VTABLE);
    this
}

// Typed from 00435950
/// `ExtraDroppedItemList`'s constructor (the engine map has no name for it):
/// extra-data type 0x3a, then the constructor of the embedded list at +0xc.
/// Returns `this`. The exception-unwinding frame is not translated.
pub fn fn_00435950(e: &mut Engine, this: Ptr<ExtraDroppedItemList>) -> Ptr<ExtraDroppedItemList> {
    construct_base(
        e,
        this.cast(),
        TYPE_DROPPED_ITEM_LIST,
        EXTRA_DROPPED_ITEM_LIST_VTABLE,
    );
    e.call(SIMPLE_LIST_CONSTRUCT, &args![payload(this)]);
    this
}

// Typed from 004359c0
/// `ExtraDroppedItemList::_scalar_deleting_destructor_` (Xbox PDB): the
/// destructor, then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_dropped_item_list_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraDroppedItemList>,
    flags: u32,
) -> Ptr<ExtraDroppedItemList> {
    typed_call!(FAMILY, 0x0043_59f0, fn_004359f0(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 004359f0
/// `ExtraDroppedItemList`'s destructor body (the engine map has no name for
/// it): resets the vtable, releases the embedded list's nodes, runs the
/// list's destructor body, then the base destructor. The exception-unwinding
/// frame is not translated.
pub fn fn_004359f0(e: &mut Engine, this: Ptr<ExtraDroppedItemList>) {
    e.mem.set_u32(this.addr(), EXTRA_DROPPED_ITEM_LIST_VTABLE);
    e.call(SIMPLE_LIST_CLEAR, &args![payload(this)]);
    e.call(SIMPLE_LIST_DESTRUCT, &args![payload(this)]);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 00435a60
/// `ExtraSavedAnimation`'s constructor (the engine map has no name for it):
/// extra-data type 0x42, no buffer. Returns `this`.
pub fn fn_00435a60(e: &mut Engine, this: Ptr<ExtraSavedAnimation>) -> Ptr<ExtraSavedAnimation> {
    construct_null_payload(
        e,
        this.cast(),
        TYPE_SAVED_ANIMATION,
        EXTRA_SAVED_ANIMATION_VTABLE,
    );
    this
}

// Typed from 00435a90
/// `ExtraSavedAnimation::_scalar_deleting_destructor_` (Xbox PDB): the
/// destructor (`00435ac0`, which Ghidra has no function for, called by
/// address), then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_saved_animation_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraSavedAnimation>,
    flags: u32,
) -> Ptr<ExtraSavedAnimation> {
    e.call(EXTRA_SAVED_ANIMATION_DESTRUCT, &args![this]);
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00435b20
/// Releases the buffer a pointer holder owns (the engine map has no name for
/// it): `this` is the address of the holder's pointer; the pointed-to block
/// is `operator delete`d (without a null test) and the pointer set to null.
pub fn fn_00435b20(e: &mut Engine, this: Ptr) {
    let buffer = e.mem.u32(this.addr());
    e.call(OPERATOR_DELETE, &args![buffer]);
    e.mem.set_u32(this.addr(), 0);
}

// Typed from 00435b50
/// `ExtraSavedHavokData`'s constructor (the engine map has no name for it):
/// extra-data type 0x3d, no buffer. Returns `this`.
pub fn fn_00435b50(e: &mut Engine, this: Ptr<ExtraSavedHavokData>) -> Ptr<ExtraSavedHavokData> {
    construct_null_payload(
        e,
        this.cast(),
        TYPE_SAVED_HAVOK_DATA,
        EXTRA_SAVED_HAVOK_DATA_VTABLE,
    );
    this
}

// Typed from 00435b80
/// `ExtraSavedHavokData::_scalar_deleting_destructor_` (Xbox PDB): the
/// destructor (`00435bb0`, which Ghidra has no function for, called by
/// address), then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_saved_havok_data_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraSavedHavokData>,
    flags: u32,
) -> Ptr<ExtraSavedHavokData> {
    e.call(EXTRA_SAVED_HAVOK_DATA_DESTRUCT, &args![this]);
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00435c10
/// `ExtraSavedAnimation::DeleteBuffer` (Xbox PDB): builds a one-word holder
/// on the stack around `pAnimationBuffer` and releases it (`operator delete`
/// of the buffer, holder cleared). `pAnimationBuffer` itself keeps its value.
pub fn extra_saved_animation_delete_buffer(e: &mut Engine, this: Ptr<ExtraSavedAnimation>) {
    let s = store(e);
    let buffer = s.get(e, this, ExtraSavedAnimation::pAnimationBuffer);
    e.with_stack(4, |e, holder| {
        let built = e.call(HOLDER_CONSTRUCT, &args![holder, buffer]).ptr();
        typed_call!(FAMILY, 0x0043_5b20, fn_00435b20(e, built));
    });
}

// Typed from 00435c40
/// `ExtraFriendHits::ExtraFriendHits` (Xbox PDB): extra-data type 0x45, then
/// the constructor of the `Hits` array. Returns `this`. The
/// exception-unwinding frame is not translated.
pub fn extra_friend_hits_extra_friend_hits(
    e: &mut Engine,
    this: Ptr<ExtraFriendHits>,
) -> Ptr<ExtraFriendHits> {
    construct_base(e, this.cast(), TYPE_FRIEND_HITS, EXTRA_FRIEND_HITS_VTABLE);
    e.call(HITS_ARRAY_CONSTRUCT, &args![payload(this)]);
    this
}

// Typed from 00435cb0
/// `ExtraFriendHits::_scalar_deleting_destructor_` (Xbox PDB): the
/// destructor, then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_friend_hits_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraFriendHits>,
    flags: u32,
) -> Ptr<ExtraFriendHits> {
    typed_call!(FAMILY, 0x0043_5ce0, fn_00435ce0(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00435ce0
/// `ExtraFriendHits`' destructor body (the engine map has no name for it):
/// destroys the `Hits` array, then runs the base destructor (the vtable is
/// not stored again). The exception-unwinding frame is not translated.
pub fn fn_00435ce0(e: &mut Engine, this: Ptr<ExtraFriendHits>) {
    e.call(HITS_ARRAY_DESTRUCT, &args![payload(this)]);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

/// The count of a `BSSimpleArray`.
fn array_size(e: &mut Engine, array: Ptr<BSSimpleArray>) -> u32 {
    typed_call!(leaf::FAMILY, 0x0044_ddc0, leaf::word_at_8(e, array))
}

/// The address of slot `index` of a `BSSimpleArray`.
fn array_slot(e: &mut Engine, array: Ptr<BSSimpleArray>, index: u32) -> Ptr {
    e.call(SIMPLE_ARRAY_AT, &args![array, index]).ptr()
}

/// The age (`00435e00`) of the time stamp stored in the array slot at `slot`,
/// which the game first copies into a local.
fn stamp_age(e: &mut Engine, slot: Ptr) -> f32 {
    let stamp = e.mem.u32(slot.addr());
    e.with_stack(4, |e, holder| {
        e.mem.set_u32(holder.addr(), stamp);
        typed_call!(FAMILY, 0x0043_5e00, fn_00435e00(e, holder))
    })
}

/// The float value of a game setting, read through its accessor.
fn setting_value(e: &mut Engine, setting: u32) -> f32 {
    let slot: Ptr = e.call(SETTING_FLOAT_VALUE, &args![setting]).ptr();
    f32::from_bits(e.mem.u32(slot.addr()))
}

/// The tail of `AddHit`: a new time stamp (`00435de0` stores the clock
/// `00435dd0` returns) appended to `hits`.
fn record_hit(e: &mut Engine, hits: Ptr<BSSimpleArray>) {
    let now = typed_call!(FAMILY, 0x0043_5dd0, fn_00435dd0(e));
    e.with_stack(4, |e, stamp| {
        typed_call!(FAMILY, 0x0043_5de0, fn_00435de0(e, stamp, now));
        e.call(SIMPLE_ARRAY_APPEND, &args![hits, stamp]);
    });
}

// Typed from 00435d40
/// `ExtraFriendHits::AddHit` (Xbox PDB): drops the hits that are too old
/// (`RemoveOldHits`), then records a hit at the current time (`00435dd0`)
/// unless the age of the newest remaining hit is below the setting at
/// `011cd78c` (a hit is also recorded when there is none, or when the
/// comparison is unordered).
pub fn extra_friend_hits_add_hit(e: &mut Engine, this: Ptr<ExtraFriendHits>) {
    typed_call!(
        FAMILY,
        0x0043_5e40,
        extra_friend_hits_remove_old_hits(e, this)
    );
    let hits = this.at(ExtraFriendHits::Hits);
    let count = array_size(e, hits) as i32;
    if count > 0 {
        let slot = array_slot(e, hits, (count - 1) as u32);
        let age = stamp_age(e, slot);
        let setting = setting_value(e, HIT_SETTING_ADD);
        // FCOMP then `TEST AH,0x41`: setting <= age, or unordered.
        if setting as f64 > age as f64 {
            return;
        }
    }
    record_hit(e, hits);
}

// Typed from 00435dd0
/// The current time the hits are aged against (the engine map has no name for
/// it): returns the `float` at `011f1bf0` in ST0.
pub fn fn_00435dd0(e: &mut Engine) -> f32 {
    e.global::<f32>(HIT_CLOCK)
}

// Typed from 00435de0
/// `CombatTimeStamp`'s constructor (the engine map has no name for it):
/// stores `value` in the one-float time stamp at `this`. Returns `this`.
pub fn fn_00435de0(e: &mut Engine, this: Ptr, value: f32) -> Ptr {
    e.mem.set_u32(this.addr(), x87_quiet(value).to_bits());
    this
}

// Typed from 00435e00
/// The age of the time stamp at `this` (the engine map has no name for it):
/// the current time (`00435dd0`) minus the stamp, rounded to `float`, in ST0.
pub fn fn_00435e00(e: &mut Engine, this: Ptr) -> f32 {
    let now = typed_call!(FAMILY, 0x0043_5dd0, fn_00435dd0(e));
    let stamp = f32::from_bits(e.mem.u32(this.addr()));
    round_to_f32(f64::from(now) - f64::from(stamp))
}

// Typed from 00435e20
/// `ExtraFriendHits::GetHitCount` (Xbox PDB): drops the old hits, then
/// returns how many remain.
pub fn extra_friend_hits_get_hit_count(e: &mut Engine, this: Ptr<ExtraFriendHits>) -> u32 {
    typed_call!(
        FAMILY,
        0x0043_5e40,
        extra_friend_hits_remove_old_hits(e, this)
    );
    let hits = this.at(ExtraFriendHits::Hits);
    array_size(e, hits)
}

// Typed from 00435e40
/// `ExtraFriendHits::RemoveOldHits` (Xbox PDB): removes every hit whose age
/// (`00435e00`) is greater than the setting at `011cd580` (the comparison is
/// ordered: an unordered result keeps the hit). The count is read again on
/// every pass; after a removal the same index is looked at again.
pub fn extra_friend_hits_remove_old_hits(e: &mut Engine, this: Ptr<ExtraFriendHits>) {
    let hits = this.at(ExtraFriendHits::Hits);
    let mut index: u32 = 0;
    while index < array_size(e, hits) {
        let slot = array_slot(e, hits, index);
        let age = stamp_age(e, slot);
        let setting = setting_value(e, HIT_SETTING_REMOVE);
        if (setting as f64) < (age as f64) {
            e.call(SIMPLE_ARRAY_REMOVE_AT, &args![hits, index, 1u32]);
            index = index.wrapping_sub(1);
        }
        index = index.wrapping_add(1);
    }
}

// Typed from 00435ec0
/// `ExtraHeadingTarget`'s constructor (the engine map has no name for it):
/// extra-data type 0x46, storing `target`. Returns `this`.
pub fn fn_00435ec0(
    e: &mut Engine,
    this: Ptr<ExtraHeadingTarget>,
    target: Ptr,
) -> Ptr<ExtraHeadingTarget> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_HEADING_TARGET,
        EXTRA_HEADING_TARGET_VTABLE,
    );
    s.set(e, this, ExtraHeadingTarget::pheadingtarget, target);
    this
}

// Typed from 00435ef0
/// `ExtraHeadingTarget`'s destructor body (the engine map has no name for
/// it): resets the vtable, clears `pheadingtarget`, then runs the base
/// destructor.
pub fn fn_00435ef0(e: &mut Engine, this: Ptr<ExtraHeadingTarget>) {
    let s = store(e);
    e.mem.set_u32(this.addr(), EXTRA_HEADING_TARGET_VTABLE);
    s.set(e, this, ExtraHeadingTarget::pheadingtarget, Ptr::NULL);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 00435f20
/// `ExtraRefractionProperty`'s constructor (the engine map has no name for
/// it): extra-data type 0x48, storing `power`. Returns `this`.
pub fn fn_00435f20(
    e: &mut Engine,
    this: Ptr<ExtraRefractionProperty>,
    power: f32,
) -> Ptr<ExtraRefractionProperty> {
    construct_base(
        e,
        this.cast(),
        TYPE_REFRACTION_PROPERTY,
        EXTRA_REFRACTION_PROPERTY_VTABLE,
    );
    e.set(this, ExtraRefractionProperty::fRefractionPower, power);
    this
}

// Typed from 00435f50
/// `ExtraRefractionProperty`'s scalar deleting destructor (the engine map has
/// no name for it): the destructor (`00435f80`,
/// called by address), then `operator delete` when `flags & 1`. Returns
/// `this`.
pub fn fn_00435f50(
    e: &mut Engine,
    this: Ptr<ExtraRefractionProperty>,
    flags: u32,
) -> Ptr<ExtraRefractionProperty> {
    e.call(EXTRA_REFRACTION_PROPERTY_DESTRUCT, &args![this]);
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00435f80
/// `ExtraRefractionProperty`'s destructor body (the engine map has no name
/// for it): resets the vtable, then runs the base destructor.
pub fn fn_00435f80(e: &mut Engine, this: Ptr<ExtraRefractionProperty>) {
    e.mem.set_u32(this.addr(), EXTRA_REFRACTION_PROPERTY_VTABLE);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

/// Copies the three words of a `NiPoint3` (the game does it with word moves).
fn copy_point(e: &mut Engine, from: u32, to: u32) {
    for word in 0..3 {
        let value = e.mem.u32(from + 4 * word);
        e.mem.set_u32(to + 4 * word, value);
    }
}

// Typed from 00435fa0
/// `ExtraEditorRefMoveData`'s constructor (the engine map has no name for
/// it): extra-data type 0x4c, the three vectors constructed (`006815c0`),
/// then, when `reference` is not null, `realAngle` takes the 12 bytes
/// `00430830` gives for the reference, `realLocation` the vector its virtual
/// method at +0x1f4 returns, and `oldLocation` a copy of `realLocation`.
/// Returns `this`. The exception-unwinding frame is not translated.
pub fn fn_00435fa0(
    e: &mut Engine,
    this: Ptr<ExtraEditorRefMoveData>,
    reference: Ptr,
) -> Ptr<ExtraEditorRefMoveData> {
    construct_base(
        e,
        this.cast(),
        TYPE_EDITOR_REF_MOVE_DATA,
        EXTRA_EDITOR_REF_MOVE_DATA_VTABLE,
    );
    for offset in [0x0c, 0x18, 0x24] {
        typed_call!(
            leaf::FAMILY,
            0x0068_15c0,
            leaf::same_word(e, this.addr() + offset)
        );
    }
    if !reference.is_null() {
        let angle = typed_call!(FAMILY, 0x0043_0830, fn_00430830(e, reference));
        copy_point(e, angle.addr(), this.addr() + 0x0c);
        // The reference's virtual method at +0x1f4 returns the address of a
        // 12-byte vector (`ExtraTalkingActor`'s constructor passes the same
        // result to `TESObjectREFR::SetLocationOnReference`).
        let location = e.vcall(reference.addr(), 0x1f4, &args![]).u32();
        copy_point(e, location, this.addr() + 0x18);
        copy_point(e, this.addr() + 0x18, this.addr() + 0x24);
    }
    this
}

// Typed from 00436090
/// `ExtraHasNoRumors`' constructor (the engine map has no name for it):
/// extra-data type 0x4e, storing the flag byte. Returns `this`.
pub fn fn_00436090(
    e: &mut Engine,
    this: Ptr<ExtraHasNoRumors>,
    no_rumors: u8,
) -> Ptr<ExtraHasNoRumors> {
    construct_base(
        e,
        this.cast(),
        TYPE_HAS_NO_RUMORS,
        EXTRA_HAS_NO_RUMORS_VTABLE,
    );
    e.set(this, ExtraHasNoRumors::bNoRumors, no_rumors);
    this
}

/// The constructor of the classes that hold a `BSSoundHandle` at +0xc: base
/// constructor, the handle's default constructor (`0041a250`), then the
/// handle passed by value (three words) is copied in (`00418900`) and its
/// destructor (`00483710`) runs on the argument copy. The exception-unwinding
/// frame is not translated.
fn construct_sound_holder(
    e: &mut Engine,
    this: Ptr,
    extra_type: u32,
    vtable: u32,
    handle: [u32; 3],
) {
    construct_base(e, this, extra_type, vtable);
    e.call(SOUND_HANDLE_CONSTRUCT, &args![payload(this)]);
    e.with_stack(12, |e, copy| {
        for (index, word) in (0u32..).zip(handle.iter()) {
            e.mem.set_u32(copy.addr() + 4 * index, *word);
        }
        e.call(SOUND_HANDLE_ASSIGN, &args![payload(this), copy]);
        e.call(SOUND_HANDLE_DESTRUCT, &args![copy]);
    });
}

/// The destructor body of those classes: the vtable is stored again when
/// `vtable` is given, the handle's destructor (`00483710`) runs, then the base
/// destructor.
fn destroy_sound_holder(e: &mut Engine, this: Ptr, vtable: Option<u32>) {
    if let Some(vtable) = vtable {
        e.mem.set_u32(this.addr(), vtable);
    }
    e.call(SOUND_HANDLE_DESTRUCT, &args![payload(this)]);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 004360c0
/// `ExtraSound`'s constructor (the engine map has no name for it): extra-data
/// type 0x4f holding a copy of the `BSSoundHandle` passed by value (its three
/// words: sound id, assume-success byte, state). Returns `this`.
pub fn fn_004360c0(
    e: &mut Engine,
    this: Ptr<ExtraSound>,
    sound_id: u32,
    assume_success: u32,
    state: u32,
) -> Ptr<ExtraSound> {
    construct_sound_holder(
        e,
        this.cast(),
        TYPE_SOUND,
        EXTRA_SOUND_VTABLE,
        [sound_id, assume_success, state],
    );
    this
}

// Typed from 00436150
/// `ExtraSound`'s scalar deleting destructor (the engine map has no name for
/// it): the destructor, then `operator delete` when `flags & 1`. Returns
/// `this`.
pub fn fn_00436150(e: &mut Engine, this: Ptr<ExtraSound>, flags: u32) -> Ptr<ExtraSound> {
    typed_call!(FAMILY, 0x0043_6180, fn_00436180(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00436180
/// `ExtraSound`'s destructor body (the engine map has no name for it): resets
/// the vtable, destroys the handle, runs the base destructor.
pub fn fn_00436180(e: &mut Engine, this: Ptr<ExtraSound>) {
    destroy_sound_holder(e, this.cast(), Some(EXTRA_SOUND_VTABLE));
}

// Typed from 004361e0
/// `ExtraCreatureAwakeSound`'s constructor (the engine map has no name for
/// it): extra-data type 0x7d holding a copy of the `BSSoundHandle` passed by
/// value. Returns `this`.
pub fn fn_004361e0(
    e: &mut Engine,
    this: Ptr<ExtraCreatureAwakeSound>,
    sound_id: u32,
    assume_success: u32,
    state: u32,
) -> Ptr<ExtraCreatureAwakeSound> {
    construct_sound_holder(
        e,
        this.cast(),
        TYPE_CREATURE_AWAKE_SOUND,
        EXTRA_CREATURE_AWAKE_SOUND_VTABLE,
        [sound_id, assume_success, state],
    );
    this
}

// Typed from 00436270
/// `ExtraCreatureAwakeSound`'s scalar deleting destructor (the engine map has
/// no name for it): the destructor, then `operator delete` when `flags & 1`.
/// Returns `this`.
pub fn fn_00436270(
    e: &mut Engine,
    this: Ptr<ExtraCreatureAwakeSound>,
    flags: u32,
) -> Ptr<ExtraCreatureAwakeSound> {
    typed_call!(FAMILY, 0x0043_62a0, fn_004362a0(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 004362a0
/// `ExtraCreatureAwakeSound`'s destructor body (the engine map has no name
/// for it): resets the vtable, destroys the handle, runs the base destructor.
pub fn fn_004362a0(e: &mut Engine, this: Ptr<ExtraCreatureAwakeSound>) {
    destroy_sound_holder(e, this.cast(), Some(EXTRA_CREATURE_AWAKE_SOUND_VTABLE));
}

// Typed from 00436300
/// `ExtraCreatureMovementSound`'s constructor (the engine map has no name for
/// it): extra-data type 0x8a holding a copy of the `BSSoundHandle` passed by
/// value. Returns `this`.
pub fn fn_00436300(
    e: &mut Engine,
    this: Ptr<ExtraCreatureMovementSound>,
    sound_id: u32,
    assume_success: u32,
    state: u32,
) -> Ptr<ExtraCreatureMovementSound> {
    construct_sound_holder(
        e,
        this.cast(),
        TYPE_CREATURE_MOVEMENT_SOUND,
        EXTRA_CREATURE_MOVEMENT_SOUND_VTABLE,
        [sound_id, assume_success, state],
    );
    this
}

// Typed from 00436390
/// `ExtraCreatureMovementSound`'s scalar deleting destructor (the engine map
/// has no name for it): the destructor, then `operator delete` when
/// `flags & 1`. Returns `this`.
pub fn fn_00436390(
    e: &mut Engine,
    this: Ptr<ExtraCreatureMovementSound>,
    flags: u32,
) -> Ptr<ExtraCreatureMovementSound> {
    typed_call!(FAMILY, 0x0043_63c0, fn_004363c0(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 004363c0
/// `ExtraCreatureMovementSound`'s destructor body (the engine map has no name
/// for it): destroys the handle, runs the base destructor. Unlike its
/// siblings it does not store the vtable again.
pub fn fn_004363c0(e: &mut Engine, this: Ptr<ExtraCreatureMovementSound>) {
    destroy_sound_holder(e, this.cast(), None);
}

// Typed from 00436420
/// `ExtraWeaponIdleSound`'s constructor (the engine map has no name for it):
/// extra-data type 0x83 holding a copy of the `BSSoundHandle` passed by
/// value. Returns `this`.
pub fn fn_00436420(
    e: &mut Engine,
    this: Ptr<ExtraWeaponIdleSound>,
    sound_id: u32,
    assume_success: u32,
    state: u32,
) -> Ptr<ExtraWeaponIdleSound> {
    construct_sound_holder(
        e,
        this.cast(),
        TYPE_WEAPON_IDLE_SOUND,
        EXTRA_WEAPON_IDLE_SOUND_VTABLE,
        [sound_id, assume_success, state],
    );
    this
}

// Typed from 004364b0
/// `ExtraWeaponIdleSound`'s scalar deleting destructor (the engine map has no
/// name for it): the destructor, then `operator delete` when `flags & 1`.
/// Returns `this`.
pub fn fn_004364b0(
    e: &mut Engine,
    this: Ptr<ExtraWeaponIdleSound>,
    flags: u32,
) -> Ptr<ExtraWeaponIdleSound> {
    typed_call!(FAMILY, 0x0043_64e0, fn_004364e0(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 004364e0
/// `ExtraWeaponIdleSound`'s destructor body (the engine map has no name for
/// it): resets the vtable, destroys the handle, runs the base destructor.
pub fn fn_004364e0(e: &mut Engine, this: Ptr<ExtraWeaponIdleSound>) {
    destroy_sound_holder(e, this.cast(), Some(EXTRA_WEAPON_IDLE_SOUND_VTABLE));
}

// Typed from 00436540
/// `ExtraWeaponAttackSound`'s constructor (the engine map has no name for
/// it): extra-data type 0x86 holding a copy of the `BSSoundHandle` passed by
/// value. Returns `this`.
pub fn fn_00436540(
    e: &mut Engine,
    this: Ptr<ExtraWeaponAttackSound>,
    sound_id: u32,
    assume_success: u32,
    state: u32,
) -> Ptr<ExtraWeaponAttackSound> {
    construct_sound_holder(
        e,
        this.cast(),
        TYPE_WEAPON_ATTACK_SOUND,
        EXTRA_WEAPON_ATTACK_SOUND_VTABLE,
        [sound_id, assume_success, state],
    );
    this
}

// Typed from 004365d0
/// `ExtraWeaponAttackSound`'s scalar deleting destructor (the engine map has
/// no name for it): the destructor, then `operator delete` when `flags & 1`.
/// Returns `this`.
pub fn fn_004365d0(
    e: &mut Engine,
    this: Ptr<ExtraWeaponAttackSound>,
    flags: u32,
) -> Ptr<ExtraWeaponAttackSound> {
    typed_call!(FAMILY, 0x0043_6600, fn_00436600(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00436600
/// `ExtraWeaponAttackSound`'s destructor body (the engine map has no name for
/// it): resets the vtable, destroys the handle, runs the base destructor.
pub fn fn_00436600(e: &mut Engine, this: Ptr<ExtraWeaponAttackSound>) {
    destroy_sound_holder(e, this.cast(), Some(EXTRA_WEAPON_ATTACK_SOUND_VTABLE));
}

// Typed from 00436660
/// `ExtraActivateLoopSound`'s constructor (the engine map has no name for
/// it): extra-data type 0x87 holding a copy of the `BSSoundHandle` passed by
/// value. Returns `this`.
pub fn fn_00436660(
    e: &mut Engine,
    this: Ptr<ExtraActivateLoopSound>,
    sound_id: u32,
    assume_success: u32,
    state: u32,
) -> Ptr<ExtraActivateLoopSound> {
    construct_sound_holder(
        e,
        this.cast(),
        TYPE_ACTIVATE_LOOP_SOUND,
        EXTRA_ACTIVATE_LOOP_SOUND_VTABLE,
        [sound_id, assume_success, state],
    );
    this
}

// Typed from 004366f0
/// `ExtraActivateLoopSound`'s scalar deleting destructor (the engine map has
/// no name for it): the destructor, then `operator delete` when `flags & 1`.
/// Returns `this`.
pub fn fn_004366f0(
    e: &mut Engine,
    this: Ptr<ExtraActivateLoopSound>,
    flags: u32,
) -> Ptr<ExtraActivateLoopSound> {
    typed_call!(FAMILY, 0x0043_6720, fn_00436720(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00436720
/// `ExtraActivateLoopSound`'s destructor body (the engine map has no name for
/// it): resets the vtable, destroys the handle, runs the base destructor.
pub fn fn_00436720(e: &mut Engine, this: Ptr<ExtraActivateLoopSound>) {
    destroy_sound_holder(e, this.cast(), Some(EXTRA_ACTIVATE_LOOP_SOUND_VTABLE));
}

/// Allocates `size` bytes (`operator new`) and runs `constructor` on them when
/// the allocation succeeded; null otherwise.
fn new_object(e: &mut Engine, size: u32, constructor: u32) -> u32 {
    let memory = e.call(OPERATOR_NEW, &args![size]).u32();
    if memory == 0 {
        0
    } else {
        e.call(constructor, &args![memory]).u32()
    }
}

// Typed from 00436780
/// A function of this unit that the engine map has no name for; it ignores
/// its `this` (ECX) and `ExtraTalkingActor`'s constructor calls it. Builds a
/// `MobileObject` (0x88 bytes) for `source` and returns it (null when
/// `source` is null). The object is given `base_object` (`SetObjectReference`),
/// the persistence flag, the position (`source + 0x30`) and the rotation
/// (`source + 0x24`) of `source`, then `source` itself (the word at +0x6c),
/// and the byte at +0x81 is set to 1. Its cell is taken from the word at
/// `source + 0x40` when that is not null and `00425fd0` approves it (virtual
/// slot 0x228 of the new object); otherwise from the virtual method at slot
/// 0 of the word at `source + 0x18`, put in the new object's extra data
/// (`ExtraDataList::SetPersistentCell`), falling back to `source + 0x40` and
/// the virtual slot 0x228 again when that gives nothing. Finally, when the
/// player has a process (the word at +0x68), a process of the same kind (the
/// word at +0x28; 0 `HighProcess`, 1 to 3 other classes of 0x25c, 200 and
/// 0xb4 bytes, nothing otherwise) is built, stored in the new object, and
/// the object is added to the process lists. The exception-unwinding frame
/// is not translated.
pub fn fn_00436780(e: &mut Engine, _unused_this: Ptr, base_object: Ptr, source: Ptr) -> Ptr {
    if source.is_null() {
        return Ptr::NULL;
    }
    let object = new_object(e, 0x88, MOBILE_OBJECT_CONSTRUCT);
    e.call(SET_OBJECT_REFERENCE, &args![object, base_object]);
    let persists = e.call(GET_REF_PERSISTS, &args![source]).u32() & 0xff;
    e.call(SET_REF_PERSISTS, &args![object, persists]);
    let position = typed_call!(
        dispatch::FAMILY,
        0x0043_6aa0,
        dispatch::refs::position(e, source)
    );
    e.call(SET_LOCATION_ON_REFERENCE, &args![object, position]);
    let rotation = typed_call!(FAMILY, 0x0043_0830, fn_00430830(e, source)).addr();
    let words = [
        e.mem.u32(rotation),
        e.mem.u32(rotation + 4),
        e.mem.u32(rotation + 8),
    ];
    e.call(SET_ROTATION, &args![object, words[0], words[1], words[2]]);
    e.call(SET_SOURCE_REFERENCE, &args![object, source]);
    e.call(SET_FLAG_BYTE_81, &args![object, 1u32]);
    e.call(OBJECT_INIT, &args![object]);

    let mut cell_set = false;
    if e.call(GET_PARENT_CELL, &args![source]).u32() != 0 {
        let cell = e.call(GET_PARENT_CELL, &args![source]).u32();
        if e.call(CELL_CHECK, &args![cell]).bool() {
            let cell = e.call(GET_PARENT_CELL, &args![source]).u32();
            e.vcall(object, 0x228, &args![cell]);
            cell_set = true;
        }
    }
    if !cell_set {
        // `source + 0x18` is the `TESChildCell` base; its virtual slot 0
        // answers the persistent cell.
        let persistent = e.vcall(source.addr() + 0x18, 0, &args![]).u32();
        if persistent != 0 {
            let extra_list = e.call(GET_EXTRA_LIST, &args![object]).u32();
            e.call(SET_PERSISTENT_CELL, &args![extra_list, persistent]);
        } else {
            let cell = e.call(GET_PARENT_CELL, &args![source]).u32();
            e.vcall(object, 0x228, &args![cell]);
        }
    }

    let player = e.mem.u32(PLAYER);
    let player_process = typed_call!(leaf::FAMILY, 0x008d_8520, leaf::word_at_68(e, player));
    if player_process != 0 {
        let kind = typed_call!(
            leaf::FAMILY,
            0x0045_cd60,
            leaf::word_at_28(e, player_process)
        );
        let process = match kind {
            0 => new_object(e, 0x46c, HIGH_PROCESS_CONSTRUCT),
            1 => new_object(e, 0x25c, PROCESS_CONSTRUCT_25C),
            2 => new_object(e, 200, PROCESS_CONSTRUCT_0C8),
            3 => new_object(e, 0xb4, PROCESS_CONSTRUCT_0B4),
            _ => 0,
        };
        e.call(SET_PROCESS, &args![object, process]);
        let kind = typed_call!(leaf::FAMILY, 0x0045_cd60, leaf::word_at_28(e, process));
        e.call(
            PROCESS_LISTS_ADD_REFERENCE,
            &args![PROCESS_LISTS, object, kind, 0u32, 0u32, 0u32],
        );
    }
    Ptr::new(object)
}

// Typed from 00436ac0
/// `ExtraTalkingActor`'s default constructor (the engine map has no name for
/// it): extra-data type 0x55, no talking object. Returns `this`.
pub fn fn_00436ac0(e: &mut Engine, this: Ptr<ExtraTalkingActor>) -> Ptr<ExtraTalkingActor> {
    construct_null_payload(
        e,
        this.cast(),
        TYPE_TALKING_ACTOR,
        EXTRA_TALKING_ACTOR_VTABLE,
    );
    this
}

// Typed from 00436af0
/// `ExtraTalkingActor`'s scalar deleting destructor (the engine map has no
/// name for it): the destructor, then `operator delete` when `flags & 1`.
/// Returns `this`.
pub fn fn_00436af0(
    e: &mut Engine,
    this: Ptr<ExtraTalkingActor>,
    flags: u32,
) -> Ptr<ExtraTalkingActor> {
    typed_call!(FAMILY, 0x0043_6bb0, fn_00436bb0(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00436b20
/// `ExtraTalkingActor::ExtraTalkingActor` (Xbox PDB): extra-data type 0x55,
/// then `00436780` builds the talking object from `base_object` and `source`
/// (stored in `pTalkObject`), and the object is moved to the vector that
/// `source`'s virtual method at +0x1f4 returns
/// (`TESObjectREFR::SetLocationOnReference`). Returns `this`. The
/// exception-unwinding frame is not translated.
pub fn extra_talking_actor_extra_talking_actor(
    e: &mut Engine,
    this: Ptr<ExtraTalkingActor>,
    base_object: Ptr,
    source: Ptr,
) -> Ptr<ExtraTalkingActor> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_TALKING_ACTOR,
        EXTRA_TALKING_ACTOR_VTABLE,
    );
    let object = typed_call!(
        FAMILY,
        0x0043_6780,
        fn_00436780(e, this.cast(), base_object, source)
    );
    s.set(e, this, ExtraTalkingActor::pTalkObject, object);
    let location = e.vcall(source.addr(), 0x1f4, &args![]).u32();
    e.call(SET_LOCATION_ON_REFERENCE, &args![object, location]);
    this
}

// Typed from 00436bb0
/// `ExtraTalkingActor`'s destructor body (the engine map has no name for it):
/// resets the vtable, then runs the base destructor. The talking object is
/// not released.
pub fn fn_00436bb0(e: &mut Engine, this: Ptr<ExtraTalkingActor>) {
    e.mem.set_u32(this.addr(), EXTRA_TALKING_ACTOR_VTABLE);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

/// The start of the `Compare`s below: casts `other` to the class with the RTTI
/// descriptor `target_type`, then asks `BSExtraData::Compare`. `None` means the
/// answer is already true.
fn compare_start(e: &mut Engine, this: Ptr, other: Ptr, target_type: u32) -> Option<Ptr> {
    let cast: Ptr = e
        .call(
            DYNAMIC_CAST,
            &args![other, 0u32, BS_EXTRA_DATA_TYPE, target_type, 0u32],
        )
        .ptr();
    if cast.is_null() {
        return None;
    }
    if e.call(BS_EXTRA_DATA_COMPARE, &args![this, other]).bool() {
        return None;
    }
    Some(cast)
}

/// `Compare` of the classes whose payload is one `float` at +0xc.
fn compare_float_payload(e: &mut Engine, this: Ptr, other: Ptr, target_type: u32) -> bool {
    let Some(cast) = compare_start(e, this, other, target_type) else {
        return true;
    };
    // `FUCOMPP`: differing or unordered (NaN) floats count as different.
    f32::from_bits(e.mem.u32(this.addr() + 0xc)) != f32::from_bits(e.mem.u32(cast.addr() + 0xc))
}

// Typed from 00436bd0
/// `ExtraRadius::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraRadius`, when `BSExtraData::Compare` says so, or when `fRadius`
/// differs.
pub fn extra_radius_compare(e: &mut Engine, this: Ptr<ExtraRadius>, other: Ptr) -> bool {
    compare_float_payload(e, this.cast(), other, EXTRA_RADIUS_TYPE)
}

// Typed from 00436c40
/// `ExtraRadiation::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraRadiation`, when `BSExtraData::Compare` says so, or when
/// `fRadiation` differs.
pub fn extra_radiation_compare(e: &mut Engine, this: Ptr<ExtraRadiation>, other: Ptr) -> bool {
    compare_float_payload(e, this.cast(), other, EXTRA_RADIATION_TYPE)
}

// Typed from 00436cb0
/// `ExtraFactionChanges`' constructor (the engine map has no name for it):
/// extra-data type 0x5e, then a new 8-byte list (`0096a2d0`) stored in
/// `pFactionChanges`. Returns `this`. The exception-unwinding frame is not
/// translated.
pub fn fn_00436cb0(e: &mut Engine, this: Ptr<ExtraFactionChanges>) -> Ptr<ExtraFactionChanges> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_FACTION_CHANGES,
        EXTRA_FACTION_CHANGES_VTABLE,
    );
    let list = new_object(e, 8, SIMPLE_LIST_CONSTRUCT);
    s.set(
        e,
        this,
        ExtraFactionChanges::pFactionChanges,
        Ptr::new(list),
    );
    this
}

// Typed from 00436d50
/// `ExtraFactionChanges::_scalar_deleting_destructor_` (Xbox PDB): the
/// destructor, then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_faction_changes_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraFactionChanges>,
    flags: u32,
) -> Ptr<ExtraFactionChanges> {
    typed_call!(FAMILY, 0x0043_6d80, fn_00436d80(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00436d80
/// `ExtraFactionChanges`' destructor body (the engine map has no name for
/// it): resets the vtable, runs the list's node release (`00470470`, also
/// when the list pointer is null) and, when the list exists, its scalar
/// deleting destructor with the flag 1; then the base destructor. The
/// entries themselves are not freed. The exception-unwinding frame is not
/// translated.
pub fn fn_00436d80(e: &mut Engine, this: Ptr<ExtraFactionChanges>) {
    let s = store(e);
    e.mem.set_u32(this.addr(), EXTRA_FACTION_CHANGES_VTABLE);
    let list = s.get(e, this, ExtraFactionChanges::pFactionChanges);
    e.call(SIMPLE_LIST_CLEAR, &args![list]);
    if !list.is_null() {
        e.call(SIMPLE_LIST_DELETE, &args![list, 1u32]);
    }
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

/// The item of a list node (`006815c0` gives the address of the node's item
/// word).
fn node_item(e: &mut Engine, node: u32) -> u32 {
    let slot = typed_call!(leaf::FAMILY, 0x0068_15c0, leaf::same_word(e, node));
    e.mem.u32(slot)
}

/// The next node of a list node (`00726070`).
fn node_next(e: &mut Engine, node: u32) -> u32 {
    typed_call!(leaf::FAMILY, 0x0072_6070, leaf::word_at_4(e, node))
}

/// The entry for `faction` in the list that begins at `node`, walking while
/// the nodes exist and hold an item.
fn find_faction_entry(e: &mut Engine, mut node: u32, faction: Ptr) -> Option<Ptr<FactionRank>> {
    while node != 0 && node_item(e, node) != 0 {
        let entry = Ptr::<FactionRank>::new(node_item(e, node));
        if e.get(entry, FactionRank::pFaction) == faction {
            return Some(entry);
        }
        node = node_next(e, node);
    }
    None
}

/// A new 8-byte entry `{faction, rank}`, added to the list at `list` through
/// `005ae3d0` (which takes the address of a word holding the entry pointer).
fn add_faction_entry(e: &mut Engine, list: Ptr, faction: Ptr, rank: u8) {
    let entry = Ptr::<FactionRank>::new(e.call(OPERATOR_NEW, &args![8u32]).u32());
    e.set(entry, FactionRank::cRank, rank);
    e.set(entry, FactionRank::pFaction, faction);
    e.with_stack(4, |e, slot| {
        e.mem.set_u32(slot.addr(), entry.addr());
        e.call(SIMPLE_LIST_ADD_HEAD, &args![list, slot]);
    });
}

// Typed from 00436e10
/// A method of `ExtraFactionChanges` that the engine map has no name for:
/// sets the rank of `faction` to `0xff` (-1) when the list has an entry for
/// it; otherwise creates the list if there is none and adds a new entry
/// `{faction, 0xff}`. The exception-unwinding frame is not translated.
pub fn fn_00436e10(e: &mut Engine, this: Ptr<ExtraFactionChanges>, faction: Ptr) {
    let s = store(e);
    let head = s.get(e, this, ExtraFactionChanges::pFactionChanges);
    if let Some(entry) = find_faction_entry(e, head.addr(), faction) {
        e.set(entry, FactionRank::cRank, 0xff);
        return;
    }
    if s.get(e, this, ExtraFactionChanges::pFactionChanges)
        .is_null()
    {
        let list = new_object(e, 8, SIMPLE_LIST_CONSTRUCT);
        s.set(
            e,
            this,
            ExtraFactionChanges::pFactionChanges,
            Ptr::new(list),
        );
    }
    let list = s.get(e, this, ExtraFactionChanges::pFactionChanges);
    add_faction_entry(e, list, faction, 0xff);
}

// Typed from 00436f20
/// A method of `ExtraFactionChanges` that the engine map has no name for:
/// sets the rank of `faction` when the list has an entry for it; nothing
/// otherwise.
pub fn fn_00436f20(e: &mut Engine, this: Ptr<ExtraFactionChanges>, faction: Ptr, rank: u8) {
    let s = store(e);
    let head = s.get(e, this, ExtraFactionChanges::pFactionChanges);
    if let Some(entry) = find_faction_entry(e, head.addr(), faction) {
        e.set(entry, FactionRank::cRank, rank);
    }
}

// Typed from 00436f80
/// A method of `ExtraFactionChanges` that the engine map has no name for:
/// sets the rank of `faction` when the list has an entry for it, otherwise
/// adds a new entry `{faction, rank}` to the list (which must exist).
pub fn fn_00436f80(e: &mut Engine, this: Ptr<ExtraFactionChanges>, faction: Ptr, rank: u8) {
    let s = store(e);
    let head = s.get(e, this, ExtraFactionChanges::pFactionChanges);
    if let Some(entry) = find_faction_entry(e, head.addr(), faction) {
        e.set(entry, FactionRank::cRank, rank);
        return;
    }
    let list = s.get(e, this, ExtraFactionChanges::pFactionChanges);
    add_faction_entry(e, list, faction, rank);
}

// Typed from 00437010
/// `ExtraFactionChanges::GetIsInFaction` (Xbox PDB): true when some entry is
/// for `faction` and has a rank above -1. An entry for it with rank -1 does
/// not stop the walk.
pub fn extra_faction_changes_get_is_in_faction(
    e: &mut Engine,
    this: Ptr<ExtraFactionChanges>,
    faction: Ptr,
) -> bool {
    let s = store(e);
    let mut node = s.get(e, this, ExtraFactionChanges::pFactionChanges).addr();
    while node != 0 && node_item(e, node) != 0 {
        let entry = Ptr::<FactionRank>::new(node_item(e, node));
        if e.get(entry, FactionRank::pFaction) == faction
            && (e.get(entry, FactionRank::cRank) as i8) > -1
        {
            return true;
        }
        node = node_next(e, node);
    }
    false
}

// Typed from 00437080
/// `ExtraFactionChanges::GetIsExpelled` (Xbox PDB): true when some entry is
/// for `faction` and has rank -1.
pub fn extra_faction_changes_get_is_expelled(
    e: &mut Engine,
    this: Ptr<ExtraFactionChanges>,
    faction: Ptr,
) -> bool {
    let s = store(e);
    let mut node = s.get(e, this, ExtraFactionChanges::pFactionChanges).addr();
    while node != 0 && node_item(e, node) != 0 {
        let entry = Ptr::<FactionRank>::new(node_item(e, node));
        if e.get(entry, FactionRank::pFaction) == faction
            && (e.get(entry, FactionRank::cRank) as i8) == -1
        {
            return true;
        }
        node = node_next(e, node);
    }
    false
}

// Typed from 004370f0
/// A method of `ExtraFactionChanges` that the engine map has no name for.
/// Builds a map of 37 buckets on the stack (`004383e0`, destroyed at the end
/// by `004388a0`); every entry of the list gets rank `0xff` and its faction
/// is recorded in the map (`005594e0`, value 1). Then, when `other` is not
/// null, every entry of the list `005d8a70` gives for `other + 0x30` whose
/// faction is not in the map, or is there with a zero value (`0057c850`), is
/// added to the list as `{faction, 0xff}`. The exception-unwinding frame is
/// not translated.
pub fn fn_004370f0(e: &mut Engine, this: Ptr<ExtraFactionChanges>, other: Ptr) {
    let s = store(e);
    e.with_stack(0x10, |e, map| {
        e.call(FACTION_MAP_CONSTRUCT, &args![map, 0x25u32]);
        let mut node = s.get(e, this, ExtraFactionChanges::pFactionChanges).addr();
        while node != 0 && node_item(e, node) != 0 {
            let entry = Ptr::<FactionRank>::new(node_item(e, node));
            node = node_next(e, node);
            e.set(entry, FactionRank::cRank, 0xff);
            let faction = e.get(entry, FactionRank::pFaction);
            e.call(FACTION_MAP_SET_AT, &args![map, faction, 1u32]);
        }
        if !other.is_null() {
            let mut node = e.call(FACTION_LIST_OF, &args![other.addr() + 0x30]).u32();
            while node != 0 && node_item(e, node) != 0 {
                let entry = Ptr::<FactionRank>::new(node_item(e, node));
                node = node_next(e, node);
                let faction = e.get(entry, FactionRank::pFaction);
                let recorded = e.with_stack(4, |e, value| {
                    let found = e
                        .call(FACTION_MAP_LOOKUP, &args![map, faction, value])
                        .bool();
                    found && e.mem.u8(value.addr()) != 0
                });
                if !recorded {
                    let list = s.get(e, this, ExtraFactionChanges::pFactionChanges);
                    add_faction_entry(e, list, faction, 0xff);
                }
            }
        }
        e.call(FACTION_MAP_DESTRUCT, &args![map]);
    });
}

// Typed from 00437240
/// A function of this unit that the engine map has no name for: copies a
/// `RadioData` (0x10 bytes: radius, range type, static percentage, position
/// reference) from `source` into `this`, nothing when `source` is null. The
/// two floats pass through x87 registers, which quiets a signalling NaN; the
/// two integers are copied as they are.
pub fn fn_00437240(e: &mut Engine, this: Ptr<RadioData>, source: Ptr<RadioData>) {
    if source.is_null() {
        return;
    }
    // 00437253: +0 radius, FLD float / FSTP float (quiets a signalling NaN).
    let radius = e.mem.u32(source.addr());
    e.mem.set_u32(this.addr(), x87_quiet_bits(radius));
    // 0043725d: +4 range type, MOV.
    let range = e.mem.u32(source.addr() + 4);
    e.mem.set_u32(this.addr() + 4, range);
    // 00437269: +8 static percentage, FLD float / FSTP float.
    let percentage = e.mem.u32(source.addr() + 8);
    e.mem.set_u32(this.addr() + 8, x87_quiet_bits(percentage));
    // 00437275: +0xc position reference, MOV.
    let reference = e.mem.u32(source.addr() + 0xc);
    e.mem.set_u32(this.addr() + 0xc, reference);
}

// Typed from 00437290
/// `ExtraRadioData::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraRadioData`, when `BSExtraData::Compare` says so, or when the 0x10
/// bytes of `Data` differ.
pub fn extra_radio_data_compare(e: &mut Engine, this: Ptr<ExtraRadioData>, other: Ptr) -> bool {
    let Some(cast) = compare_start(e, this.cast(), other, EXTRA_RADIO_DATA_TYPE) else {
        return true;
    };
    e.call(MEMCMP, &args![payload(this), payload(cast), 0x10u32])
        .u32()
        != 0
}

/// Extra-data type bytes of the classes of the last part of this range.
const TYPE_SAY_TOPIC_INFO_ONCE_A_DAY: u32 = 0x73;
const TYPE_SAY_TO_TOPIC_INFO: u32 = 0x75;
const TYPE_WATER_ZONE_MAP: u32 = 0x7e;
const TYPE_IGNORED_BY_SANDBOX: u32 = 0x80;
const TYPE_PATROL_REF_IN_USE_DATA: u32 = 0x88;
const TYPE_FOLLOWER_SWIM_BREADCRUMBS: u32 = 0x8b;

/// Their vtables.
const EXTRA_SAY_TOPIC_INFO_ONCE_A_DAY_VTABLE: u32 = 0x0101_5f3c;
const EXTRA_SAY_TO_TOPIC_INFO_VTABLE: u32 = 0x0101_5f48;
const EXTRA_WATER_ZONE_MAP_VTABLE: u32 = 0x0101_5f54;
const EXTRA_IGNORED_BY_SANDBOX_VTABLE: u32 = 0x0101_5f64;
const EXTRA_PATROL_REF_IN_USE_DATA_VTABLE: u32 = 0x0101_5f70;
const EXTRA_FOLLOWER_SWIM_BREADCRUMBS_VTABLE: u32 = 0x0101_5f7c;

/// `RTTI Type Descriptor`s of the classes whose `Compare` casts `other`.
const EXTRA_COMBAT_STYLE_TYPE: u32 = 0x0118_495c;
const EXTRA_PRIMITIVE_TYPE: u32 = 0x0118_42b0;
const EXTRA_AMMO_TYPE: u32 = 0x0118_48c0;

/// The list's head removal (`this` is the list; the first item's node is
/// taken out and the next node moves into the head).
const SIMPLE_LIST_REMOVE_HEAD: u32 = 0x0063_f7b0;
/// The list's replacement of the head item (`this` is the list; the
/// argument is the address of a word holding the new item).
const SIMPLE_LIST_SET_HEAD: u32 = 0x0072_6c60;
/// The insertion `ExtraFollowerSwimBreadcrumbs` uses for its crumb list
/// (`this` is the list; the argument is the address of the word holding the
/// crumb pointer).
const CRUMB_LIST_ADD: u32 = 0x0090_5820;
/// `ExtraFollowerSwimBreadcrumb`'s scalar deleting destructor (`this`, the
/// flags).
const CRUMB_DELETE: u32 = 0x007b_3fa0;
/// The constructor of the map `ExtraWaterZoneMap` embeds at +0xc (`this` is
/// the map, the argument the bucket count).
const WATER_ZONE_MAP_CONSTRUCT: u32 = 0x0043_83b0;
/// The destructor of that map (`this` is the map).
const WATER_ZONE_MAP_DESTRUCT: u32 = 0x0043_8720;
/// The map's lookup (`this` is the map, the arguments the key and the
/// address of the word that receives the count).
const ZONE_MAP_LOOKUP: u32 = 0x0085_3130;
/// The map's `SetAt` (`this` is the map, the arguments the key and the value).
const ZONE_MAP_SET_AT: u32 = 0x0084_4700;
/// The map's `RemoveAt` (`this` is the map, the argument the key).
const ZONE_MAP_REMOVE_AT: u32 = 0x0040_5430;
/// The map's first position (`this` is the map; 0 when it is empty).
const ZONE_MAP_FIRST: u32 = 0x004b_9ba0;
/// The map's iteration step (`this` is the map; the arguments are the
/// addresses of the position, of the key and of the value, which receive
/// the entry; the position advances).
const ZONE_MAP_NEXT: u32 = 0x006b_7f20;
/// The float `fn_00437850` starts its search for the highest zone from (the
/// lowest `f32`, held in the exe's data).
const LOWEST_FLOAT: u32 = 0x0101_5f5c;
/// The reference for a handle (cdecl, one stack argument; null when the
/// handle has none).
const REFERENCE_FOR_HANDLE: u32 = 0x0048_39c0;
/// A method of the reference that returns a small integer (the code
/// compared with 3 and with 5 below; `this` only).
const REFERENCE_KIND: u32 = 0x004f_8960;
/// `MobileObject::GetCurrentProcessType` (Xbox PDB).
const GET_CURRENT_PROCESS_TYPE: u32 = 0x0093_1850;
/// The global that holds a fixed string's empty value.
const EMPTY_FIXED_STRING: u32 = 0x0109_b220;
/// Creates the fixed string for a C string (cdecl, one stack argument).
const FIXED_STRING_CREATE: u32 = 0x00a5_b690;
/// Releases a fixed string's text (cdecl, one stack argument: the address
/// of the text's header, 8 bytes before the text).
const FIXED_STRING_RELEASE: u32 = 0x0040_19a0;
/// The string assignment `ExtraSecuritronFace` uses (`this` is the string,
/// the argument a C string).
const STRING_ASSIGN: u32 = 0x0043_8390;
/// The two methods of `ExtraSecuritronFace` that copy a string out (`this`
/// is the extra data, the argument the address of the destination string).
const GET_FACE_STRING_FIRST: u32 = 0x0042_8070;
const GET_FACE_STRING_SECOND: u32 = 0x0042_80b0;
/// A string's length (`this` is the string).
const STRING_LENGTH: u32 = 0x0040_48e0;
/// A string's destructor (`this` is the string).
const STRING_DESTRUCT: u32 = 0x0040_37d0;
/// `sprintf`-style formatting (cdecl: buffer, size, format, then the values).
const FORMAT_STRING: u32 = 0x0040_6d00;
/// `SwapPlatformLanguageTexturePath(source, destination, size)` (Xbox PDB;
/// cdecl).
const SWAP_PLATFORM_LANGUAGE_TEXTURE_PATH: u32 = 0x004b_7240;
/// The global holding the `TES` pointer.
const TES: u32 = 0x011d_ea10;
/// `TES::CreateTextureImage` (Xbox PDB; `this` is the TES object, then the
/// path, the address of the smart pointer that receives the image and two
/// zero words).
const CREATE_TEXTURE_IMAGE: u32 = 0x0045_68c0;
/// `NiAVObject::GetProperty` (Xbox PDB; `this`, the property type).
const GET_PROPERTY: u32 = 0x00a5_9d30;
/// The format of the texture path `ApplyFace` builds.
const SECURITRON_TEXTURE_FORMAT: u32 = 0x0101_5f84;
/// The name of the object `ApplyFace` looks for.
const SECURITRON_SCREEN_NAME: u32 = 0x0101_5fac;
/// The three-word exe global `ExtraFollowerSwimBreadcrumbs`' constructor
/// copies into `LeaderLocation`.
const DEFAULT_LEADER_LOCATION: u32 = 0x011f_426c;

// Typed from 00437300
/// `ExtraCombatStyle::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraCombatStyle`, when `BSExtraData::Compare` says so, or when
/// `pCombatStyle` differs.
pub fn extra_combat_style_compare(e: &mut Engine, this: Ptr<ExtraCombatStyle>, other: Ptr) -> bool {
    let Some(cast) = compare_start(e, this.cast(), other, EXTRA_COMBAT_STYLE_TYPE) else {
        return true;
    };
    e.mem.u32(this.addr() + 0xc) != e.mem.u32(cast.addr() + 0xc)
}

// Typed from 00437370
/// `ExtraPrimitive::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraPrimitive` or when `pPrimitive` differs (the base compare is not
/// asked).
pub fn extra_primitive_compare(e: &mut Engine, this: Ptr<ExtraPrimitive>, other: Ptr) -> bool {
    compare_payload_word(e, this, other, EXTRA_PRIMITIVE_TYPE)
}

// Typed from 004373c0
/// `ExtraAmmo::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraAmmo`, when `BSExtraData::Compare` says so, or when `iCount` (+0x10)
/// or `pAmmo` (+0xc) differs.
pub fn extra_ammo_compare(e: &mut Engine, this: Ptr<ExtraAmmo>, other: Ptr) -> bool {
    let Some(cast) = compare_start(e, this.cast(), other, EXTRA_AMMO_TYPE) else {
        return true;
    };
    if e.mem.u32(this.addr() + 0x10) != e.mem.u32(cast.addr() + 0x10) {
        return true;
    }
    e.mem.u32(this.addr() + 0xc) != e.mem.u32(cast.addr() + 0xc)
}

// Typed from 00437440
/// `ExtraSayTopicInfoOnceADay`'s constructor (the engine map has no name for
/// it): extra-data type 0x73, then a new 8-byte list (`0096a2d0`) stored in
/// `pListofSaidOnceTopicInfos`. Returns `this`. The exception-unwinding frame
/// is not translated.
pub fn fn_00437440(
    e: &mut Engine,
    this: Ptr<ExtraSayTopicInfoOnceADay>,
) -> Ptr<ExtraSayTopicInfoOnceADay> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_SAY_TOPIC_INFO_ONCE_A_DAY,
        EXTRA_SAY_TOPIC_INFO_ONCE_A_DAY_VTABLE,
    );
    let list = new_object(e, 8, SIMPLE_LIST_CONSTRUCT);
    s.set(
        e,
        this,
        ExtraSayTopicInfoOnceADay::pListofSaidOnceTopicInfos,
        Ptr::new(list),
    );
    this
}

// Typed from 004374e0
/// `ExtraSayTopicInfoOnceADay::_scalar_deleting_destructor_` (Xbox PDB): the
/// destructor, then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_say_topic_info_once_a_day_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraSayTopicInfoOnceADay>,
    flags: u32,
) -> Ptr<ExtraSayTopicInfoOnceADay> {
    typed_call!(FAMILY, 0x0043_75c0, fn_004375c0(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00437510
/// A constructor of `ExtraSayTopicInfoOnceADay` that the engine map has no
/// name for: as `00437440`, then it adds `item` (the one stack word, passed
/// by address) to the new list (`005ae3d0`, with no null check). Returns
/// `this`. The exception-unwinding frame is not translated.
pub fn fn_00437510(
    e: &mut Engine,
    this: Ptr<ExtraSayTopicInfoOnceADay>,
    item: u32,
) -> Ptr<ExtraSayTopicInfoOnceADay> {
    let s = store(e);
    typed_call!(FAMILY, 0x0043_7440, fn_00437440(e, this));
    let list = s.get(
        e,
        this,
        ExtraSayTopicInfoOnceADay::pListofSaidOnceTopicInfos,
    );
    e.with_stack(4, |e, slot| {
        e.mem.set_u32(slot.addr(), item);
        e.call(SIMPLE_LIST_ADD_HEAD, &args![list, slot]);
    });
    this
}

// Typed from 004375c0
/// `ExtraSayTopicInfoOnceADay`'s destructor body (the engine map has no name
/// for it): resets the vtable, then, while the list has a first item, takes
/// the item, removes the head node (`0063f7b0`) and frees the item
/// (`operator delete`); then runs the list's scalar deleting destructor with
/// the flag 1 when the list exists; then the base destructor. The
/// exception-unwinding frame is not translated.
pub fn fn_004375c0(e: &mut Engine, this: Ptr<ExtraSayTopicInfoOnceADay>) {
    let s = store(e);
    e.mem
        .set_u32(this.addr(), EXTRA_SAY_TOPIC_INFO_ONCE_A_DAY_VTABLE);
    let list = e
        .get(this, ExtraSayTopicInfoOnceADay::pListofSaidOnceTopicInfos)
        .addr();
    while list != 0 && node_item(e, list) != 0 {
        let item = node_item(e, list);
        e.call(SIMPLE_LIST_REMOVE_HEAD, &args![list]);
        e.call(OPERATOR_DELETE, &args![item]);
    }
    let list = s.get(
        e,
        this,
        ExtraSayTopicInfoOnceADay::pListofSaidOnceTopicInfos,
    );
    if !list.is_null() {
        e.call(SIMPLE_LIST_DELETE, &args![list, 1u32]);
    }
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 00437690
/// `ExtraSayToTopicInfo`'s default constructor (the engine map has no name
/// for it): extra-data type 0x75, `pInfo`, `pTopic` and `nResponseID` zero,
/// `bVoiceDone` false. Returns `this`.
pub fn fn_00437690(e: &mut Engine, this: Ptr<ExtraSayToTopicInfo>) -> Ptr<ExtraSayToTopicInfo> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_SAY_TO_TOPIC_INFO,
        EXTRA_SAY_TO_TOPIC_INFO_VTABLE,
    );
    s.set(e, this, ExtraSayToTopicInfo::pInfo, Ptr::new(0));
    s.set(e, this, ExtraSayToTopicInfo::pTopic, Ptr::new(0));
    s.set(e, this, ExtraSayToTopicInfo::nResponseID, 0);
    e.set(this, ExtraSayToTopicInfo::bVoiceDone, 0);
    this
}

// Typed from 004376e0
/// `ExtraSayToTopicInfo`'s constructor with the info (the engine map has no
/// name for it): extra-data type 0x75, `pInfo` = `info`, the other fields
/// zero (`nResponseID` is not written by this one). Returns `this`.
pub fn fn_004376e0(
    e: &mut Engine,
    this: Ptr<ExtraSayToTopicInfo>,
    info: u32,
) -> Ptr<ExtraSayToTopicInfo> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_SAY_TO_TOPIC_INFO,
        EXTRA_SAY_TO_TOPIC_INFO_VTABLE,
    );
    s.set(e, this, ExtraSayToTopicInfo::pInfo, Ptr::new(info));
    s.set(e, this, ExtraSayToTopicInfo::pTopic, Ptr::new(0));
    s.set(e, this, ExtraSayToTopicInfo::nResponseID, 0);
    e.set(this, ExtraSayToTopicInfo::bVoiceDone, 0);
    this
}

// Typed from 00437730
/// A setter of `ExtraSayToTopicInfo` that the engine map has no name for:
/// stores `topic` in `pTopic` (+0x10).
pub fn fn_00437730(e: &mut Engine, this: Ptr<ExtraSayToTopicInfo>, topic: u32) {
    let s = store(e);
    s.set(e, this, ExtraSayToTopicInfo::pTopic, Ptr::new(topic));
}

// Typed from 00437750
/// `ExtraWaterZoneMap`'s constructor (the engine map has no name for it):
/// extra-data type 0x7e, the embedded map built with 0x25 buckets (`004383b0`)
/// and `pHighestWaterZone` null. Returns `this`. The exception-unwinding
/// frame is not translated.
pub fn fn_00437750(e: &mut Engine, this: Ptr<ExtraWaterZoneMap>) -> Ptr<ExtraWaterZoneMap> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_WATER_ZONE_MAP,
        EXTRA_WATER_ZONE_MAP_VTABLE,
    );
    e.call(WATER_ZONE_MAP_CONSTRUCT, &args![this.addr() + 0xc, 0x25u32]);
    s.set(e, this, ExtraWaterZoneMap::pHighestWaterZone, Ptr::new(0));
    this
}

// Typed from 004377c0
/// `ExtraWaterZoneMap::_scalar_deleting_destructor_` (Xbox PDB): the
/// destructor, then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_water_zone_map_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraWaterZoneMap>,
    flags: u32,
) -> Ptr<ExtraWaterZoneMap> {
    typed_call!(FAMILY, 0x0043_77f0, fn_004377f0(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 004377f0
/// `ExtraWaterZoneMap`'s destructor body (the engine map names this body
/// after a folded library class): destroys the embedded map (`00438720`),
/// then runs the base destructor. The vtable is not stored again. The
/// exception-unwinding frame is not translated.
pub fn fn_004377f0(e: &mut Engine, this: Ptr<ExtraWaterZoneMap>) {
    e.call(WATER_ZONE_MAP_DESTRUCT, &args![this.addr() + 0xc]);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 00437850
/// A method of `ExtraWaterZoneMap` that the engine map has no name for; it
/// keeps the count of the zone in the map (`WaterZoneMap`, at +0xc) and
/// `pHighestWaterZone`, the zone with the greatest float at +0x18.
///
/// With `add` set: looks the zone up (the result is not used), sets its count
/// to 1 (`SetAt`) and, when its float is above the lowest float, makes it the
/// highest. With `add` clear: looks the count up; at 1 or less the zone is
/// removed (`RemoveAt`) and the highest zone is searched again over the
/// remaining entries (an entry replaces the best one when its float is
/// strictly above it, so the earlier entry wins a tie; `pHighestWaterZone` is
/// left alone when no entry is above the lowest float); above 1 the count is
/// set to the count minus 1.
pub fn fn_00437850(e: &mut Engine, this: Ptr<ExtraWaterZoneMap>, zone: u32, add: bool) {
    let s = store(e);
    let map = this.addr() + 0xc;
    let lowest = e.global::<f32>(LOWEST_FLOAT);
    if add {
        e.with_stack(4, |e, count| {
            e.mem.set_u32(count.addr(), 0);
            e.call(ZONE_MAP_LOOKUP, &args![map, zone, count]);
        });
        e.call(ZONE_MAP_SET_AT, &args![map, zone, 1u32]);
        if lowest < f32::from_bits(e.mem.u32(zone + 0x18)) {
            s.set(
                e,
                this,
                ExtraWaterZoneMap::pHighestWaterZone,
                Ptr::new(zone),
            );
        }
        return;
    }
    let count = e.with_stack(4, |e, slot| {
        e.mem.set_u32(slot.addr(), 0);
        e.call(ZONE_MAP_LOOKUP, &args![map, zone, slot]);
        e.mem.u32(slot.addr()) as i32
    });
    if count > 1 {
        e.call(ZONE_MAP_SET_AT, &args![map, zone, (count - 1) as u32]);
        return;
    }
    e.call(ZONE_MAP_REMOVE_AT, &args![map, zone]);
    let first = e.call(ZONE_MAP_FIRST, &args![map]).u32();
    e.with_stack(12, |e, slots| {
        let position = slots.addr();
        let key = slots.addr() + 4;
        let value = slots.addr() + 8;
        e.mem.set_u32(position, first);
        let mut best = lowest;
        while e.mem.u32(position) != 0 {
            e.call(ZONE_MAP_NEXT, &args![map, position, key, value]);
            let candidate = e.mem.u32(key);
            let height = f32::from_bits(e.mem.u32(candidate + 0x18));
            if best < height {
                best = height;
                s.set(
                    e,
                    this,
                    ExtraWaterZoneMap::pHighestWaterZone,
                    Ptr::new(candidate),
                );
            }
        }
    });
}

// Typed from 00437970
/// `ExtraIgnoredBySandbox`'s constructor (the engine map has no name for it):
/// extra-data type 0x80, no payload. Returns `this`.
pub fn fn_00437970(e: &mut Engine, this: Ptr) -> Ptr {
    construct_base(
        e,
        this,
        TYPE_IGNORED_BY_SANDBOX,
        EXTRA_IGNORED_BY_SANDBOX_VTABLE,
    );
    this
}

// Typed from 004379a0
/// `ExtraPatrolRefInUseData`'s constructor (the engine map has no name for
/// it): extra-data type 0x88; `User` is the handle of `user` (`0084e3a0`), or
/// 0 for a null `user`. Returns `this`. The exception-unwinding frame is not
/// translated.
pub fn fn_004379a0(
    e: &mut Engine,
    this: Ptr<ExtraPatrolRefInUseData>,
    user: Ptr,
) -> Ptr<ExtraPatrolRefInUseData> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_PATROL_REF_IN_USE_DATA,
        EXTRA_PATROL_REF_IN_USE_DATA_VTABLE,
    );
    let handle = if user.is_null() {
        0
    } else {
        typed_call!(leaf::FAMILY, 0x0084_e3a0, leaf::word_at_c(e, user))
    };
    s.set(e, this, ExtraPatrolRefInUseData::User, handle);
    this
}

// Typed from 00437a30
/// `ExtraPatrolRefInUseData::Compare` (Xbox PDB): true when `User` differs.
/// It neither casts `other` nor asks the base compare.
pub fn extra_patrol_ref_in_use_data_compare(
    e: &mut Engine,
    this: Ptr<ExtraPatrolRefInUseData>,
    other: Ptr<ExtraPatrolRefInUseData>,
) -> bool {
    let s = store(e);
    s.get(e, this, ExtraPatrolRefInUseData::User) != s.get(e, other, ExtraPatrolRefInUseData::User)
}

// Typed from 00437a60
/// `ExtraPatrolRefInUseData`'s scalar deleting destructor (the engine map has
/// no name for it): the destructor, then `operator delete` when `flags & 1`.
/// Returns `this`.
pub fn fn_00437a60(
    e: &mut Engine,
    this: Ptr<ExtraPatrolRefInUseData>,
    flags: u32,
) -> Ptr<ExtraPatrolRefInUseData> {
    typed_call!(FAMILY, 0x0043_7a90, fn_00437a90(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00437a90
/// `ExtraPatrolRefInUseData`'s destructor body (the engine map has no name
/// for it): resets the vtable, then runs the base destructor.
pub fn fn_00437a90(e: &mut Engine, this: Ptr<ExtraPatrolRefInUseData>) {
    e.mem
        .set_u32(this.addr(), EXTRA_PATROL_REF_IN_USE_DATA_VTABLE);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 00437ab0
/// `ExtraPatrolRefInUseData::GetCanBeUsedBy` (Xbox PDB): true when `user` is
/// the reference that already holds the patrol reference (its handle,
/// `0084e3a0`, equals `User`). Otherwise looks up the reference for `User`
/// (`004839c0`): when it is gone, or virtual slot 0x22c (called with 0) is
/// true, or its flags word has `0x820` (`00437b90`), the hold is released
/// (`User` = 0) and the answer is true. A reference for which virtual slot
/// 0x100 is false answers false. Otherwise the hold is released (answer true)
/// when its kind code (`004f8960`) is 5 or 3, or when it has a current process
/// type (`00931850`, non-zero) and the byte at +0x87 is clear; else false.
pub fn extra_patrol_ref_in_use_data_get_can_be_used_by(
    e: &mut Engine,
    this: Ptr<ExtraPatrolRefInUseData>,
    user: Ptr,
) -> bool {
    let s = store(e);
    if !user.is_null() {
        let handle = typed_call!(leaf::FAMILY, 0x0084_e3a0, leaf::word_at_c(e, user));
        if handle == s.get(e, this, ExtraPatrolRefInUseData::User) {
            return true;
        }
    }
    let handle = s.get(e, this, ExtraPatrolRefInUseData::User);
    let reference = e.call(REFERENCE_FOR_HANDLE, &args![handle]).ptr();
    if reference.is_null() {
        s.set(e, this, ExtraPatrolRefInUseData::User, 0);
        return true;
    }
    if e.vcall(reference.addr(), 0x22c, &args![0u32]).bool()
        || typed_call!(FAMILY, 0x0043_7b90, fn_00437b90(e, reference))
    {
        s.set(e, this, ExtraPatrolRefInUseData::User, 0);
        return true;
    }
    if !e.vcall(reference.addr(), 0x100, &args![]).bool() {
        return false;
    }
    let releases = typed_call!(FAMILY, 0x0043_7bf0, fn_00437bf0(e, reference))
        || typed_call!(FAMILY, 0x0043_7bd0, fn_00437bd0(e, reference))
        || (e.call(GET_CURRENT_PROCESS_TYPE, &args![reference]).u32() != 0
            && typed_call!(FAMILY, 0x0043_7bb0, fn_00437bb0(e, reference)) == 0);
    if releases {
        s.set(e, this, ExtraPatrolRefInUseData::User, 0);
        return true;
    }
    false
}

// Typed from 00437b90
/// A predicate on a form that the engine map has no name for: true when the
/// flags word at +0x8 has bit `0x20` or `0x800` set.
pub fn fn_00437b90(e: &mut Engine, this: Ptr) -> bool {
    e.mem.u32(this.addr() + 8) & 0x820 != 0
}

// Typed from 00437bb0
/// An accessor that the engine map has no name for: the byte at +0x87 of a
/// reference.
pub fn fn_00437bb0(e: &mut Engine, this: Ptr) -> u8 {
    e.mem.u8(this.addr() + 0x87)
}

// Typed from 00437bd0
/// A predicate on a reference that the engine map has no name for: true when
/// its kind code (`004f8960`) is 3.
pub fn fn_00437bd0(e: &mut Engine, this: Ptr) -> bool {
    e.call(REFERENCE_KIND, &args![this]).u32() == 3
}

// Typed from 00437bf0
/// A predicate on a reference that the engine map has no name for: true when
/// its kind code (`004f8960`) is 5.
pub fn fn_00437bf0(e: &mut Engine, this: Ptr) -> bool {
    e.call(REFERENCE_KIND, &args![this]).u32() == 5
}

// Typed from 00437c10
/// The constructor of an `ExtraFollowerSwimBreadcrumb` (0x24 bytes) that the
/// engine map has no name for: runs the folded `NiPoint3` constructor on the
/// two vectors at +0 and +0x10 (it writes nothing). Returns `this`.
pub fn fn_00437c10(e: &mut Engine, this: Ptr) -> Ptr {
    typed_call!(leaf::FAMILY, 0x0068_15c0, leaf::same_word(e, this));
    typed_call!(
        leaf::FAMILY,
        0x0068_15c0,
        leaf::same_word(e, this.addr() + 0x10)
    );
    this
}

// Typed from 00437c40
/// `ExtraFollowerSwimBreadcrumbs::ExtraFollowerSwimBreadcrumbs` (Xbox PDB):
/// extra-data type 0x8b, `eLeaderState` 0, `LeaderLocation` copied from the
/// three-word exe global at `011f426c`, `LeaderNavMeshID` 0 and the crumb
/// list at +0x20 built in place (`0096a2d0`). Returns `this`. The
/// exception-unwinding frame is not translated.
pub fn extra_follower_swim_breadcrumbs_extra_follower_swim_breadcrumbs(
    e: &mut Engine,
    this: Ptr<ExtraFollowerSwimBreadcrumbs>,
) -> Ptr<ExtraFollowerSwimBreadcrumbs> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_FOLLOWER_SWIM_BREADCRUMBS,
        EXTRA_FOLLOWER_SWIM_BREADCRUMBS_VTABLE,
    );
    s.set(e, this, ExtraFollowerSwimBreadcrumbs::eLeaderState, 0);
    for offset in [0u32, 4, 8] {
        let word = e.mem.u32(DEFAULT_LEADER_LOCATION + offset);
        e.mem.set_u32(this.addr() + 0x10 + offset, word);
    }
    s.set(e, this, ExtraFollowerSwimBreadcrumbs::LeaderNavMeshID, 0);
    e.call(SIMPLE_LIST_CONSTRUCT, &args![this.addr() + 0x20]);
    this
}

// Typed from 00437ce0
/// `ExtraFollowerSwimBreadcrumbs::_scalar_deleting_destructor_` (Xbox PDB):
/// the destructor, then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_follower_swim_breadcrumbs_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraFollowerSwimBreadcrumbs>,
    flags: u32,
) -> Ptr<ExtraFollowerSwimBreadcrumbs> {
    typed_call!(FAMILY, 0x0043_7d10, fn_00437d10(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00437d10
/// `ExtraFollowerSwimBreadcrumbs`' destructor body (the engine map has no
/// name for it): resets the vtable; walks every node of the crumb list at
/// +0x20, running the scalar deleting destructor (`007b3fa0`, flag 1) of
/// each non-null crumb and clearing the node's item; then the list's
/// destructor (`0046ffb0`) and the base destructor. The exception-unwinding
/// frame is not translated.
pub fn fn_00437d10(e: &mut Engine, this: Ptr<ExtraFollowerSwimBreadcrumbs>) {
    e.mem
        .set_u32(this.addr(), EXTRA_FOLLOWER_SWIM_BREADCRUMBS_VTABLE);
    let list = this.addr() + 0x20;
    let mut node = list;
    while node != 0 {
        let slot = typed_call!(leaf::FAMILY, 0x0068_15c0, leaf::same_word(e, node));
        let crumb = e.mem.u32(slot);
        if crumb != 0 {
            e.call(CRUMB_DELETE, &args![crumb, 1u32]);
        }
        let slot = typed_call!(leaf::FAMILY, 0x0068_15c0, leaf::same_word(e, node));
        e.mem.set_u32(slot, 0);
        node = node_next(e, node);
    }
    e.call(SIMPLE_LIST_DESTRUCT, &args![list]);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 00437dd0
/// An accessor of `ExtraFollowerSwimBreadcrumbs` that the engine map has no
/// name for: the first crumb of the list at +0x20.
pub fn fn_00437dd0(e: &mut Engine, this: Ptr<ExtraFollowerSwimBreadcrumbs>) -> u32 {
    node_item(e, this.addr() + 0x20)
}

// Typed from 00437df0
/// A method of `ExtraFollowerSwimBreadcrumbs` that the engine map has no
/// name for: destroys the first crumb (`007b3fa0`, flag 1, when not null),
/// sets the head item of the list at +0x20 to null (`00726c60`) and removes
/// the head node (`0063f7b0`).
pub fn fn_00437df0(e: &mut Engine, this: Ptr<ExtraFollowerSwimBreadcrumbs>) {
    let list = this.addr() + 0x20;
    let crumb = node_item(e, list);
    if crumb != 0 {
        e.call(CRUMB_DELETE, &args![crumb, 1u32]);
    }
    e.with_stack(4, |e, zero| {
        e.mem.set_u32(zero.addr(), 0);
        e.call(SIMPLE_LIST_SET_HEAD, &args![list, zero]);
    });
    e.call(SIMPLE_LIST_REMOVE_HEAD, &args![list]);
}

// Typed from 00437e50
/// A method of `ExtraFollowerSwimBreadcrumbs` that the engine map has no
/// name for (it records the leader's move): `state` is the new
/// `eLeaderState`, and `x`, `y`, `z` and `nav_mesh` the new leader location
/// and navmesh. Does nothing and returns false when `state` is 0. When
/// `state` differs from the current one and the current one is not 0, a new
/// crumb (0x24 bytes, `00437c10`) is appended to the list (`00905820`) first:
/// it holds the old location (+0), the old navmesh (+0xc), the new location
/// (+0x10), the new navmesh (+0x1c) and whether `state` is 2 (+0x20). In
/// every non-zero case the location, navmesh and state are then stored.
/// Returns whether a crumb was appended. The exception-unwinding frame is not
/// translated.
pub fn fn_00437e50(
    e: &mut Engine,
    this: Ptr<ExtraFollowerSwimBreadcrumbs>,
    state: u32,
    x: u32,
    y: u32,
    z: u32,
    nav_mesh: u32,
) -> bool {
    let s = store(e);
    if state == 0 {
        return false;
    }
    let mut appended = false;
    let current = s.get(e, this, ExtraFollowerSwimBreadcrumbs::eLeaderState);
    if state != current && current != 0 {
        let memory = e.call(OPERATOR_NEW, &args![0x24u32]).u32();
        let crumb = if memory == 0 {
            0
        } else {
            typed_call!(FAMILY, 0x0043_7c10, fn_00437c10(e, Ptr::new(memory))).addr()
        };
        for offset in [0u32, 4, 8] {
            let word = e.mem.u32(this.addr() + 0x10 + offset);
            e.mem.set_u32(crumb + offset, word);
        }
        let old_nav_mesh = s.get(e, this, ExtraFollowerSwimBreadcrumbs::LeaderNavMeshID);
        e.mem.set_u32(crumb + 0xc, old_nav_mesh);
        e.mem.set_u32(crumb + 0x10, x);
        e.mem.set_u32(crumb + 0x14, y);
        e.mem.set_u32(crumb + 0x18, z);
        e.mem.set_u32(crumb + 0x1c, nav_mesh);
        e.mem.set_u8(crumb + 0x20, (state == 2) as u8);
        e.with_stack(4, |e, slot| {
            e.mem.set_u32(slot.addr(), crumb);
            e.call(CRUMB_LIST_ADD, &args![this.addr() + 0x20, slot]);
        });
        appended = true;
    }
    e.mem.set_u32(this.addr() + 0x10, x);
    e.mem.set_u32(this.addr() + 0x14, y);
    e.mem.set_u32(this.addr() + 0x18, z);
    s.set(
        e,
        this,
        ExtraFollowerSwimBreadcrumbs::LeaderNavMeshID,
        nav_mesh,
    );
    s.set(e, this, ExtraFollowerSwimBreadcrumbs::eLeaderState, state);
    appended
}

// Typed from 00437f90
/// `ExtraSecuritronFace::ApplyFace` (Xbox PDB): `this` holds two strings
/// (personality at +0xc, mood at +0x14) and `root` is the Securitron's scene
/// object. Nothing happens for a null `root`. Otherwise the object named
/// "Screen01:0" is asked of `root` (virtual slot 0x9c, called with a fixed
/// string); when there is none, nothing more happens. When both strings
/// (`00428070`, `004280b0`) are non-empty, the texture path
/// `Textures\creatures\Securitron\%s_%s.dds` is formatted from them, passed
/// through `SwapPlatformLanguageTexturePath` and loaded
/// (`TES::CreateTextureImage`) into a smart pointer on the stack. Then the
/// screen's property of the type `00438220` gives (3) is asked
/// (`NiAVObject::GetProperty`) and, when it exists, receives the texture
/// (`00438230`), null when none was loaded. The exception-unwinding frame and
/// the stack-cookie check are not translated.
pub fn extra_securitron_face_apply_face(e: &mut Engine, this: Ptr<ExtraSecuritronFace>, root: Ptr) {
    if root.is_null() {
        return;
    }
    e.with_stack(0x228, |e, frame| {
        let base = frame.addr();
        let name = Ptr::new(base);
        let first = base + 4;
        let second = base + 0xc;
        let path = base + 0x14;
        let texture = base + 0x120;
        let localized = base + 0x124;
        let name_text = typed_call!(
            FAMILY,
            0x0043_8170,
            fixed_string_construct(e, name, SECURITRON_SCREEN_NAME)
        );
        let screen: Ptr = e.vcall(root.addr(), 0x9c, &args![name_text]).ptr();
        typed_call!(FAMILY, 0x0043_81b0, fixed_string_destruct(e, name));
        if screen.is_null() {
            return;
        }
        e.call(NI_POINTER_CONSTRUCT, &args![texture, 0u32]);
        e.call(GET_FACE_STRING_FIRST, &args![this, first]);
        e.call(GET_FACE_STRING_SECOND, &args![this, second]);
        if e.call(STRING_LENGTH, &args![first]).u32() != 0
            && e.call(STRING_LENGTH, &args![second]).u32() != 0
        {
            let second_text = typed_call!(leaf::FAMILY, 0x0055_9450, leaf::word_at_0(e, second));
            let first_text = typed_call!(leaf::FAMILY, 0x0055_9450, leaf::word_at_0(e, first));
            e.call(
                FORMAT_STRING,
                &args![
                    path,
                    0x104u32,
                    SECURITRON_TEXTURE_FORMAT,
                    first_text,
                    second_text
                ],
            );
            e.call(
                SWAP_PLATFORM_LANGUAGE_TEXTURE_PATH,
                &args![path, localized, 0x104u32],
            );
            let tes = e.global::<u32>(TES);
            e.call(
                CREATE_TEXTURE_IMAGE,
                &args![tes, localized, texture, 0u32, 0u32],
            );
        }
        let property_type = typed_call!(FAMILY, 0x0043_8220, fn_00438220(e));
        let property: Ptr = e.call(GET_PROPERTY, &args![screen, property_type]).ptr();
        if !property.is_null() {
            let image = typed_call!(leaf::FAMILY, 0x0055_9450, leaf::word_at_0(e, texture));
            typed_call!(FAMILY, 0x0043_8230, fn_00438230(e, property, image));
        }
        e.call(STRING_DESTRUCT, &args![second]);
        e.call(STRING_DESTRUCT, &args![first]);
        e.call(NI_POINTER_DESTRUCT, &args![texture]);
    });
}

// Typed from 00438170
/// The constructor of a fixed string (`NiFixedString`) that the engine map
/// has no name for: stores in `this` the empty value (the word at `0109b220`)
/// for a null `text`, otherwise the fixed string made for `text`
/// (`00a5b690`). Returns `this`.
pub fn fixed_string_construct(e: &mut Engine, this: Ptr, text: u32) -> Ptr {
    let value = if text == 0 {
        e.mem.u32(EMPTY_FIXED_STRING)
    } else {
        e.call(FIXED_STRING_CREATE, &args![text]).u32()
    };
    e.mem.set_u32(this.addr(), value);
    this
}

// Typed from 004381b0
/// The destructor of a fixed string that the engine map has no name for:
/// releases the text (`004381d0`).
pub fn fixed_string_destruct(e: &mut Engine, this: Ptr) {
    typed_call!(FAMILY, 0x0043_81d0, fixed_string_release(e, this));
}

// Typed from 004381d0
/// Releases the text of the fixed string at `slot` (cdecl, one stack word;
/// the engine map has no name for it): nothing for the empty value, otherwise
/// `004019a0` with the header address of the text (`00438210`).
pub fn fixed_string_release(e: &mut Engine, slot: Ptr) {
    if e.mem.u32(slot.addr()) == e.mem.u32(EMPTY_FIXED_STRING) {
        return;
    }
    let header = typed_call!(FAMILY, 0x0043_8210, fn_00438210(e, slot));
    e.call(FIXED_STRING_RELEASE, &args![header]);
}

// Typed from 00438210
/// The address of the header of a fixed string's text that the engine map
/// has no name for (cdecl, one stack word): the text pointer stored at `slot`
/// minus 8.
pub fn fn_00438210(e: &mut Engine, slot: Ptr) -> u32 {
    e.mem.u32(slot.addr()).wrapping_sub(8)
}

// Typed from 00438220
/// A function with no arguments that the engine map has no name for: returns
/// 3 (the property type `ApplyFace` asks for).
pub fn fn_00438220(_e: &mut Engine) -> u32 {
    3
}

// Typed from 00438230
/// A method of the property `ApplyFace` finds that the engine map has no
/// name for: assigns `texture` to the smart pointer at +0x60
/// (`NiPointer::operator=`, `0066b0d0`) and clears the word at +0x38.
pub fn fn_00438230(e: &mut Engine, this: Ptr, texture: u32) {
    e.call(NI_POINTER_ASSIGN, &args![this.addr() + 0x60, texture]);
    e.mem.set_u32(this.addr() + 0x38, 0);
}

// Typed from 00438260
/// `ExtraSecuritronFace::SetPersonality` (Xbox PDB): assigns `text` to
/// `msPersonality` (+0xc, `00438390`).
pub fn extra_securitron_face_set_personality(
    e: &mut Engine,
    this: Ptr<ExtraSecuritronFace>,
    text: u32,
) {
    e.call(STRING_ASSIGN, &args![this.addr() + 0xc, text]);
}

// Typed from 00438280
/// `ExtraSecuritronFace::SetMood` (Xbox PDB): assigns `text` to `msMood`
/// (+0x14, `00438390`).
pub fn extra_securitron_face_set_mood(e: &mut Engine, this: Ptr<ExtraSecuritronFace>, text: u32) {
    e.call(STRING_ASSIGN, &args![this.addr() + 0x14, text]);
}

/// This file's functions, by exe address.
#[rustfmt::skip]
pub fn entries() -> Vec<(u32, AbiFn)> {
    vec![
        entry!(0x0043_53f0,
            extra_x_target_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_5440,
            fn_00435440(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_5470,
            fn_00435470(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_54a0, fn_004354a0(Ptr<_>)),
        entry!(0x0043_54c0,
            extra_emittance_source_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_5510,
            fn_00435510(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_5540,
            extra_multi_bound_ref_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_5590,
            fn_00435590(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_55c0,
            extra_multi_bound_data_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            )
                -> Ptr<_>
        ),
        entry!(0x0043_55f0, fn_004355f0(Ptr<_>)),
        entry!(0x0043_5630,
            extra_multi_bound_data_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_5690,
            fn_00435690(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_5710,
            extra_multi_bound_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            ) -> Ptr<_>
        ),
        entry!(0x0043_5740, fn_00435740(Ptr<_>)),
        entry!(0x0043_57a0,
            fn_004357a0(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_5820,
            fn_00435820(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_58a0, fn_004358a0(Ptr<_>) -> Ptr<_>),
        entry!(0x0043_5920,
            fn_00435920(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_5950,
            fn_00435950(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_59c0,
            extra_dropped_item_list_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            )
                -> Ptr<_>
        ),
        entry!(0x0043_59f0, fn_004359f0(Ptr<_>)),
        entry!(0x0043_5a60,
            fn_00435a60(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_5a90,
            extra_saved_animation_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            )
                -> Ptr<_>
        ),
        entry!(0x0043_5b20, fn_00435b20(Ptr<_>)),
        entry!(0x0043_5b50,
            fn_00435b50(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_5b80,
            extra_saved_havok_data_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            )
                -> Ptr<_>
        ),
        entry!(0x0043_5c10,
            extra_saved_animation_delete_buffer(Ptr<_>)
        ),
        entry!(0x0043_5c40,
            extra_friend_hits_extra_friend_hits(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_5cb0,
            extra_friend_hits_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            ) -> Ptr<_>
        ),
        entry!(0x0043_5ce0, fn_00435ce0(Ptr<_>)),
        entry!(0x0043_5d40, extra_friend_hits_add_hit(Ptr<_>)),
        entry!(0x0043_5dd0, fn_00435dd0() -> f32),
        entry!(0x0043_5de0, fn_00435de0(Ptr<_>, f32) -> Ptr<_>),
        entry!(0x0043_5e00, fn_00435e00(Ptr<_>) -> f32),
        entry!(0x0043_5e20,
            extra_friend_hits_get_hit_count(Ptr<_>) -> u32
        ),
        entry!(0x0043_5e40,
            extra_friend_hits_remove_old_hits(Ptr<_>)
        ),
        entry!(0x0043_5ec0,
            fn_00435ec0(Ptr<_>, Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_5ef0, fn_00435ef0(Ptr<_>)),
        entry!(0x0043_5f20,
            fn_00435f20(Ptr<_>, f32) -> Ptr<_>
        ),
        entry!(0x0043_5f50,
            fn_00435f50(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_5f80, fn_00435f80(Ptr<_>)),
        entry!(0x0043_5fa0,
            fn_00435fa0(Ptr<_>, Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_6090,
            fn_00436090(Ptr<_>, u8) -> Ptr<_>
        ),
        entry!(0x0043_60c0,
            fn_004360c0(Ptr<_>, u32, u32, u32) -> Ptr<_>
        ),
        entry!(0x0043_6150,
            fn_00436150(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_6180, fn_00436180(Ptr<_>)),
        entry!(0x0043_61e0,
            fn_004361e0(
                Ptr<_>,
                u32,
                u32,
                u32,
            ) -> Ptr<_>
        ),
        entry!(0x0043_6270,
            fn_00436270(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_62a0, fn_004362a0(Ptr<_>)),
        entry!(0x0043_6300,
            fn_00436300(
                Ptr<_>,
                u32,
                u32,
                u32,
            ) -> Ptr<_>
        ),
        entry!(0x0043_6390,
            fn_00436390(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_63c0, fn_004363c0(Ptr<_>)),
        entry!(0x0043_6420,
            fn_00436420(Ptr<_>, u32, u32, u32) -> Ptr<_>
        ),
        entry!(0x0043_64b0,
            fn_004364b0(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_64e0, fn_004364e0(Ptr<_>)),
        entry!(0x0043_6540,
            fn_00436540(Ptr<_>, u32, u32, u32) -> Ptr<_>
        ),
        entry!(0x0043_65d0,
            fn_004365d0(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_6600, fn_00436600(Ptr<_>)),
        entry!(0x0043_6660,
            fn_00436660(Ptr<_>, u32, u32, u32) -> Ptr<_>
        ),
        entry!(0x0043_66f0,
            fn_004366f0(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_6720, fn_00436720(Ptr<_>)),
        entry!(0x0043_6780, fn_00436780(Ptr<_>, Ptr<_>, Ptr<_>) -> Ptr<_>),
        entry!(0x0043_6ac0,
            fn_00436ac0(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_6af0,
            fn_00436af0(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_6b20,
            extra_talking_actor_extra_talking_actor(
                Ptr<_>,
                Ptr<_>,
                Ptr<_>,
            ) -> Ptr<_>
        ),
        entry!(0x0043_6bb0, fn_00436bb0(Ptr<_>)),
        entry!(0x0043_6bd0,
            extra_radius_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_6c40,
            extra_radiation_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_6cb0,
            fn_00436cb0(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_6d50,
            extra_faction_changes_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            )
                -> Ptr<_>
        ),
        entry!(0x0043_6d80, fn_00436d80(Ptr<_>)),
        entry!(0x0043_6e10, fn_00436e10(Ptr<_>, Ptr<_>)),
        entry!(0x0043_6f20, fn_00436f20(Ptr<_>, Ptr<_>, u8)),
        entry!(0x0043_6f80, fn_00436f80(Ptr<_>, Ptr<_>, u8)),
        entry!(0x0043_7010,
            extra_faction_changes_get_is_in_faction(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_7080,
            extra_faction_changes_get_is_expelled(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_70f0, fn_004370f0(Ptr<_>, Ptr<_>)),
        entry!(0x0043_7240, fn_00437240(Ptr<_>, Ptr<_>)),
        entry!(0x0043_7290,
            extra_radio_data_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_7300,
            extra_combat_style_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_7370,
            extra_primitive_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_73c0, extra_ammo_compare(Ptr<_>, Ptr<_>) -> bool),
        entry!(0x0043_7440,
            fn_00437440(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_74e0,
            extra_say_topic_info_once_a_day_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            ) -> Ptr<_>
        ),
        entry!(0x0043_7510,
            fn_00437510(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_75c0, fn_004375c0(Ptr<_>)),
        entry!(0x0043_7690,
            fn_00437690(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_76e0,
            fn_004376e0(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_7730, fn_00437730(Ptr<_>, u32)),
        entry!(0x0043_7750,
            fn_00437750(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_77c0,
            extra_water_zone_map_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            ) -> Ptr<_>
        ),
        entry!(0x0043_77f0, fn_004377f0(Ptr<_>)),
        entry!(0x0043_7850, fn_00437850(Ptr<_>, u32, bool)),
        entry!(0x0043_7970, fn_00437970(Ptr<_>) -> Ptr<_>),
        entry!(0x0043_79a0,
            fn_004379a0(Ptr<_>, Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_7a30,
            extra_patrol_ref_in_use_data_compare(
                Ptr<_>,
                Ptr<_>,
            ) -> bool
        ),
        entry!(0x0043_7a60,
            fn_00437a60(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_7a90, fn_00437a90(Ptr<_>)),
        entry!(0x0043_7ab0,
            extra_patrol_ref_in_use_data_get_can_be_used_by(
                Ptr<_>,
                Ptr<_>,
            ) -> bool
        ),
        entry!(0x0043_7b90, fn_00437b90(Ptr<_>) -> bool),
        entry!(0x0043_7bb0, fn_00437bb0(Ptr<_>) -> u8),
        entry!(0x0043_7bd0, fn_00437bd0(Ptr<_>) -> bool),
        entry!(0x0043_7bf0, fn_00437bf0(Ptr<_>) -> bool),
        entry!(0x0043_7c10, fn_00437c10(Ptr<_>) -> Ptr<_>),
        entry!(0x0043_7c40,
            extra_follower_swim_breadcrumbs_extra_follower_swim_breadcrumbs(
                Ptr<_>,
            ) -> Ptr<_>
        ),
        entry!(0x0043_7ce0,
            extra_follower_swim_breadcrumbs_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            ) -> Ptr<_>
        ),
        entry!(0x0043_7d10, fn_00437d10(Ptr<_>)),
        entry!(0x0043_7dd0,
            fn_00437dd0(Ptr<_>) -> u32
        ),
        entry!(0x0043_7df0, fn_00437df0(Ptr<_>)),
        entry!(0x0043_7e50,
            fn_00437e50(Ptr<_>, u32, u32, u32, u32, u32) -> bool
        ),
        entry!(0x0043_7f90,
            extra_securitron_face_apply_face(Ptr<_>, Ptr<_>)
        ),
        entry!(0x0043_8170, fixed_string_construct(Ptr<_>, u32) -> Ptr<_>),
        entry!(0x0043_81b0, fixed_string_destruct(Ptr<_>)),
        entry!(0x0043_81d0, fixed_string_release(Ptr<_>)),
        entry!(0x0043_8210, fn_00438210(Ptr<_>) -> u32),
        entry!(0x0043_8220, fn_00438220() -> u32),
        entry!(0x0043_8230, fn_00438230(Ptr<_>, u32)),
        entry!(0x0043_8260,
            extra_securitron_face_set_personality(Ptr<_>, u32)
        ),
        entry!(0x0043_8280,
            extra_securitron_face_set_mood(Ptr<_>, u32)
        ),
    ]
}
