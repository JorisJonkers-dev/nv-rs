// SPDX-License-Identifier: GPL-3.0-only
//! The extra data node classes' functions of the extradataobjects unit (nv-rs#952, step 1).

use super::layouts::*;
use super::{low_half, store, FAMILY};
use engine_core::prelude::*;
use engine_core::types::*;
use engine_core::AbiFn;

/// `operator new(size)` (cdecl, one stack argument).
pub(crate) const OPERATOR_NEW: u32 = 0x0040_1000;
/// `operator delete(pointer)` (cdecl, one stack argument).
pub(crate) const OPERATOR_DELETE: u32 = 0x0040_1030;
/// `BSExtraData::BSExtraData(type)` (the base constructor; `this`, then the
/// extra-data type byte).
pub(crate) const BS_EXTRA_DATA_CONSTRUCT: u32 = 0x0040_ec80;
/// `BSExtraData::~BSExtraData` (the base destructor body).
pub(crate) const BS_EXTRA_DATA_DESTRUCT: u32 = 0x0040_ecb0;
/// `BSExtraData::Compare` (Xbox PDB; `this`, other): true when `other` is
/// null or `004f1540` gives a different answer for the two.
pub(crate) const BS_EXTRA_DATA_COMPARE: u32 = 0x0040_f700;
/// `BSSimpleArray<T,1024>::operator[]` (`this` is the array, the index is
/// the argument): the address of the element slot.
pub(crate) const SIMPLE_ARRAY_AT: u32 = 0x006a_7ad0;
/// `BSSimpleArray<BGSBodyPart_P_1024>::AddUninitialized` (Xbox PDB name of
/// the folded body): appends the pointer stored at the address given and
/// returns the index of the new slot.
pub(crate) const SIMPLE_ARRAY_ADD: u32 = 0x007c_b2e0;
/// `BSSimpleArray<TESBoundObject_P_1024>::CompareBuffer<1024>` (Xbox PDB):
/// whether the `this` array holds the same elements as the one given.
pub(crate) const SIMPLE_ARRAY_COMPARE_BUFFER: u32 = 0x0043_87b0;
/// `BSSimpleArray<T,1024>::Find` of the folded body (`this` is the array;
/// the arguments are the address of the value, the first index to look at
/// and a comparison function; the index found, or -1).
pub(crate) const SIMPLE_ARRAY_FIND: u32 = 0x0071_9b20;
/// The comparison function `00719b20` is given by `ExtraGuardedRefData`.
pub(crate) const GUARD_COMPARE: u32 = 0x009a_3830;
/// The array's clear (`this` is the array, the argument says whether to free
/// the buffer); `CompareBuffer`'s caller uses it to drop an array that
/// duplicates an earlier one.
pub(crate) const SIMPLE_ARRAY_CLEAR: u32 = 0x0084_54f0;
/// `__RTDynamicCast(object, vfDelta, sourceType, targetType, isReference)`.
pub(crate) const DYNAMIC_CAST: u32 = 0x00ec_43fb;
/// `memcmp(a, b, size)`.
pub(crate) const MEMCMP: u32 = 0x00ec_4835;
/// `_ftol2_sse` (`00ec62c0`): truncates the float in ST0 to an integer in
/// EAX. The uniform form has no ST0 argument, so the value is passed as an
/// `f64` argument (two words, exact for every `float`).
pub(crate) const FTOL: u32 = 0x00ec_62c0;

/// `RTTI Type Descriptor` of `BSExtraData`, the source type of the
/// checked casts in the `Compare` methods.
pub(crate) const BS_EXTRA_DATA_TYPE: u32 = 0x0118_3b2c;
/// `RTTI Type Descriptor` of `ExtraStartingPosition`.
pub(crate) const EXTRA_STARTING_POSITION_TYPE: u32 = 0x0118_4b10;
/// `RTTI Type Descriptor` of `ExtraLock`.
pub(crate) const EXTRA_LOCK_TYPE: u32 = 0x0118_4754;

/// `0.0` (`double`), what `REFR_LOCK::IsBroken` compares the entry point's
/// result with.
pub(crate) const ZERO: u32 = 0x0101_2060;
/// The global holding the `PlayerCharacter` pointer (`PlayerCharacter::pSingleton`).
pub(crate) const PLAYER: u32 = 0x011d_ea3c;

/// `Actor::GetLevel` (returns the level in AX; `this` is the actor).
pub(crate) const ACTOR_GET_LEVEL: u32 = 0x0087_f9f0;
/// `TESObjectREFR::GetCalcLevel(bool)` (Xbox PDB name).
pub(crate) const REFR_GET_CALC_LEVEL: u32 = 0x0056_7e10;
/// A float game-setting accessor: `this` is the setting (an exe global), the
/// result is the address of its value.
pub(crate) const SETTING_FLOAT_VALUE: u32 = 0x0040_3e20;
/// An integer game-setting accessor: `this` is the setting (an exe global),
/// the result is the address of its value.
pub(crate) const SETTING_INT_VALUE: u32 = 0x0043_d4d0;
/// `BGSEntryPoint::HandleEntryPoint(entryPoint, actor, result, ...)` (Xbox
/// PDB name, cdecl).
pub(crate) const HANDLE_ENTRY_POINT: u32 = 0x005e_58f0;
/// The entry point `REFR_LOCK::IsBroken` asks (its result tells whether the
/// player's perks make a failed attempt count less).
pub(crate) const ENTRY_POINT_LOCK_BROKEN: u32 = 0x20;

/// The float setting `REFR_LOCK::GetLevel` scales a leveled lock's level
/// bonus by.
pub(crate) const LEVELED_LOCK_SETTING: u32 = 0x011c_39b4;
/// The integer settings that hold the top of each lock difficulty bracket
/// (very easy, easy, average, hard, very hard), and the one `00430bc0` also
/// knows for the enumeration value 5 (key only). See
/// the removed `crates/world/src/locks.rs` (git history, 93922e5a).
pub(crate) const LOCK_LEVEL_SETTINGS: [u32; 6] = [
    0x011c_3a4c,
    0x011c_3a30,
    0x011c_3a00,
    0x011c_3ab0,
    0x011c_399c,
    0x011c_3a24,
];

/// `ExtraAnim`'s vtable.
pub(crate) const EXTRA_ANIM_VTABLE: u32 = 0x0101_5b28;
/// `ExtraDismemberedLimbs`'s vtable.
pub(crate) const EXTRA_DISMEMBERED_LIMBS_VTABLE: u32 = 0x0101_5b34;
/// `ExtraStartingPosition`'s vtable.
pub(crate) const EXTRA_STARTING_POSITION_VTABLE: u32 = 0x0101_5b40;
/// The vtable of the `BSExtraData` subclass of type 0x49 built by `004308c0`.
pub(crate) const EXTRA_TYPE_49_VTABLE: u32 = 0x0101_5b4c;
/// `ExtraLight`'s vtable.
pub(crate) const EXTRA_LIGHT_VTABLE: u32 = 0x0101_5b58;
/// `ExtraLock`'s vtable.
pub(crate) const EXTRA_LOCK_VTABLE: u32 = 0x0101_589c;
/// `ExtraFollower`'s vtable.
pub(crate) const EXTRA_FOLLOWER_VTABLE: u32 = 0x0101_5b64;
/// `ExtraGuardedRefData`'s vtable.
pub(crate) const EXTRA_GUARDED_REF_DATA_VTABLE: u32 = 0x0101_5b70;
/// The vtable of the `BSExtraData` subclass of type 0x2e built by `004311f0`.
pub(crate) const EXTRA_TYPE_2E_VTABLE: u32 = 0x0101_5b7c;
/// `ExtraTeleport`'s vtable.
pub(crate) const EXTRA_TELEPORT_VTABLE: u32 = 0x0101_58a8;

/// Extra-data type bytes, as passed to the base constructor.
pub(crate) const TYPE_DISMEMBERED_LIMBS: u32 = 0x5f;
pub(crate) const TYPE_STARTING_POSITION: u32 = 0x0f;
pub(crate) const TYPE_49: u32 = 0x49;
pub(crate) const TYPE_LIGHT: u32 = 0x29;
pub(crate) const TYPE_LOCK: u32 = 0x2a;
pub(crate) const TYPE_FOLLOWER: u32 = 0x1d;
pub(crate) const TYPE_GUARDED_REF_DATA: u32 = 0x7c;
pub(crate) const TYPE_2E: u32 = 0x2e;
pub(crate) const TYPE_TELEPORT: u32 = 0x2b;
pub(crate) const TYPE_MAP_MARKER: u32 = 0x2c;
pub(crate) const TYPE_AUDIO_MARKER: u32 = 0x90;
pub(crate) const TYPE_AUDIO_BUOY_MARKER: u32 = 0x91;
pub(crate) const TYPE_ACTION: u32 = 0x0e;
pub(crate) const TYPE_CONTAINER_CHANGES: u32 = 0x15;
pub(crate) const TYPE_ORIGINAL_REFERENCE: u32 = 0x20;
pub(crate) const TYPE_OWNERSHIP: u32 = 0x21;
pub(crate) const TYPE_GLOBAL: u32 = 0x22;
pub(crate) const TYPE_RANK: u32 = 0x23;
pub(crate) const TYPE_COUNT: u32 = 0x24;
pub(crate) const TYPE_HEALTH: u32 = 0x25;
pub(crate) const TYPE_USES: u32 = 0x26;
pub(crate) const TYPE_TIME_LEFT: u32 = 0x27;
pub(crate) const TYPE_CHARGE: u32 = 0x28;

/// `RTTI Type Descriptor`s of the classes whose `Compare` casts `other`.
pub(crate) const EXTRA_TELEPORT_TYPE: u32 = 0x0118_4430;
pub(crate) const EXTRA_MAP_MARKER_TYPE: u32 = 0x0118_45fc;
pub(crate) const EXTRA_AUDIO_MARKER_TYPE: u32 = 0x0118_45dc;
pub(crate) const EXTRA_AUDIO_BUOY_MARKER_TYPE: u32 = 0x0118_45b8;
pub(crate) const EXTRA_ACTION_TYPE: u32 = 0x0118_4bc0;
pub(crate) const EXTRA_ORIGINAL_REFERENCE_TYPE: u32 = 0x0118_4c00;
pub(crate) const EXTRA_OWNERSHIP_TYPE: u32 = 0x0118_476c;
pub(crate) const EXTRA_GLOBAL_TYPE: u32 = 0x0118_478c;
pub(crate) const EXTRA_RANK_TYPE: u32 = 0x0118_47a8;
pub(crate) const EXTRA_COUNT_TYPE: u32 = 0x0118_47c0;
pub(crate) const EXTRA_LEVELED_ITEM_TYPE: u32 = 0x0118_4680;
pub(crate) const EXTRA_HEALTH_TYPE: u32 = 0x0118_47dc;
pub(crate) const EXTRA_HEALTH_PERC_TYPE: u32 = 0x0118_4208;
pub(crate) const EXTRA_USES_TYPE: u32 = 0x0118_47f8;
pub(crate) const EXTRA_TIME_LEFT_TYPE: u32 = 0x0118_4810;

/// The vtables of the classes built by `00431360` to `00431f60`.
pub(crate) const EXTRA_MAP_MARKER_VTABLE: u32 = 0x0101_5b88;
pub(crate) const EXTRA_AUDIO_MARKER_VTABLE: u32 = 0x0101_5b94;
pub(crate) const EXTRA_AUDIO_BUOY_MARKER_VTABLE: u32 = 0x0101_5ba0;
pub(crate) const EXTRA_ACTION_VTABLE: u32 = 0x0101_5bac;
pub(crate) const EXTRA_CONTAINER_CHANGES_VTABLE: u32 = 0x0101_5bb8;
pub(crate) const EXTRA_ORIGINAL_REFERENCE_VTABLE: u32 = 0x0101_5bc4;
pub(crate) const EXTRA_OWNERSHIP_VTABLE: u32 = 0x0101_58b4;
pub(crate) const EXTRA_GLOBAL_VTABLE: u32 = 0x0101_58c0;
pub(crate) const EXTRA_RANK_VTABLE: u32 = 0x0101_58cc;
pub(crate) const EXTRA_COUNT_VTABLE: u32 = 0x0101_58d8;
pub(crate) const EXTRA_HEALTH_VTABLE: u32 = 0x0101_58e4;
pub(crate) const EXTRA_USES_VTABLE: u32 = 0x0101_58f0;
pub(crate) const EXTRA_TIME_LEFT_VTABLE: u32 = 0x0101_58fc;
pub(crate) const EXTRA_CHARGE_VTABLE: u32 = 0x0101_5908;

/// `DoorTeleportData::Compare` (Xbox PDB; `this` is the data, the argument is
/// the other data): true when the two differ.
pub(crate) const DOOR_TELEPORT_DATA_COMPARE: u32 = 0x0043_a860;
/// `MapMarkerData::Compare` (Xbox PDB; `this` is the data, the argument is
/// the other data): true when the two differ.
pub(crate) const MAP_MARKER_DATA_COMPARE: u32 = 0x0043_8e40;
/// `AudioMarkerData::Compare` (Xbox PDB; `this` is the data, the argument is
/// the other data): true when the two differ.
pub(crate) const AUDIO_MARKER_DATA_COMPARE: u32 = 0x0058_97c0;
/// A 15-byte folded body that ignores its argument and returns true (the
/// engine map names it `DetailedActorPathHandler::IsDetailedPathHandler`);
/// `ExtraAudioBuoyMarker::Compare` calls it with the two `AudioBuoyMarkerData`
/// pointers (`this` is the first, the argument the second).
pub(crate) const AUDIO_BUOY_DATA_COMPARE: u32 = 0x0040_1290;
/// Scalar deleting destructor of the `MapMarkerData` an `ExtraMapMarker` owns
/// (`this` is the data, the argument says whether to free it).
pub(crate) const MAP_MARKER_DATA_DELETE: u32 = 0x0041_9350;
/// Scalar deleting destructor of the `AudioMarkerData` an `ExtraAudioMarker`
/// owns (`this` is the data, the argument says whether to free it).
pub(crate) const AUDIO_MARKER_DATA_DELETE: u32 = 0x0041_9480;
/// Scalar deleting destructor of the `AudioBuoyMarkerData` an
/// `ExtraAudioBuoyMarker` owns (in `the racesexmenu unit` by address range; `this`
/// is the data, the argument says whether to free it).
pub(crate) const AUDIO_BUOY_DATA_DELETE: u32 = 0x007b_3fa0;
/// The destructor body of the `InventoryChanges` an `ExtraContainerChanges`
/// owns (in `the inventorychanges unit`; `this` is the object).
pub(crate) const INVENTORY_CHANGES_DESTRUCT: u32 = 0x004b_f150;

/// `RTTI Type Descriptor`s of the classes whose `Compare` casts `other`
/// (second batch).
pub(crate) const EXTRA_CHARGE_TYPE: u32 = 0x0118_482c;
pub(crate) const EXTRA_SCRIPT_TYPE: u32 = 0x0118_4514;
pub(crate) const EXTRA_WEAPON_MOD_FLAGS_TYPE: u32 = 0x0118_48d8;
pub(crate) const EXTRA_MODDING_ITEM_TYPE: u32 = 0x0118_493c;
pub(crate) const EXTRA_SCALE_TYPE: u32 = 0x0118_4848;
pub(crate) const EXTRA_HOT_KEY_TYPE: u32 = 0x0118_4864;
pub(crate) const EXTRA_SEED_TYPE: u32 = 0x0118_4c98;
pub(crate) const EXTRA_PACKAGE_START_LOCATION_TYPE: u32 = 0x0118_448c;

/// The vtables of the classes built by `00432000` to `00432cb0`.
pub(crate) const EXTRA_SCRIPT_VTABLE: u32 = 0x0101_5914;
pub(crate) const EXTRA_SCALE_VTABLE: u32 = 0x0101_5920;
pub(crate) const EXTRA_HOT_KEY_VTABLE: u32 = 0x0101_592c;
pub(crate) const EXTRA_REFERENCE_POINTER_VTABLE: u32 = 0x0101_5938;
pub(crate) const EXTRA_TRES_PASS_PACKAGE_VTABLE: u32 = 0x0101_5944;
pub(crate) const EXTRA_LEVELED_ITEM_VTABLE: u32 = 0x0101_5950;
pub(crate) const EXTRA_GHOST_VTABLE: u32 = 0x0101_5bd0;
pub(crate) const EXTRA_WORN_VTABLE: u32 = 0x0101_5bdc;
pub(crate) const EXTRA_WORN_LEFT_VTABLE: u32 = 0x0101_5be8;
pub(crate) const EXTRA_CANNOT_WEAR_VTABLE: u32 = 0x0101_5bf4;
pub(crate) const EXTRA_INFO_GENERAL_TOPIC_VTABLE: u32 = 0x0101_5c00;
pub(crate) const EXTRA_SEED_VTABLE: u32 = 0x0101_5c0c;
pub(crate) const EXTRA_PACKAGE_START_LOCATION_VTABLE: u32 = 0x0101_5c18;
pub(crate) const EXTRA_PACKAGE_VTABLE: u32 = 0x0101_5c24;
pub(crate) const EXTRA_PLAYER_CRIME_LIST_VTABLE: u32 = 0x0101_5c30;
pub(crate) const EXTRA_PERSISTENT_CELL_VTABLE: u32 = 0x0101_5c3c;
pub(crate) const EXTRA_RAG_DOLL_DATA_VTABLE: u32 = 0x0101_5c48;

/// Extra-data type bytes of the second batch.
pub(crate) const TYPE_SCRIPT: u32 = 0x0d;
pub(crate) const TYPE_SCALE: u32 = 0x30;
pub(crate) const TYPE_GHOST: u32 = 0x1f;
pub(crate) const TYPE_WORN: u32 = 0x16;
pub(crate) const TYPE_WORN_LEFT: u32 = 0x17;
pub(crate) const TYPE_CANNOT_WEAR: u32 = 0x3e;
pub(crate) const TYPE_HOT_KEY: u32 = 0x4a;
pub(crate) const TYPE_INFO_GENERAL_TOPIC: u32 = 0x4d;
pub(crate) const TYPE_SEED: u32 = 0x31;
pub(crate) const TYPE_PACKAGE_START_LOCATION: u32 = 0x18;
pub(crate) const TYPE_REFERENCE_POINTER: u32 = 0x1c;
pub(crate) const TYPE_PACKAGE: u32 = 0x19;
pub(crate) const TYPE_TRES_PASS_PACKAGE: u32 = 0x1a;
pub(crate) const TYPE_PLAYER_CRIME_LIST: u32 = 0x35;
pub(crate) const TYPE_LEVELED_ITEM: u32 = 0x2f;
pub(crate) const TYPE_PERSISTENT_CELL: u32 = 0x0c;
pub(crate) const TYPE_RAG_DOLL_DATA: u32 = 0x14;

/// Scalar deleting destructor of the `ScriptLocals` an `ExtraScript` owns
/// (in `the extradatalist unit` by address range; `this` is the object, the
/// argument says whether to free it).
pub(crate) const SCRIPT_LOCALS_DELETE: u32 = 0x0041_af70;
/// `MenuTopic::~MenuTopic` (Xbox PDB name; it is the scalar deleting
/// destructor: `this` is the topic, the argument says whether to free it).
pub(crate) const MENU_TOPIC_DELETE: u32 = 0x0042_5ff0;
/// The constructor of the 0x2c-byte `MenuTopic` an `ExtraInfoGeneralTopic`
/// creates (`this` is the memory, the result is the object).
pub(crate) const MENU_TOPIC_CONSTRUCT: u32 = 0x0083_da50;
/// The constructor of the `WORLD_LOCATION` member of
/// `ExtraPackageStartLocation` (`this` is the member).
pub(crate) const WORLD_LOCATION_CONSTRUCT: u32 = 0x006d_5320;
/// `TESPackage::SetIsCreated(created)` (Xbox PDB name; `this` is the
/// package).
pub(crate) const PACKAGE_SET_IS_CREATED: u32 = 0x0067_4d70;
/// The global holding the `TESSaveLoadGame` object pointer (the singleton
/// whose `DeleteForm` `ExtraTresPassPackage`'s destructor calls).
pub(crate) const SAVE_LOAD_GAME: u32 = 0x011d_e45c;
/// A 13-byte stub (`xor al, al`) the destructor of `ExtraTresPassPackage`
/// calls on the save/load object: it always answers false.
pub(crate) const SAVE_LOAD_GAME_ALWAYS_FALSE: u32 = 0x0047_c850;
/// `TESSaveLoadGame::DeleteForm(form)` (Xbox PDB name; `this` is the
/// save/load object).
pub(crate) const SAVE_LOAD_GAME_DELETE_FORM: u32 = 0x0085_a2e0;
/// Adds the item held at the address given to the `BSSimpleList` (`this` is
/// the list; an unplaced body, `005ae3d0`).
pub(crate) const CRIME_LIST_ADD: u32 = 0x005a_e3d0;
/// The destructor body of the `RagDollData` an `ExtraRagDollData` owns (in
/// `the ragdolldata unit`; `this` is the object).
pub(crate) const RAG_DOLL_DATA_DESTRUCT: u32 = 0x004d_9380;

/// `RagDollData::Compare` (Xbox PDB; `this` is the data, the argument the
/// other data): true when the two differ.
pub(crate) const RAG_DOLL_DATA_COMPARE: u32 = 0x004d_9780;

/// `RTTI Type Descriptor`s of the classes whose `Compare` or `Copy` casts
/// `other` (third batch).
pub(crate) const EXTRA_RAG_DOLL_DATA_TYPE: u32 = 0x0118_4cd4;
pub(crate) const EXTRA_ENCOUNTER_ZONE_TYPE: u32 = 0x0118_4cf4;
pub(crate) const EXTRA_ENABLE_STATE_PARENT_TYPE: u32 = 0x0118_4d7c;
pub(crate) const EXTRA_RANDOM_TELEPORT_MARKER_TYPE: u32 = 0x0118_4dcc;
pub(crate) const EXTRA_LINKED_REF_TYPE: u32 = 0x0118_4e1c;
pub(crate) const EXTRA_ACTIVATE_REF_TYPE: u32 = 0x0118_4e84;

/// The vtables of the classes built by `00432e60` to `00433ca0` (names from
/// the RTTI type descriptors).
pub(crate) const EXTRA_ENCOUNTER_ZONE_VTABLE: u32 = 0x0101_5c54;
pub(crate) const EXTRA_USED_MARKERS_VTABLE: u32 = 0x0101_5c60;
pub(crate) const EXTRA_RESERVED_MARKERS_VTABLE: u32 = 0x0101_5c6c;
pub(crate) const EXTRA_RUN_ONCE_PACKS_VTABLE: u32 = 0x0101_5c78;
pub(crate) const EXTRA_DISTANT_DATA_VTABLE: u32 = 0x0101_5c84;
pub(crate) const EXTRA_ENABLE_STATE_PARENT_VTABLE: u32 = 0x0101_5c90;
pub(crate) const EXTRA_ENABLE_STATE_CHILDREN_VTABLE: u32 = 0x0101_5c9c;
pub(crate) const EXTRA_RANDOM_TELEPORT_MARKER_VTABLE: u32 = 0x0101_5ca8;
pub(crate) const EXTRA_LINKED_REF_CHILDREN_VTABLE: u32 = 0x0101_5cb4;
pub(crate) const EXTRA_LINKED_REF_VTABLE: u32 = 0x0101_5cc0;
pub(crate) const EXTRA_ASH_PILE_REF_VTABLE: u32 = 0x0101_5ccc;
pub(crate) const EXTRA_ACTIVATE_REF_CHILDREN_VTABLE: u32 = 0x0101_5cd8;
pub(crate) const EXTRA_ACTIVATE_REF_VTABLE: u32 = 0x0101_5ce4;
pub(crate) const EXTRA_DECAL_REFS_VTABLE: u32 = 0x0101_5cf0;

/// Extra-data type bytes of the third batch.
pub(crate) const TYPE_ENCOUNTER_ZONE: u32 = 0x74;
pub(crate) const TYPE_USED_MARKERS: u32 = 0x12;
pub(crate) const TYPE_RESERVED_MARKERS: u32 = 0x82;
pub(crate) const TYPE_RUN_ONCE_PACKS: u32 = 0x1b;
pub(crate) const TYPE_DISTANT_DATA: u32 = 0x13;
pub(crate) const TYPE_ENABLE_STATE_PARENT: u32 = 0x37;
pub(crate) const TYPE_ENABLE_STATE_CHILDREN: u32 = 0x38;
pub(crate) const TYPE_RANDOM_TELEPORT_MARKER: u32 = 0x3b;
pub(crate) const TYPE_LINKED_REF_CHILDREN: u32 = 0x52;
pub(crate) const TYPE_LINKED_REF: u32 = 0x51;
pub(crate) const TYPE_ASH_PILE_REF: u32 = 0x89;
pub(crate) const TYPE_ACTIVATE_REF_CHILDREN: u32 = 0x54;
pub(crate) const TYPE_ACTIVATE_REF: u32 = 0x53;
pub(crate) const TYPE_DECAL_REFS: u32 = 0x57;

/// `BSSimpleList<T>`'s constructor (`this` is the head node: item and next
/// are cleared).
pub(crate) const SIMPLE_LIST_CONSTRUCT: u32 = 0x0096_a2d0;
/// `BSSimpleList<T>::Clear` (the engine map has no name; `this` is the head
/// node: frees every node after the head and clears its item).
pub(crate) const SIMPLE_LIST_CLEAR: u32 = 0x0047_0470;
/// The destructor body of a `BSSimpleList<T>` (`this` is the head node): it
/// clears the list.
pub(crate) const SIMPLE_LIST_DESTRUCT: u32 = 0x0046_ffb0;
/// `BSSimpleList<T>`'s scalar deleting destructor (`this` is the list, the
/// argument says whether to free it).
pub(crate) const SIMPLE_LIST_DELETE: u32 = 0x0047_02f0;
/// Removes the head of a list (`this` is the head node): the next node's
/// item and successor move into the head and the next node is deleted; the
/// head's item is cleared when there is no next node.
pub(crate) const SIMPLE_LIST_REMOVE_HEAD: u32 = 0x0063_f7b0;
/// Adds the item held at the address given as the new head of the list (`this`
/// is the list; the same body as `CRIME_LIST_ADD`, which names it by its use
/// in `ExtraPlayerCrimeList`).
pub(crate) const SIMPLE_LIST_ADD_HEAD: u32 = CRIME_LIST_ADD;
/// The number of non-empty nodes of a list (`this` is the head node;
/// the engine map calls it `VATS::GetCount` by mistake).
pub(crate) const SIMPLE_LIST_COUNT: u32 = 0x005a_e380;

/// The `float` constant `ExtraDistantData`'s constructor uses for the
/// z component of its land normal.
pub(crate) const DISTANT_DATA_NORMAL_Z: u32 = 0x0101_45a8;
/// `BSStringT<char>::BSStringT` (`this` is the string: empties it).
pub(crate) const BS_STRING_CONSTRUCT: u32 = 0x0040_37b0;
/// `BSStringT<char>::Set(source, length)` (`this` is the string; the source
/// is a character pointer and the length 0 means all of it).
pub(crate) const BS_STRING_SET: u32 = 0x0040_37f0;
/// `BSStringT<char>::Clear` (`this` is the string; `Set(0, 0)`).
pub(crate) const BS_STRING_CLEAR: u32 = 0x0040_37d0;
/// `BSStringT<char>::Set(other)` (Xbox PDB; `this` is the string, the
/// argument the string to copy; in this unit, not yet translated).
pub(crate) const BS_STRING_COPY: u32 = 0x0043_8470;
/// `BSStringT<char>::GetLength` (the folded body: the stored length, or the
/// C string's length when that is 0xffff; `this` is the string).
pub(crate) const BS_STRING_LENGTH: u32 = 0x0040_48e0;
/// `strcmp(a, b)` (the wrapper; `00ec6da0` is the CRT function).
pub(crate) const STRCMP: u32 = 0x0040_8b20;
/// `memcpy(destination, source, size)` (the wrapper around `00ec44d0`).
pub(crate) const MEMCPY: u32 = 0x0040_1460;
/// The constructor of a `REF_ACTIVATE_DATA` (`this` is the 8-byte data:
/// clears the reference and the delay).
pub(crate) const REF_ACTIVATE_DATA_CONSTRUCT: u32 = 0x0041_4010;
/// An empty C string in the exe's data (what `ExtraActivateRef`'s constructor sets
/// its text to).
pub(crate) const EMPTY_STRING: u32 = 0x0101_1584;

/// Constructor of the `DismemberedLimbs` array member of
/// `ExtraDismemberedLimbs` (`this` is the array). In this unit; not yet
/// translated.
pub(crate) const DISMEMBERED_LIMBS_ARRAY_CONSTRUCT: u32 = 0x0043_8570;
/// Destructor of the same array. In this unit; not yet translated.
pub(crate) const DISMEMBERED_LIMBS_ARRAY_DESTRUCT: u32 = 0x0043_85a0;
/// Constructor of the `Guards` array member of `ExtraGuardedRefData`. In
/// this unit; not yet translated.
pub(crate) const GUARDS_ARRAY_CONSTRUCT: u32 = 0x0043_8660;
/// Destructor of the same array. In this unit; not yet translated.
pub(crate) const GUARDS_ARRAY_DESTRUCT: u32 = 0x0043_8690;
/// Constructor of a `DismemberedLimb` entry (`this` is the 0x14-byte block).
pub(crate) const DISMEMBERED_LIMB_CONSTRUCT: u32 = 0x0042_c470;
/// Destructor body of the `ObjectArray` member of a `DismemberedLimb` (`this`
/// is the array).
pub(crate) const DISMEMBERED_LIMB_ARRAY_DESTRUCT: u32 = 0x0042_ff80;
/// `TESNPC::BuildObjectArray(reference, base, array)` (Xbox PDB name); `this`
/// is what `004181e0` returns for the reference.
pub(crate) const TESNPC_BUILD_OBJECT_ARRAY: u32 = 0x0060_5fc0;
/// Takes a reference and returns the object `TESNPC::BuildObjectArray` is
/// called on (a 19-byte accessor in `the extradatalist unit`).
pub(crate) const REFR_NPC_ACCESSOR: u32 = 0x0041_81e0;
/// The constructor of the `FILE_POS_ROT` member of `ExtraStartingPosition`
/// (`this` is the member).
pub(crate) const FILE_POS_ROT_CONSTRUCT: u32 = 0x0069_2710;
/// Deletes the animation an `ExtraAnim` owns (`this` is the animation, the
/// argument says whether to free it).
pub(crate) const ANIMATION_DELETE: u32 = 0x0041_8d20;
/// Deletes the light an `ExtraLight` owns (`this` is the light, the argument
/// says whether to free it).
pub(crate) const LIGHT_DELETE: u32 = 0x0041_8f10;
/// Deletes the teleport data an `ExtraTeleport` owns (`this` is the data,
/// the argument says whether to free it).
pub(crate) const TELEPORT_DATA_DELETE: u32 = 0x0041_9220;
/// Constructor of the 8-byte actor list an `ExtraFollower` owns.
pub(crate) const ACTOR_LIST_CONSTRUCT: u32 = 0x0096_a2d0;
/// Clears the actor list (`this` is the list).
pub(crate) const ACTOR_LIST_CLEAR: u32 = 0x0047_0470;
/// Scalar deleting destructor of the actor list (`this` is the list, the
/// argument says whether to free it).
pub(crate) const ACTOR_LIST_DELETE: u32 = 0x0047_02f0;
/// `LookupFormByID(id)` (cdecl, one stack argument).
pub(crate) const LOOKUP_FORM_BY_ID: u32 = 0x0048_39c0;

/// `RTTI Type Descriptor`s used by the fourth batch: the source and target of
/// the `InitItem`s' cast of a looked-up form (`TESForm`, `TESObjectREFR`), and
/// the classes whose `Compare` or `Copy` casts `other`.
pub(crate) const TES_FORM_TYPE: u32 = 0x0118_3028;
pub(crate) const TES_OBJECT_REFR_TYPE: u32 = 0x0118_41cc;
pub(crate) const EXTRA_DECAL_REFS_TYPE: u32 = 0x0118_4ea4;
pub(crate) const EXTRA_REFLECTED_REFS_TYPE: u32 = 0x0118_3fec;
pub(crate) const EXTRA_REFLECTOR_REFS_TYPE: u32 = 0x0118_4010;
pub(crate) const EXTRA_WATER_LIGHT_REFS_TYPE: u32 = 0x0118_4034;
pub(crate) const EXTRA_LIT_WATER_REFS_TYPE: u32 = 0x0118_4058;
pub(crate) const EXTRA_MERCHANT_CONTAINER_TYPE: u32 = 0x0118_4ec4;
pub(crate) const EXTRA_LEV_CREA_MODIFIER_TYPE: u32 = 0x0118_4eec;
pub(crate) const EXTRA_POISON_TYPE: u32 = 0x0118_461c;
pub(crate) const EXTRA_LAST_FINISHED_SEQUENCE_TYPE: u32 = 0x0118_4408;

/// The vtables of the classes of the fourth batch (names from the RTTI
/// type descriptors).
pub(crate) const EXTRA_REFLECTED_REFS_VTABLE: u32 = 0x0101_4428;
pub(crate) const EXTRA_REFLECTOR_REFS_VTABLE: u32 = 0x0101_4434;
pub(crate) const EXTRA_WATER_LIGHT_REFS_VTABLE: u32 = 0x0101_4440;
pub(crate) const EXTRA_LIT_WATER_REFS_VTABLE: u32 = 0x0101_444c;
pub(crate) const EXTRA_MERCHANT_CONTAINER_VTABLE: u32 = 0x0101_5e04;
pub(crate) const EXTRA_LEV_CREA_MODIFIER_VTABLE: u32 = 0x0101_5e10;
pub(crate) const EXTRA_POISON_VTABLE: u32 = 0x0101_595c;
pub(crate) const EXTRA_LAST_FINISHED_SEQUENCE_VTABLE: u32 = 0x0101_5e1c;
pub(crate) const EXTRA_X_TARGET_VTABLE: u32 = 0x0101_5e28;

/// Extra-data type bytes of the fourth batch.
pub(crate) const TYPE_MERCHANT_CONTAINER: u32 = 0x3c;
pub(crate) const TYPE_LEV_CREA_MODIFIER: u32 = 0x1e;
pub(crate) const TYPE_POISON: u32 = 0x3f;
pub(crate) const TYPE_LAST_FINISHED_SEQUENCE: u32 = 0x41;
pub(crate) const TYPE_X_TARGET: u32 = 0x44;

/// `LCM_NONE` (Xbox PDB `LEV_CREA_MODIFIER`; the others are `LCM_EASY` 0,
/// `LCM_MEDIUM` 1, `LCM_HARD` 2, `LCM_BOSS` 3).
pub(crate) const LEV_CREA_MODIFIER_NONE: u32 = 4;
/// The table of float-setting pointers, one per `LEV_CREA_MODIFIER` value
/// below `LCM_NONE`, that `fn_004350c0` indexes.
pub(crate) const LEV_CREA_SETTING_TABLE: u32 = 0x0118_4ab0;

/// `TESForm::GetFile(index)` (Xbox PDB name; `this` is the form, the argument
/// -1 asks for the last file that changed it).
pub(crate) const FORM_GET_FILE: u32 = 0x0048_4e60;
/// `TESForm::AddCompileIndex(formIdAddress, file)` (Xbox PDB name, cdecl, two
/// stack arguments): adds the file's compile index to the form id held at the
/// address given.
pub(crate) const FORM_ADD_COMPILE_INDEX: u32 = 0x0048_5d50;
/// `TESObjectREFR::GetRefPersists` (Xbox PDB name; `this` is the reference).
pub(crate) const REFR_GET_REF_PERSISTS: u32 = 0x0056_53d0;
/// The accessor of a reference's extra-data list (`this + 0x44`, in
/// `the tesscriptfunctions unit` by address range; `this` is the reference, the
/// result the address of the embedded list).
pub(crate) const REFR_EXTRA_LIST: u32 = 0x005d_43c0;
/// The log function (cdecl, `format, ...`) the `InitItem`s report with.
pub(crate) const MASTERFILE_LOG: u32 = 0x005b_5e40;
/// Two methods of `ExtraDataList` (in `the extradatalist unit` by address range;
/// `this` is the list, the arguments are a reference and a flag, 1 to add)
/// that `ExtraReflectorRefs::InitItem` calls for effect flag bits 0 and 1.
pub(crate) const REFLECTOR_ADD_FIRST: u32 = 0x0041_f4c0;
pub(crate) const REFLECTOR_ADD_SECOND: u32 = 0x0041_f650;
/// `ExtraDataList::SetWaterLightRef(reference, add)` (engine map name; `this`
/// is the extra-data list).
pub(crate) const WATER_LIGHT_REF_SET: u32 = 0x0041_f840;
/// Removes from the list starting at `this` the first node whose item equals
/// the word at the address given (in `the highprocess unit` by address range).
pub(crate) const SIMPLE_LIST_REMOVE_ITEM: u32 = 0x0090_5330;
/// Whether the list at `this` holds an item equal to the word at the address
/// given (the engine map has no name for it).
pub(crate) const SIMPLE_LIST_CONTAINS: u32 = 0x005f_65d0;
/// The constructor of a `REF_DECAL_DATA` (`this` is the 0x1c-byte block; it
/// runs two folded `NiPoint3` constructors and returns `this`).
pub(crate) const REF_DECAL_DATA_CONSTRUCT: u32 = 0x0055_a400;
/// `strlen(string)` (the wrapper around the CRT function `00ec6130`, cdecl).
pub(crate) const STRLEN: u32 = 0x0044_a670;
/// `strcpy_s(destination, size, source)` (the wrapper around `00ec65a6`,
/// cdecl).
pub(crate) const STRING_COPY_CHECKED: u32 = 0x0040_6d30;

/// The messages the `InitItem`s log (the format strings of `MASTERFILE_LOG`;
/// each takes one form id).
pub(crate) const MESSAGE_DECAL_NOT_FOUND: u32 = 0x0101_5cf8;
pub(crate) const MESSAGE_REFLECTOR_NOT_FOUND: u32 = 0x0101_5d8c;
pub(crate) const MESSAGE_NOT_PERSISTENT: u32 = 0x0101_5d30;
pub(crate) const MESSAGE_LIT_WATER_NOT_FOUND: u32 = 0x0101_5dc8;

/// The count of a `BSSimpleArray`.
pub(crate) fn array_size(e: &mut Engine, array: Ptr<BSSimpleArray>) -> u32 {
    typed_call!(leaf::FAMILY, 0x0044_ddc0, leaf::word_at_8(e, array))
}

/// The address of slot `index` of a `BSSimpleArray`.
pub(crate) fn array_slot(e: &mut Engine, array: Ptr<BSSimpleArray>, index: u32) -> Ptr {
    e.call(SIMPLE_ARRAY_AT, &args![array, index]).ptr()
}

/// The pointer stored in slot `index` of a `BSSimpleArray` of pointers.
pub(crate) fn array_pointer_at<T>(e: &mut Engine, array: Ptr<BSSimpleArray>, index: u32) -> Ptr<T> {
    let slot = array_slot(e, array, index);
    Ptr::new(e.mem.u32(slot.addr()))
}

/// `_ftol2_sse` on a `float`.
pub(crate) fn float_to_int(e: &mut Engine, value: f32) -> i32 {
    e.call(FTOL, &args![value as f64]).i32()
}

/// The base constructor, the vtable store that follows it.
pub(crate) fn construct_base(e: &mut Engine, this: Ptr, extra_type: u32, vtable: u32) {
    e.call(BS_EXTRA_DATA_CONSTRUCT, &args![this, extra_type]);
    e.mem.set_u32(this.addr(), vtable);
}

/// `operator delete(this)` when bit 0 of `flags` is set (the tail of every
/// scalar deleting destructor).
pub(crate) fn delete_when_asked(e: &mut Engine, this: Ptr, flags: u32) {
    if flags & 1 != 0 {
        e.call(OPERATOR_DELETE, &args![this]);
    }
}

/// The start every `Compare` below shares: casts `other` to the class with
/// the RTTI descriptor `target_type` and asks `BSExtraData::Compare`. `None`
/// means the answer is already true (`other` is not of that class, or the base
/// says they differ); `Some(cast)` is `other` as the class, whose own fields
/// remain to be compared.
pub(crate) fn compare_prologue<T, U>(
    e: &mut Engine,
    this: Ptr<T>,
    other: Ptr,
    target_type: u32,
) -> Option<Ptr<U>> {
    let cast: Ptr<U> = e
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

/// The destructor body of the classes that own one object through the pointer
/// at +0xc: resets the vtable, runs the owned object's scalar deleting
/// destructor (`deleter`, with its flag set to 1) when there is one, then the
/// base destructor. The exception-unwinding frame is not translated.
pub(crate) fn destroy_owner(e: &mut Engine, this: Ptr, vtable: u32, deleter: u32) {
    e.mem.set_u32(this.addr(), vtable);
    let owned = Ptr::<()>::new(e.mem.u32(this.addr() + 0xc));
    if !owned.is_null() {
        e.call(deleter, &args![owned, 1u32]);
    }
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 004300f0
/// `ExtraAnim::_scalar_deleting_destructor_` (Xbox PDB): the destructor, then
/// `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_anim_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraAnim>,
    flags: u32,
) -> Ptr<ExtraAnim> {
    typed_call!(FAMILY, 0x0043_0120, fn_00430120(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00430120
/// `ExtraAnim`'s destructor body (the engine map has no name for it): resets
/// the vtable, deletes the owned animation if there is one, then runs the
/// base destructor. The exception-unwinding frame is not translated.
pub fn fn_00430120(e: &mut Engine, this: Ptr<ExtraAnim>) {
    let s = store(e);
    e.mem.set_u32(this.addr(), EXTRA_ANIM_VTABLE);
    let animation = s.get(e, this, ExtraAnim::pAnimation);
    if !animation.is_null() {
        e.call(ANIMATION_DELETE, &args![animation, 1u32]);
    }
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 004301b0
/// The `u16` at `this + 0x4c + 2 * slot` of an animation-sequence object, where
/// `slot` is the section number except that section 0x14 (the whole body) uses
/// slot 1 and section 0x15 (the upper body) uses slot 4. The class is not named
/// by the Xbox PDB or the engine map.
pub fn fn_004301b0(e: &mut Engine, this: Ptr, section: i32) -> u16 {
    let slot = match section {
        0x14 => 1,
        0x15 => 4,
        other => other,
    };
    e.mem.u16(
        this.addr()
            .wrapping_add(0x4c)
            .wrapping_add((slot as u32).wrapping_mul(2)),
    )
}

// Typed from 00430200
/// `ExtraDismemberedLimbs::ExtraDismemberedLimbs` (Xbox PDB): extra-data type
/// 0x5f, an empty limb array, no limbs dismembered, no cause of death
/// (`eCauseofDeath` and `eLastHitLimb` are -1), not eaten. Returns `this`.
pub fn extra_dismembered_limbs_extra_dismembered_limbs(
    e: &mut Engine,
    this: Ptr<ExtraDismemberedLimbs>,
) -> Ptr<ExtraDismemberedLimbs> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_DISMEMBERED_LIMBS,
        EXTRA_DISMEMBERED_LIMBS_VTABLE,
    );
    let array = this.at(ExtraDismemberedLimbs::DismemberedLimbs);
    e.call(DISMEMBERED_LIMBS_ARRAY_CONSTRUCT, &args![array]);
    e.set(this, ExtraDismemberedLimbs::sLimbs, 0);
    s.set(e, this, ExtraDismemberedLimbs::eCauseofDeath, 0xffff_ffff);
    s.set(e, this, ExtraDismemberedLimbs::pDeathObject, Ptr::NULL);
    s.set(e, this, ExtraDismemberedLimbs::eLastHitLimb, 0xffff_ffff);
    e.set(this, ExtraDismemberedLimbs::bEaten, false);
    this
}

// Typed from 004302a0
/// `ExtraDismemberedLimbs::_scalar_deleting_destructor_` (Xbox PDB): the
/// destructor, then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_dismembered_limbs_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraDismemberedLimbs>,
    flags: u32,
) -> Ptr<ExtraDismemberedLimbs> {
    typed_call!(FAMILY, 0x0043_02d0, fn_004302d0(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 004302d0
/// `ExtraDismemberedLimbs`' destructor body: deletes every non-null
/// `DismemberedLimb` in the array (the count is read again on every pass, as
/// the loop condition does), destroys the array, then runs the base
/// destructor. The exception-unwinding frame is not translated.
pub fn fn_004302d0(e: &mut Engine, this: Ptr<ExtraDismemberedLimbs>) {
    e.mem.set_u32(this.addr(), EXTRA_DISMEMBERED_LIMBS_VTABLE);
    let array = this.at(ExtraDismemberedLimbs::DismemberedLimbs);
    let mut index = 0;
    while index < array_size(e, array) {
        let limb: Ptr<DismemberedLimb> = array_pointer_at(e, array, index);
        if !limb.is_null() {
            typed_call!(FAMILY, 0x0043_0390, fn_00430390(e, limb, 1));
        }
        index += 1;
    }
    e.call(DISMEMBERED_LIMBS_ARRAY_DESTRUCT, &args![array]);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 00430390
/// `DismemberedLimb`'s scalar deleting destructor (the engine map has no
/// name): the destructor, then `operator delete` when `flags & 1`. Returns
/// `this`.
pub fn fn_00430390(e: &mut Engine, this: Ptr<DismemberedLimb>, flags: u32) -> Ptr<DismemberedLimb> {
    typed_call!(FAMILY, 0x0043_03c0, fn_004303c0(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 004303c0
/// `DismemberedLimb`'s destructor: destroys the `ObjectArray` member.
pub fn fn_004303c0(e: &mut Engine, this: Ptr<DismemberedLimb>) {
    let array = this.at(DismemberedLimb::ObjectArray);
    e.call(DISMEMBERED_LIMB_ARRAY_DESTRUCT, &args![array]);
}

// Typed from 004303e0
/// `ExtraDismemberedLimbs::Dismembered` (Xbox PDB): whether bit `limb` of
/// `sLimbs` is set. The shift count is taken modulo 32, as `SHL` does.
pub fn extra_dismembered_limbs_dismembered(
    e: &mut Engine,
    this: Ptr<ExtraDismemberedLimbs>,
    limb: u8,
) -> bool {
    let limbs = e.get(this, ExtraDismemberedLimbs::sLimbs) as u32;
    limbs & (1u32 << (limb & 31)) != 0
}

// Typed from 00430410
/// `ExtraDismemberedLimbs::Dismember` (Xbox PDB): records that `limb` of
/// `actor` (a `TESObjectREFR*`) is dismembered, unless it already is.
///
/// A new `DismemberedLimb` (limb, `exploded`, not identical) is appended to
/// the array. An exploded limb is simply marked identical. Otherwise, for a
/// non-player actor whose vtable `+0x100` and `+0x218` functions are both
/// true, the actor's object array is built into the new entry, and if the
/// most recent earlier entry that is not itself marked identical has the same
/// object array, the new entry is marked identical and its array cleared
/// (`008454f0(array, 1)`).
///
/// The exception-unwinding frame is not translated.
pub fn extra_dismembered_limbs_dismember(
    e: &mut Engine,
    this: Ptr<ExtraDismemberedLimbs>,
    actor: Ptr,
    limb: u8,
    exploded: u8,
) {
    if typed_call!(
        FAMILY,
        0x0043_03e0,
        extra_dismembered_limbs_dismembered(e, this, limb)
    ) {
        return;
    }
    let limbs = e.get(this, ExtraDismemberedLimbs::sLimbs) as u32;
    e.set(
        this,
        ExtraDismemberedLimbs::sLimbs,
        low_half(limbs | (1u32 << (limb & 31))),
    );

    let memory = e.call(OPERATOR_NEW, &args![0x14u32]).u32();
    let entry: Ptr<DismemberedLimb> = if memory != 0 {
        e.call(DISMEMBERED_LIMB_CONSTRUCT, &args![memory]).ptr()
    } else {
        Ptr::NULL
    };
    let array = this.at(ExtraDismemberedLimbs::DismemberedLimbs);
    // The game appends the pointer held in a stack local and then reads the
    // local again to fill the entry in.
    let (entry, previous_count) = e.with_stack(4, |e, local| {
        e.mem.set_u32(local.addr(), entry.addr());
        let count = e.call(SIMPLE_ARRAY_ADD, &args![array, local]).u32();
        (Ptr::<DismemberedLimb>::new(e.mem.u32(local.addr())), count)
    });
    e.set(entry, DismemberedLimb::cLimb, limb);
    e.set(entry, DismemberedLimb::bLimbExploded, exploded);
    e.set(entry, DismemberedLimb::bObjectArrayIdentical, 0);

    if exploded != 0 {
        e.set(entry, DismemberedLimb::bObjectArrayIdentical, 1);
        return;
    }
    if !e.vcall(actor.addr(), 0x100, &[]).bool() {
        return;
    }
    if actor.addr() == e.global::<u32>(PLAYER) {
        return;
    }
    if !e.vcall(actor.addr(), 0x218, &[]).bool() {
        return;
    }
    let npc = e.call(REFR_NPC_ACCESSOR, &args![actor]).u32();
    let object_array = entry.at(DismemberedLimb::ObjectArray);
    let base = e.vcall(actor.addr(), 0x1e8, &[]).u32();
    e.call(
        TESNPC_BUILD_OBJECT_ARRAY,
        &args![npc, actor, base, object_array],
    );
    if previous_count == 0 {
        return;
    }
    // The newest earlier entry that is not itself marked identical.
    let mut index = previous_count as i32 - 1;
    let earlier: Ptr<DismemberedLimb> = loop {
        if index < 0 {
            return;
        }
        let candidate: Ptr<DismemberedLimb> = array_pointer_at(e, array, index as u32);
        if e.get(candidate, DismemberedLimb::bObjectArrayIdentical) == 0 {
            break candidate;
        }
        index -= 1;
    };
    let earlier_array = earlier.at(DismemberedLimb::ObjectArray);
    if e.call(
        SIMPLE_ARRAY_COMPARE_BUFFER,
        &args![earlier_array, object_array],
    )
    .bool()
    {
        e.set(entry, DismemberedLimb::bObjectArrayIdentical, 1);
        e.call(SIMPLE_ARRAY_CLEAR, &args![object_array, 1u32]);
    }
}

// Typed from 004305f0
/// Sets `bLimbRemoved` to `removed` on every entry whose `cLimb` is `limb`
/// (the Xbox PDB has no name for this method of `ExtraDismemberedLimbs`).
pub fn fn_004305f0(e: &mut Engine, this: Ptr<ExtraDismemberedLimbs>, limb: u32, removed: u8) {
    let array = this.at(ExtraDismemberedLimbs::DismemberedLimbs);
    let mut index = 0;
    while index < array_size(e, array) {
        let entry: Ptr<DismemberedLimb> = array_pointer_at(e, array, index);
        if !entry.is_null() && e.get(entry, DismemberedLimb::cLimb) as u32 == limb {
            e.set(entry, DismemberedLimb::bLimbRemoved, removed);
        }
        index += 1;
    }
}

// Typed from 00430660
/// `ExtraDismemberedLimbs::CheckDismemberedLimbsIdentical` (Xbox PDB):
/// whether `other` has the same dismembered limbs, the same number of
/// entries, and entry by entry the same limb, exploded, identical and removed
/// flags and the same object array.
pub fn extra_dismembered_limbs_check_dismembered_limbs_identical(
    e: &mut Engine,
    this: Ptr<ExtraDismemberedLimbs>,
    other: Ptr<ExtraDismemberedLimbs>,
) -> bool {
    if e.get(other, ExtraDismemberedLimbs::sLimbs) != e.get(this, ExtraDismemberedLimbs::sLimbs) {
        return false;
    }
    let other_array = other.at(ExtraDismemberedLimbs::DismemberedLimbs);
    let this_array = this.at(ExtraDismemberedLimbs::DismemberedLimbs);
    if array_size(e, other_array) != array_size(e, this_array) {
        return false;
    }
    let mut index = 0;
    while index < array_size(e, this_array) {
        let mine: Ptr<DismemberedLimb> = array_pointer_at(e, this_array, index);
        let theirs: Ptr<DismemberedLimb> = array_pointer_at(e, other_array, index);
        if e.get(mine, DismemberedLimb::cLimb) != e.get(theirs, DismemberedLimb::cLimb)
            || e.get(mine, DismemberedLimb::bLimbExploded)
                != e.get(theirs, DismemberedLimb::bLimbExploded)
            || e.get(mine, DismemberedLimb::bObjectArrayIdentical)
                != e.get(theirs, DismemberedLimb::bObjectArrayIdentical)
            || e.get(mine, DismemberedLimb::bLimbRemoved)
                != e.get(theirs, DismemberedLimb::bLimbRemoved)
        {
            return false;
        }
        let mine_objects = mine.at(DismemberedLimb::ObjectArray);
        let their_objects = theirs.at(DismemberedLimb::ObjectArray);
        if !e
            .call(
                SIMPLE_ARRAY_COMPARE_BUFFER,
                &args![mine_objects, their_objects],
            )
            .bool()
        {
            return false;
        }
        index += 1;
    }
    true
}

// Typed from 00430780
/// `ExtraStartingPosition`'s constructor from a reference (the engine map has
/// no name for it): extra-data type 0x0f; the position (the three floats the
/// reference's vtable `+0x1f4` function returns the address of) and the
/// rotation (`00430830` of the reference) are copied into `startPosition`.
/// Returns `this`. The exception-unwinding frame is not translated.
pub fn fn_00430780(
    e: &mut Engine,
    this: Ptr<ExtraStartingPosition>,
    reference: Ptr,
) -> Ptr<ExtraStartingPosition> {
    construct_base(
        e,
        this.cast(),
        TYPE_STARTING_POSITION,
        EXTRA_STARTING_POSITION_VTABLE,
    );
    let start = this.at(ExtraStartingPosition::startPosition);
    e.call(FILE_POS_ROT_CONSTRUCT, &args![start]);
    let position = e.vcall(reference.addr(), 0x1f4, &[]).u32();
    let target = start.at(FilePosRot::pos).addr();
    copy_words(e, position, target);
    let rotation = typed_call!(FAMILY, 0x0043_0830, fn_00430830(e, reference));
    let target = start.at(FilePosRot::rot).addr();
    copy_words(e, rotation.addr(), target);
    this
}

/// Copies three 32-bit words (a `NiPoint3`) bit for bit.
pub(crate) fn copy_words(e: &mut Engine, from: u32, to: u32) {
    for word in 0..3 {
        let value = e.mem.u32(from + 4 * word);
        e.mem.set_u32(to + 4 * word, value);
    }
}

// Typed from 00430830
/// The address of the 12-byte vector at `this + 0x24` (an accessor of the
/// reference passed to `00430780`; the engine map attributes it to this unit
/// by address range only).
pub fn fn_00430830(_e: &mut Engine, this: Ptr) -> Ptr {
    this.byte_add(0x24)
}

// Typed from 00430850
/// `ExtraStartingPosition::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraStartingPosition`, when `BSExtraData::Compare` returns true, or when
/// the 0x18 bytes of `startPosition` differ.
pub fn extra_starting_position_compare(
    e: &mut Engine,
    this: Ptr<ExtraStartingPosition>,
    other: Ptr,
) -> bool {
    let cast: Ptr<ExtraStartingPosition> = e
        .call(
            DYNAMIC_CAST,
            &args![
                other,
                0u32,
                BS_EXTRA_DATA_TYPE,
                EXTRA_STARTING_POSITION_TYPE,
                0u32
            ],
        )
        .ptr();
    if cast.is_null() {
        return true;
    }
    if e.call(BS_EXTRA_DATA_COMPARE, &args![this, other]).bool() {
        return true;
    }
    let mine = this.at(ExtraStartingPosition::startPosition);
    let theirs = cast.at(ExtraStartingPosition::startPosition);
    e.call(MEMCMP, &args![mine, theirs, 0x18u32]).u32() != 0
}

// Typed from 004308c0
/// Constructor of the `BSExtraData` subclass of type 0x49 (vtable
/// `01015b4c`; no Xbox PDB name is known): the word at +0xc is cleared.
/// Returns `this`.
pub fn fn_004308c0(e: &mut Engine, this: Ptr<ExtraType49>) -> Ptr<ExtraType49> {
    let s = store(e);
    construct_base(e, this.cast(), TYPE_49, EXTRA_TYPE_49_VTABLE);
    s.set(e, this, ExtraType49::unnamed0C, 0);
    this
}

// Typed from 004308f0
/// `ExtraLight`'s constructor (the engine map has no name for it): extra-data
/// type 0x29, taking ownership of `light`. Returns `this`.
pub fn fn_004308f0(e: &mut Engine, this: Ptr<ExtraLight>, light: Ptr) -> Ptr<ExtraLight> {
    let s = store(e);
    construct_base(e, this.cast(), TYPE_LIGHT, EXTRA_LIGHT_VTABLE);
    s.set(e, this, ExtraLight::pLight, light);
    this
}

// Typed from 00430920
/// `ExtraLight::_scalar_deleting_destructor_` (Xbox PDB): the destructor,
/// then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_light_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraLight>,
    flags: u32,
) -> Ptr<ExtraLight> {
    typed_call!(FAMILY, 0x0043_0950, fn_00430950(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00430950
/// `ExtraLight`'s destructor body (the engine map has no name for it): resets
/// the vtable, deletes the owned light if there is one, then runs the base
/// destructor. The exception-unwinding frame is not translated.
pub fn fn_00430950(e: &mut Engine, this: Ptr<ExtraLight>) {
    let s = store(e);
    e.mem.set_u32(this.addr(), EXTRA_LIGHT_VTABLE);
    let light = s.get(e, this, ExtraLight::pLight);
    if !light.is_null() {
        e.call(LIGHT_DELETE, &args![light, 1u32]);
    }
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 004309e0
/// `REFR_LOCK::GetLockLevel` (Xbox PDB): the difficulty bracket (0 very easy
/// to 4 very hard, 5 key only) of the lock's level for `reference`.
pub fn refr_lock_get_lock_level(e: &mut Engine, this: Ptr<RefrLock>, reference: Ptr) -> i32 {
    let level = typed_call!(FAMILY, 0x0043_0a10, refr_lock_get_level(e, this, reference));
    typed_call!(
        FAMILY,
        0x0043_0b40,
        refr_lock_numeric_value_to_enum(e, level)
    )
}

// Typed from 00430a10
/// `REFR_LOCK::GetLevel` (Xbox PDB): the lock's level. A leveled lock
/// (`cFlags` bit 2) adds the level of `reference` (the player when it is
/// null) times the float setting at `011c39b4` truncated to an integer, and
/// is capped at 99.
pub fn refr_lock_get_level(e: &mut Engine, this: Ptr<RefrLock>, reference: Ptr) -> i32 {
    let mut level = e.get(this, RefrLock::cBaseLevel) as i32;
    if e.get(this, RefrLock::cFlags) as i32 & 4 != 0 {
        let reference_level = if reference.is_null() {
            let player = e.global::<u32>(PLAYER);
            e.call(ACTOR_GET_LEVEL, &args![player]).u16() as i32
        } else {
            e.call(REFR_GET_CALC_LEVEL, &args![reference, 0u32]).i32()
        };
        let setting = e
            .call(SETTING_FLOAT_VALUE, &args![LEVELED_LOCK_SETTING])
            .u32();
        let scale = e.mem.f32(setting);
        let scale = float_to_int(e, scale);
        level = scale.wrapping_mul(reference_level).wrapping_add(level);
        if level > 99 {
            level = 99;
        }
    }
    level
}

// Typed from 00430a90
/// `REFR_LOCK::SetLocked` (Xbox PDB): sets or clears bit 0 of `cFlags`;
/// unlocking also resets `uiNumTries`.
pub fn refr_lock_set_locked(e: &mut Engine, this: Ptr<RefrLock>, locked: bool) {
    let s = store(e);
    let flags = e.get(this, RefrLock::cFlags);
    if locked {
        e.set(this, RefrLock::cFlags, flags | 1);
    } else {
        e.set(this, RefrLock::cFlags, flags & !1);
        s.set(e, this, RefrLock::uiNumTries, 0);
    }
}

// Typed from 00430ae0
/// `REFR_LOCK::IsBroken` (Xbox PDB): whether `uiNumTries` has reached the
/// number of tries the lock takes: 1 when the entry point 0x20 (asked of the
/// player) leaves its value at 0.0, otherwise 2.
pub fn refr_lock_is_broken(e: &mut Engine, this: Ptr<RefrLock>) -> bool {
    let s = store(e);
    let player = e.global::<u32>(PLAYER);
    let value = e.with_stack(4, |e, result| {
        e.mem.set_f32(result.addr(), 0.0);
        e.call(
            HANDLE_ENTRY_POINT,
            &args![ENTRY_POINT_LOCK_BROKEN, player, result],
        );
        e.mem.f32(result.addr())
    });
    let zero: f64 = e.global(ZERO);
    let tries_allowed: u32 = if value as f64 == zero { 1 } else { 2 };
    s.get(e, this, RefrLock::uiNumTries) >= tries_allowed
}

// Typed from 00430b40
/// `REFR_LOCK::NumericValueToEnum` (Xbox PDB): the first difficulty bracket
/// whose top setting (`LOCK_LEVEL_SETTINGS`) is at least `value`; 5 when
/// `value` passes every one.
pub fn refr_lock_numeric_value_to_enum(e: &mut Engine, value: i32) -> i32 {
    for (bracket, setting) in (0i32..).zip(LOCK_LEVEL_SETTINGS[..5].iter()) {
        let top = e.call(SETTING_INT_VALUE, &args![*setting]).u32();
        if value <= e.mem.i32(top) {
            return bracket;
        }
    }
    5
}

// Typed from 00430bc0
/// The inverse of `REFR_LOCK::NumericValueToEnum` (no Xbox PDB name): the
/// value of the setting for difficulty `bracket` (0 to 5), 0 for anything
/// else. The switch is the jump table at `00430c50`.
pub fn fn_00430bc0(e: &mut Engine, bracket: u32) -> i32 {
    match LOCK_LEVEL_SETTINGS.get(bracket as usize) {
        Some(setting) => {
            let value = e.call(SETTING_INT_VALUE, &args![*setting]).u32();
            e.mem.i32(value)
        }
        None => 0,
    }
}

// Typed from 00430c70
/// `ExtraLock::ExtraLock` (Xbox PDB): extra-data type 0x2a, taking ownership
/// of `lock`. Returns `this`.
pub fn extra_lock_extra_lock(
    e: &mut Engine,
    this: Ptr<ExtraLock>,
    lock: Ptr<RefrLock>,
) -> Ptr<ExtraLock> {
    let s = store(e);
    construct_base(e, this.cast(), TYPE_LOCK, EXTRA_LOCK_VTABLE);
    s.set(e, this, ExtraLock::pLock, lock);
    this
}

// Typed from 00430ca0
/// `ExtraLock`'s destructor body (the engine map has no name for it; the
/// scalar deleting destructor is `0042c550`, in `the extradatalist unit`): resets
/// the vtable, frees the owned lock (without a null check), then runs the base
/// destructor.
pub fn fn_00430ca0(e: &mut Engine, this: Ptr<ExtraLock>) {
    let s = store(e);
    e.mem.set_u32(this.addr(), EXTRA_LOCK_VTABLE);
    let lock = s.get(e, this, ExtraLock::pLock);
    e.call(OPERATOR_DELETE, &args![lock]);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 00430ce0
/// `ExtraLock::Compare` (Xbox PDB): true when `other` is not an `ExtraLock`,
/// when `BSExtraData::Compare` returns true, or when the two locks differ
/// in `cFlags`, `pKey`, difficulty bracket of `cBaseLevel`, `uiNumTries` or
/// `uiTimesUnlocked`.
pub fn extra_lock_compare(e: &mut Engine, this: Ptr<ExtraLock>, other: Ptr) -> bool {
    let s = store(e);
    let cast: Ptr<ExtraLock> = e
        .call(
            DYNAMIC_CAST,
            &args![other, 0u32, BS_EXTRA_DATA_TYPE, EXTRA_LOCK_TYPE, 0u32],
        )
        .ptr();
    if cast.is_null() {
        return true;
    }
    if e.call(BS_EXTRA_DATA_COMPARE, &args![this, other]).bool() {
        return true;
    }
    let mine = s.get(e, this, ExtraLock::pLock);
    let theirs = s.get(e, cast, ExtraLock::pLock);
    if e.get(mine, RefrLock::cFlags) != e.get(theirs, RefrLock::cFlags) {
        return true;
    }
    if s.get(e, mine, RefrLock::pKey) != s.get(e, theirs, RefrLock::pKey) {
        return true;
    }
    let my_bracket = typed_call!(
        FAMILY,
        0x0043_0b40,
        refr_lock_numeric_value_to_enum(e, e.get(mine, RefrLock::cBaseLevel) as i32)
    );
    let their_bracket = typed_call!(
        FAMILY,
        0x0043_0b40,
        refr_lock_numeric_value_to_enum(e, e.get(theirs, RefrLock::cBaseLevel) as i32)
    );
    if my_bracket != their_bracket {
        return true;
    }
    if s.get(e, mine, RefrLock::uiNumTries) != s.get(e, theirs, RefrLock::uiNumTries) {
        return true;
    }
    s.get(e, mine, RefrLock::uiTimesUnlocked) != s.get(e, theirs, RefrLock::uiTimesUnlocked)
}

// Typed from 00430dd0
/// `ExtraFollower::ExtraFollower` (Xbox PDB): extra-data type 0x1d and a
/// newly allocated, constructed 8-byte actor list (null when the allocation
/// fails). Returns `this`. The exception-unwinding frame is not translated.
pub fn extra_follower_extra_follower(
    e: &mut Engine,
    this: Ptr<ExtraFollower>,
) -> Ptr<ExtraFollower> {
    let s = store(e);
    construct_base(e, this.cast(), TYPE_FOLLOWER, EXTRA_FOLLOWER_VTABLE);
    let memory = e.call(OPERATOR_NEW, &args![8u32]).u32();
    let list: Ptr = if memory != 0 {
        e.call(ACTOR_LIST_CONSTRUCT, &args![memory]).ptr()
    } else {
        Ptr::NULL
    };
    s.set(e, this, ExtraFollower::pActorlist, list);
    this
}

// Typed from 00430e70
/// `ExtraFollower::_scalar_deleting_destructor_` (Xbox PDB): the destructor,
/// then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_follower_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraFollower>,
    flags: u32,
) -> Ptr<ExtraFollower> {
    typed_call!(FAMILY, 0x0043_0ea0, fn_00430ea0(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00430ea0
/// `ExtraFollower`'s destructor body (the engine map has no name for it):
/// resets the vtable, clears the actor list (called even when the pointer is
/// null), deletes it if there is one, then runs the base destructor. The
/// exception-unwinding frame is not translated.
pub fn fn_00430ea0(e: &mut Engine, this: Ptr<ExtraFollower>) {
    let s = store(e);
    e.mem.set_u32(this.addr(), EXTRA_FOLLOWER_VTABLE);
    let list = s.get(e, this, ExtraFollower::pActorlist);
    e.call(ACTOR_LIST_CLEAR, &args![list]);
    let list = s.get(e, this, ExtraFollower::pActorlist);
    if !list.is_null() {
        e.call(ACTOR_LIST_DELETE, &args![list, 1u32]);
    }
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 00430f30
/// `ExtraGuardedRefData::ExtraGuardedRefData` (Xbox PDB): extra-data type
/// 0x7c and an empty `Guards` array. Returns `this`. The exception-unwinding
/// frame is not translated.
pub fn extra_guarded_ref_data_extra_guarded_ref_data(
    e: &mut Engine,
    this: Ptr<ExtraGuardedRefData>,
) -> Ptr<ExtraGuardedRefData> {
    construct_base(
        e,
        this.cast(),
        TYPE_GUARDED_REF_DATA,
        EXTRA_GUARDED_REF_DATA_VTABLE,
    );
    let guards = this.at(ExtraGuardedRefData::Guards);
    e.call(GUARDS_ARRAY_CONSTRUCT, &args![guards]);
    this
}

// Typed from 00430fa0
/// `ExtraGuardedRefData::_scalar_deleting_destructor_` (Xbox PDB): the
/// destructor, then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_guarded_ref_data_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraGuardedRefData>,
    flags: u32,
) -> Ptr<ExtraGuardedRefData> {
    typed_call!(FAMILY, 0x0043_0fd0, fn_00430fd0(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00430fd0
/// `ExtraGuardedRefData`'s destructor body (the engine map has no name for
/// it): resets the vtable, destroys the `Guards` array, then runs the base
/// destructor. The exception-unwinding frame is not translated.
pub fn fn_00430fd0(e: &mut Engine, this: Ptr<ExtraGuardedRefData>) {
    e.mem.set_u32(this.addr(), EXTRA_GUARDED_REF_DATA_VTABLE);
    let guards = this.at(ExtraGuardedRefData::Guards);
    e.call(GUARDS_ARRAY_DESTRUCT, &args![guards]);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 00431030
/// `ExtraGuardedRefData::Compare` (Xbox PDB): true when
/// `BSExtraData::Compare` returns false (the opposite test to the other
/// `Compare` methods of this unit, as the code has it), when the guard counts
/// differ, or when some guard of `this` is not in `other`'s array. `other` is
/// used as an `ExtraGuardedRefData` without a type check.
pub fn extra_guarded_ref_data_compare(
    e: &mut Engine,
    this: Ptr<ExtraGuardedRefData>,
    other: Ptr<ExtraGuardedRefData>,
) -> bool {
    if !e.call(BS_EXTRA_DATA_COMPARE, &args![this, other]).bool() {
        return true;
    }
    let their_guards = other.at(ExtraGuardedRefData::Guards);
    let my_guards = this.at(ExtraGuardedRefData::Guards);
    if array_size(e, their_guards) != array_size(e, my_guards) {
        return true;
    }
    let mut index = 0;
    while index < array_size(e, my_guards) {
        let guard = array_slot(e, my_guards, index);
        let found = e
            .call(
                SIMPLE_ARRAY_FIND,
                &args![their_guards, guard, 0u32, GUARD_COMPARE],
            )
            .u32();
        if found == 0xffff_ffff {
            return true;
        }
        index += 1;
    }
    false
}

// Typed from 004310d0
/// `ExtraGuardedRefData::AddGuard` (Xbox PDB): appends the value
/// `0084e3a0` gives for `reference` to `Guards` unless it is already there.
pub fn extra_guarded_ref_data_add_guard(
    e: &mut Engine,
    this: Ptr<ExtraGuardedRefData>,
    reference: Ptr,
) {
    let guard = typed_call!(leaf::FAMILY, 0x0084_e3a0, leaf::word_at_c(e, reference));
    let guards = this.at(ExtraGuardedRefData::Guards);
    e.with_stack(4, |e, local| {
        e.mem.set_u32(local.addr(), guard);
        let found = e
            .call(
                SIMPLE_ARRAY_FIND,
                &args![guards, local, 0u32, GUARD_COMPARE],
            )
            .u32();
        if found == 0xffff_ffff {
            e.call(SIMPLE_ARRAY_ADD, &args![guards, local]);
        }
    });
}

// Typed from 00431120
/// For every guard in `Guards` (the engine map has no name for this method of
/// `ExtraGuardedRefData`): looks the form up, and when it is a form whose
/// vtable `+0xf0` and `+0x100` functions are both true and `008d8520` gives it
/// a process, calls that process's vtable `+0x60c` function with the form,
/// `second` and `first`.
pub fn fn_00431120(e: &mut Engine, this: Ptr<ExtraGuardedRefData>, first: u32, second: u32) {
    let guards = this.at(ExtraGuardedRefData::Guards);
    let mut index = 0;
    while index < array_size(e, guards) {
        let id = array_pointer_at::<()>(e, guards, index).addr();
        let form = e.call(LOOKUP_FORM_BY_ID, &args![id]).u32();
        if form != 0 && e.vcall(form, 0xf0, &[]).bool() && e.vcall(form, 0x100, &[]).bool() {
            let process = typed_call!(leaf::FAMILY, 0x008d_8520, leaf::word_at_68(e, form));
            if process != 0 {
                e.vcall(process, 0x60c, &args![form, second, first]);
            }
        }
        index += 1;
    }
}

// Typed from 004311f0
/// Constructor of the `BSExtraData` subclass of type 0x2e (vtable
/// `01015b7c`; no Xbox PDB name is known): the two words at +0xc and +0x10
/// are cleared. Returns `this`.
pub fn fn_004311f0(e: &mut Engine, this: Ptr<ExtraType2e>) -> Ptr<ExtraType2e> {
    let s = store(e);
    construct_base(e, this.cast(), TYPE_2E, EXTRA_TYPE_2E_VTABLE);
    s.set(e, this, ExtraType2e::unnamed0C, 0);
    s.set(e, this, ExtraType2e::unnamed10, 0);
    this
}

// Typed from 00431230
/// `ExtraTeleport`'s constructor (the engine map has no name for it):
/// extra-data type 0x2b, taking ownership of `data`. Returns `this`.
pub fn fn_00431230(e: &mut Engine, this: Ptr<ExtraTeleport>, data: Ptr) -> Ptr<ExtraTeleport> {
    let s = store(e);
    construct_base(e, this.cast(), TYPE_TELEPORT, EXTRA_TELEPORT_VTABLE);
    s.set(e, this, ExtraTeleport::pData, data);
    this
}

// Typed from 00431260
/// `ExtraTeleport`'s destructor body (the engine map has no name for it; the
/// scalar deleting destructor is `0042c5b0`, in `the extradatalist unit`): resets
/// the vtable, deletes the owned teleport data if there is any, then runs the
/// base destructor. The exception-unwinding frame is not translated.
pub fn fn_00431260(e: &mut Engine, this: Ptr<ExtraTeleport>) {
    let s = store(e);
    e.mem.set_u32(this.addr(), EXTRA_TELEPORT_VTABLE);
    let data = s.get(e, this, ExtraTeleport::pData);
    if !data.is_null() {
        e.call(TELEPORT_DATA_DELETE, &args![data, 1u32]);
    }
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 004312f0
/// `ExtraTeleport::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraTeleport`, when `BSExtraData::Compare` returns true, or when
/// `DoorTeleportData::Compare` (Xbox PDB) says the two teleport data differ.
pub fn extra_teleport_compare(e: &mut Engine, this: Ptr<ExtraTeleport>, other: Ptr) -> bool {
    let s = store(e);
    let Some(cast) = compare_prologue::<_, ExtraTeleport>(e, this, other, EXTRA_TELEPORT_TYPE)
    else {
        return true;
    };
    let mine = s.get(e, this, ExtraTeleport::pData);
    let theirs = s.get(e, cast, ExtraTeleport::pData);
    e.call(DOOR_TELEPORT_DATA_COMPARE, &args![mine, theirs])
        .bool()
}

// Typed from 00431360
/// `ExtraMapMarker`'s constructor (the engine map has no name for it; the class
/// is the one whose vtable `01015b88` it stores): extra-data type 0x2c, taking
/// ownership of `data`. Returns `this`.
pub fn fn_00431360(e: &mut Engine, this: Ptr<ExtraMapMarker>, data: Ptr) -> Ptr<ExtraMapMarker> {
    let s = store(e);
    construct_base(e, this.cast(), TYPE_MAP_MARKER, EXTRA_MAP_MARKER_VTABLE);
    s.set(e, this, ExtraMapMarker::pMapData, data);
    this
}

// Typed from 00431390
/// `ExtraMapMarker::_scalar_deleting_destructor_` (Xbox PDB): the destructor,
/// then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_map_marker_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraMapMarker>,
    flags: u32,
) -> Ptr<ExtraMapMarker> {
    typed_call!(FAMILY, 0x0043_13c0, fn_004313c0(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 004313c0
/// `ExtraMapMarker`'s destructor body (the engine map has no name for it):
/// resets the vtable, runs the destructor of the owned `MapMarkerData`
/// (`00419350`) if there is one, then the base destructor. The
/// exception-unwinding frame is not translated.
pub fn fn_004313c0(e: &mut Engine, this: Ptr<ExtraMapMarker>) {
    destroy_owner(
        e,
        this.cast(),
        EXTRA_MAP_MARKER_VTABLE,
        MAP_MARKER_DATA_DELETE,
    );
}

// Typed from 00431450
/// `ExtraMapMarker::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraMapMarker`, when `BSExtraData::Compare` returns true, or when
/// `MapMarkerData::Compare` (Xbox PDB) says the two map data differ.
pub fn extra_map_marker_compare(e: &mut Engine, this: Ptr<ExtraMapMarker>, other: Ptr) -> bool {
    let s = store(e);
    let Some(cast) = compare_prologue::<_, ExtraMapMarker>(e, this, other, EXTRA_MAP_MARKER_TYPE)
    else {
        return true;
    };
    let mine = s.get(e, this, ExtraMapMarker::pMapData);
    let theirs = s.get(e, cast, ExtraMapMarker::pMapData);
    e.call(MAP_MARKER_DATA_COMPARE, &args![mine, theirs]).bool()
}

// Typed from 004314c0
/// `ExtraAudioMarker`'s constructor (the engine map has no name for it; the
/// class is the one whose vtable `01015b94` it stores): extra-data type 0x90,
/// taking ownership of `data`. Returns `this`.
pub fn fn_004314c0(
    e: &mut Engine,
    this: Ptr<ExtraAudioMarker>,
    data: Ptr,
) -> Ptr<ExtraAudioMarker> {
    let s = store(e);
    construct_base(e, this.cast(), TYPE_AUDIO_MARKER, EXTRA_AUDIO_MARKER_VTABLE);
    s.set(e, this, ExtraAudioMarker::pAudioData, data);
    this
}

// Typed from 004314f0
/// `ExtraAudioMarker::_scalar_deleting_destructor_` (Xbox PDB): the
/// destructor, then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_audio_marker_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraAudioMarker>,
    flags: u32,
) -> Ptr<ExtraAudioMarker> {
    typed_call!(FAMILY, 0x0043_1520, fn_00431520(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00431520
/// `ExtraAudioMarker`'s destructor body (the engine map has no name for it):
/// resets the vtable, runs the destructor of the owned `AudioMarkerData`
/// (`00419480`) if there is one, then the base destructor. The
/// exception-unwinding frame is not translated.
pub fn fn_00431520(e: &mut Engine, this: Ptr<ExtraAudioMarker>) {
    destroy_owner(
        e,
        this.cast(),
        EXTRA_AUDIO_MARKER_VTABLE,
        AUDIO_MARKER_DATA_DELETE,
    );
}

// Typed from 004315b0
/// `ExtraAudioMarker::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraAudioMarker`, when `BSExtraData::Compare` returns true, or when
/// `AudioMarkerData::Compare` (Xbox PDB) says the two audio data differ.
pub fn extra_audio_marker_compare(e: &mut Engine, this: Ptr<ExtraAudioMarker>, other: Ptr) -> bool {
    let s = store(e);
    let Some(cast) =
        compare_prologue::<_, ExtraAudioMarker>(e, this, other, EXTRA_AUDIO_MARKER_TYPE)
    else {
        return true;
    };
    let mine = s.get(e, this, ExtraAudioMarker::pAudioData);
    let theirs = s.get(e, cast, ExtraAudioMarker::pAudioData);
    e.call(AUDIO_MARKER_DATA_COMPARE, &args![mine, theirs])
        .bool()
}

// Typed from 00431620
/// `ExtraAudioBuoyMarker`'s constructor (the engine map has no name for it;
/// the class is the one whose vtable `01015ba0` it stores): extra-data type
/// 0x91, taking ownership of `data`. Returns `this`.
pub fn fn_00431620(
    e: &mut Engine,
    this: Ptr<ExtraAudioBuoyMarker>,
    data: Ptr,
) -> Ptr<ExtraAudioBuoyMarker> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_AUDIO_BUOY_MARKER,
        EXTRA_AUDIO_BUOY_MARKER_VTABLE,
    );
    s.set(e, this, ExtraAudioBuoyMarker::pAudioBuoyData, data);
    this
}

// Typed from 00431650
/// `ExtraAudioBuoyMarker::_scalar_deleting_destructor_` (Xbox PDB): the
/// destructor, then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_audio_buoy_marker_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraAudioBuoyMarker>,
    flags: u32,
) -> Ptr<ExtraAudioBuoyMarker> {
    typed_call!(FAMILY, 0x0043_1680, fn_00431680(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00431680
/// `ExtraAudioBuoyMarker`'s destructor body (the engine map has no name for
/// it): resets the vtable, runs the destructor of the owned
/// `AudioBuoyMarkerData` (`007b3fa0`) if there is one, then the base
/// destructor. The exception-unwinding frame is not translated.
pub fn fn_00431680(e: &mut Engine, this: Ptr<ExtraAudioBuoyMarker>) {
    destroy_owner(
        e,
        this.cast(),
        EXTRA_AUDIO_BUOY_MARKER_VTABLE,
        AUDIO_BUOY_DATA_DELETE,
    );
}

// Typed from 00431710
/// `ExtraAudioBuoyMarker::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraAudioBuoyMarker`, when `BSExtraData::Compare` returns true, or when
/// the folded body `00401290` (which returns true whatever it is given) says
/// the two buoy data differ; so, past the first two tests, it is always true.
pub fn extra_audio_buoy_marker_compare(
    e: &mut Engine,
    this: Ptr<ExtraAudioBuoyMarker>,
    other: Ptr,
) -> bool {
    let s = store(e);
    let Some(cast) =
        compare_prologue::<_, ExtraAudioBuoyMarker>(e, this, other, EXTRA_AUDIO_BUOY_MARKER_TYPE)
    else {
        return true;
    };
    let mine = s.get(e, this, ExtraAudioBuoyMarker::pAudioBuoyData);
    let theirs = s.get(e, cast, ExtraAudioBuoyMarker::pAudioBuoyData);
    e.call(AUDIO_BUOY_DATA_COMPARE, &args![mine, theirs]).bool()
}

// Typed from 00431780
/// `ExtraAction`'s constructor (the engine map has no name for it; the class is
/// the one whose vtable `01015bac` it stores): extra-data type 0x0e, `eAction`
/// set to 1 (only that byte) and `pActionRef` cleared. Returns `this`.
pub fn fn_00431780(e: &mut Engine, this: Ptr<ExtraAction>) -> Ptr<ExtraAction> {
    let s = store(e);
    construct_base(e, this.cast(), TYPE_ACTION, EXTRA_ACTION_VTABLE);
    e.set(this, ExtraAction::eAction, 1);
    s.set(e, this, ExtraAction::pActionRef, Ptr::NULL);
    this
}

// Typed from 004317c0
/// `ExtraAction::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraAction`, when `BSExtraData::Compare` returns true, or when
/// `eAction` differs.
pub fn extra_action_compare(e: &mut Engine, this: Ptr<ExtraAction>, other: Ptr) -> bool {
    let Some(cast) = compare_prologue::<_, ExtraAction>(e, this, other, EXTRA_ACTION_TYPE) else {
        return true;
    };
    e.get(this, ExtraAction::eAction) != e.get(cast, ExtraAction::eAction)
}

// Typed from 00431830
/// `ExtraContainerChanges`'s constructor (the engine map has no name for it;
/// the class is the one whose vtable `01015bb8` it stores): extra-data type
/// 0x15, taking ownership of `changes`. Returns `this`.
pub fn fn_00431830(
    e: &mut Engine,
    this: Ptr<ExtraContainerChanges>,
    changes: Ptr,
) -> Ptr<ExtraContainerChanges> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_CONTAINER_CHANGES,
        EXTRA_CONTAINER_CHANGES_VTABLE,
    );
    s.set(e, this, ExtraContainerChanges::pChanges, changes);
    this
}

// Typed from 00431860
/// `ExtraContainerChanges::_scalar_deleting_destructor_` (Xbox PDB): the
/// destructor, then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_container_changes_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraContainerChanges>,
    flags: u32,
) -> Ptr<ExtraContainerChanges> {
    typed_call!(FAMILY, 0x0043_1890, fn_00431890(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00431890
/// `ExtraContainerChanges`'s destructor body (the engine map has no name for
/// it): resets the vtable, runs the scalar deleting destructor of the owned
/// `InventoryChanges` (`00431920`, with its flag set to 1) if there is one,
/// then the base destructor. The exception-unwinding frame is not translated.
pub fn fn_00431890(e: &mut Engine, this: Ptr<ExtraContainerChanges>) {
    let s = store(e);
    e.mem.set_u32(this.addr(), EXTRA_CONTAINER_CHANGES_VTABLE);
    let changes = s.get(e, this, ExtraContainerChanges::pChanges);
    if !changes.is_null() {
        typed_call!(FAMILY, 0x0043_1920, fn_00431920(e, changes, 1));
    }
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 00431920
/// Scalar deleting destructor of the object `ExtraContainerChanges::pChanges`
/// points at (an `InventoryChanges` by the Xbox PDB's field type; the engine
/// map has no name for it): its destructor body (`004bf150`), then
/// `operator delete` when `flags & 1`. Returns `this`.
pub fn fn_00431920(e: &mut Engine, this: Ptr, flags: u32) -> Ptr {
    e.call(INVENTORY_CHANGES_DESTRUCT, &args![this]);
    delete_when_asked(e, this, flags);
    this
}

// Typed from 00431950
/// `ExtraOriginalReference`'s constructor (the engine map has no name for it;
/// the class is the one whose vtable `01015bc4` it stores): extra-data type
/// 0x20, storing `reference`. Returns `this`.
pub fn fn_00431950(
    e: &mut Engine,
    this: Ptr<ExtraOriginalReference>,
    reference: Ptr,
) -> Ptr<ExtraOriginalReference> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_ORIGINAL_REFERENCE,
        EXTRA_ORIGINAL_REFERENCE_VTABLE,
    );
    s.set(e, this, ExtraOriginalReference::pReference, reference);
    this
}

// Typed from 00431980
/// `ExtraOriginalReference`'s scalar deleting destructor (the engine map has no
/// name for it): the destructor, then `operator delete` when `flags & 1`.
/// Returns `this`.
pub fn fn_00431980(
    e: &mut Engine,
    this: Ptr<ExtraOriginalReference>,
    flags: u32,
) -> Ptr<ExtraOriginalReference> {
    typed_call!(FAMILY, 0x0043_19b0, fn_004319b0(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 004319b0
/// `ExtraOriginalReference`'s destructor body (the engine map has no name for
/// it): resets the vtable, then runs the base destructor.
pub fn fn_004319b0(e: &mut Engine, this: Ptr<ExtraOriginalReference>) {
    e.mem.set_u32(this.addr(), EXTRA_ORIGINAL_REFERENCE_VTABLE);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 004319d0
/// `ExtraOriginalReference::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraOriginalReference`, when `BSExtraData::Compare` returns true, or when
/// `pReference` differs.
pub fn extra_original_reference_compare(
    e: &mut Engine,
    this: Ptr<ExtraOriginalReference>,
    other: Ptr,
) -> bool {
    let s = store(e);
    let Some(cast) = compare_prologue::<_, ExtraOriginalReference>(
        e,
        this,
        other,
        EXTRA_ORIGINAL_REFERENCE_TYPE,
    ) else {
        return true;
    };
    s.get(e, this, ExtraOriginalReference::pReference)
        != s.get(e, cast, ExtraOriginalReference::pReference)
}

// Typed from 00431a40
/// `ExtraOwnership`'s constructor (the engine map has no name for it; the class
/// is the one whose vtable `010158b4` it stores): extra-data type 0x21,
/// storing `owner`. Returns `this`.
pub fn fn_00431a40(e: &mut Engine, this: Ptr<ExtraOwnership>, owner: Ptr) -> Ptr<ExtraOwnership> {
    let s = store(e);
    construct_base(e, this.cast(), TYPE_OWNERSHIP, EXTRA_OWNERSHIP_VTABLE);
    s.set(e, this, ExtraOwnership::pOwner, owner);
    this
}

// Typed from 00431a70
/// `ExtraOwnership::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraOwnership`, when `BSExtraData::Compare` returns true, or when
/// `pOwner` differs.
pub fn extra_ownership_compare(e: &mut Engine, this: Ptr<ExtraOwnership>, other: Ptr) -> bool {
    let s = store(e);
    let Some(cast) = compare_prologue::<_, ExtraOwnership>(e, this, other, EXTRA_OWNERSHIP_TYPE)
    else {
        return true;
    };
    s.get(e, this, ExtraOwnership::pOwner) != s.get(e, cast, ExtraOwnership::pOwner)
}

// Typed from 00431ae0
/// `ExtraGlobal`'s constructor (the engine map has no name for it; the class is
/// the one whose vtable `010158c0` it stores): extra-data type 0x22, storing
/// `global`. Returns `this`.
pub fn fn_00431ae0(e: &mut Engine, this: Ptr<ExtraGlobal>, global: Ptr) -> Ptr<ExtraGlobal> {
    let s = store(e);
    construct_base(e, this.cast(), TYPE_GLOBAL, EXTRA_GLOBAL_VTABLE);
    s.set(e, this, ExtraGlobal::pGlobal, global);
    this
}

// Typed from 00431b10
/// `ExtraGlobal::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraGlobal`, when `BSExtraData::Compare` returns true, or when `pGlobal`
/// differs.
pub fn extra_global_compare(e: &mut Engine, this: Ptr<ExtraGlobal>, other: Ptr) -> bool {
    let s = store(e);
    let Some(cast) = compare_prologue::<_, ExtraGlobal>(e, this, other, EXTRA_GLOBAL_TYPE) else {
        return true;
    };
    s.get(e, this, ExtraGlobal::pGlobal) != s.get(e, cast, ExtraGlobal::pGlobal)
}

// Typed from 00431b80
/// `ExtraRank`'s constructor (the engine map has no name for it; the class is
/// the one whose vtable `010158cc` it stores): extra-data type 0x23, storing
/// `rank`. Returns `this`.
pub fn fn_00431b80(e: &mut Engine, this: Ptr<ExtraRank>, rank: i32) -> Ptr<ExtraRank> {
    let s = store(e);
    construct_base(e, this.cast(), TYPE_RANK, EXTRA_RANK_VTABLE);
    s.set(e, this, ExtraRank::iRank, rank);
    this
}

// Typed from 00431bb0
/// `ExtraRank::Compare` (Xbox PDB): true when `other` is not an `ExtraRank`,
/// when `BSExtraData::Compare` returns true, or when `iRank` differs.
pub fn extra_rank_compare(e: &mut Engine, this: Ptr<ExtraRank>, other: Ptr) -> bool {
    let s = store(e);
    let Some(cast) = compare_prologue::<_, ExtraRank>(e, this, other, EXTRA_RANK_TYPE) else {
        return true;
    };
    s.get(e, this, ExtraRank::iRank) != s.get(e, cast, ExtraRank::iRank)
}

// Typed from 00431c20
/// `ExtraCount`'s constructor (the engine map has no name for it; the class is
/// the one whose vtable `010158d8` it stores): extra-data type 0x24, storing
/// the 16-bit `count` (the two bytes above it are left alone). Returns `this`.
pub fn fn_00431c20(e: &mut Engine, this: Ptr<ExtraCount>, count: i16) -> Ptr<ExtraCount> {
    construct_base(e, this.cast(), TYPE_COUNT, EXTRA_COUNT_VTABLE);
    e.set(this, ExtraCount::iCount, count);
    this
}

// Typed from 00431c50
/// `ExtraCount::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraCount`, when `BSExtraData::Compare` returns true, or when `iCount`
/// differs.
pub fn extra_count_compare(e: &mut Engine, this: Ptr<ExtraCount>, other: Ptr) -> bool {
    let Some(cast) = compare_prologue::<_, ExtraCount>(e, this, other, EXTRA_COUNT_TYPE) else {
        return true;
    };
    e.get(this, ExtraCount::iCount) != e.get(cast, ExtraCount::iCount)
}

// Typed from 00431cc0
/// `ExtraLeveledItem::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraLeveledItem` or when `BSExtraData::Compare` returns true; the
/// object's own fields (`iIndex`, `bdefault`) are not compared.
pub fn extra_leveled_item_compare(e: &mut Engine, this: Ptr<ExtraLeveledItem>, other: Ptr) -> bool {
    compare_prologue::<_, ExtraLeveledItem>(e, this, other, EXTRA_LEVELED_ITEM_TYPE).is_none()
}

// Typed from 00431d10
/// `ExtraHealth`'s constructor (the engine map has no name for it; the class is
/// the one whose vtable `010158e4` it stores): extra-data type 0x25, storing
/// `health` through x87 FLD/FSTP (signaling NaNs become quiet). Returns `this`.
pub fn fn_00431d10(e: &mut Engine, this: Ptr<ExtraHealth>, health: f32) -> Ptr<ExtraHealth> {
    construct_base(e, this.cast(), TYPE_HEALTH, EXTRA_HEALTH_VTABLE);
    let bits = x87_quiet_bits(health.to_bits());
    e.set(this, ExtraHealth::fHealth, f32::from_bits(bits));
    this
}

// Typed from 00431d40
/// `ExtraHealth::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraHealth`, when `BSExtraData::Compare` returns true, or when `fHealth`
/// differs (as `float`s: a NaN differs from everything, `0.0` equals `-0.0`).
pub fn extra_health_compare(e: &mut Engine, this: Ptr<ExtraHealth>, other: Ptr) -> bool {
    let Some(cast) = compare_prologue::<_, ExtraHealth>(e, this, other, EXTRA_HEALTH_TYPE) else {
        return true;
    };
    e.get(this, ExtraHealth::fHealth) != e.get(cast, ExtraHealth::fHealth)
}

// Typed from 00431db0
/// `ExtraHealthPerc::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraHealthPerc`, when `BSExtraData::Compare` returns true, or when
/// `fHealthPerc` differs (as `float`s: a NaN differs from everything, `0.0`
/// equals `-0.0`).
pub fn extra_health_perc_compare(e: &mut Engine, this: Ptr<ExtraHealthPerc>, other: Ptr) -> bool {
    let Some(cast) = compare_prologue::<_, ExtraHealthPerc>(e, this, other, EXTRA_HEALTH_PERC_TYPE)
    else {
        return true;
    };
    e.get(this, ExtraHealthPerc::fHealthPerc) != e.get(cast, ExtraHealthPerc::fHealthPerc)
}

// Typed from 00431e20
/// `ExtraUses`'s constructor (the engine map has no name for it; the class is
/// the one whose vtable `010158f0` it stores): extra-data type 0x26, storing
/// the byte `uses` (the three bytes above it are left alone). Returns `this`.
pub fn fn_00431e20(e: &mut Engine, this: Ptr<ExtraUses>, uses: u8) -> Ptr<ExtraUses> {
    construct_base(e, this.cast(), TYPE_USES, EXTRA_USES_VTABLE);
    e.set(this, ExtraUses::cUses, uses);
    this
}

// Typed from 00431e50
/// `ExtraUses::Compare` (Xbox PDB): true when `other` is not an `ExtraUses`,
/// when `BSExtraData::Compare` returns true, or when `cUses` differs.
pub fn extra_uses_compare(e: &mut Engine, this: Ptr<ExtraUses>, other: Ptr) -> bool {
    let Some(cast) = compare_prologue::<_, ExtraUses>(e, this, other, EXTRA_USES_TYPE) else {
        return true;
    };
    e.get(this, ExtraUses::cUses) != e.get(cast, ExtraUses::cUses)
}

// Typed from 00431ec0
/// `ExtraTimeLeft`'s constructor (the engine map has no name for it; the class
/// is the one whose vtable `010158fc` it stores): extra-data type 0x27,
/// storing `time`. Returns `this`.
pub fn fn_00431ec0(e: &mut Engine, this: Ptr<ExtraTimeLeft>, time: f32) -> Ptr<ExtraTimeLeft> {
    construct_base(e, this.cast(), TYPE_TIME_LEFT, EXTRA_TIME_LEFT_VTABLE);
    e.set(this, ExtraTimeLeft::fTime, time);
    this
}

// Typed from 00431ef0
/// `ExtraTimeLeft::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraTimeLeft`, when `BSExtraData::Compare` returns true, or when `fTime`
/// differs (as `float`s: a NaN differs from everything, `0.0` equals `-0.0`).
pub fn extra_time_left_compare(e: &mut Engine, this: Ptr<ExtraTimeLeft>, other: Ptr) -> bool {
    let Some(cast) = compare_prologue::<_, ExtraTimeLeft>(e, this, other, EXTRA_TIME_LEFT_TYPE)
    else {
        return true;
    };
    e.get(this, ExtraTimeLeft::fTime) != e.get(cast, ExtraTimeLeft::fTime)
}

// Typed from 00431f60
/// `ExtraCharge`'s constructor (the engine map has no name for it; the class is
/// the one whose vtable `01015908` it stores): extra-data type 0x28, storing
/// `charge`. Returns `this`.
pub fn fn_00431f60(e: &mut Engine, this: Ptr<ExtraCharge>, charge: f32) -> Ptr<ExtraCharge> {
    construct_base(e, this.cast(), TYPE_CHARGE, EXTRA_CHARGE_VTABLE);
    e.set(this, ExtraCharge::fCharge, charge);
    this
}

// Typed from 00431f90
/// `ExtraCharge::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraCharge`, when `BSExtraData::Compare` returns true, or when `fCharge`
/// differs (as `float`s: a NaN differs from everything, `0.0` equals `-0.0`).
pub fn extra_charge_compare(e: &mut Engine, this: Ptr<ExtraCharge>, other: Ptr) -> bool {
    let Some(cast) = compare_prologue::<_, ExtraCharge>(e, this, other, EXTRA_CHARGE_TYPE) else {
        return true;
    };
    e.get(this, ExtraCharge::fCharge) != e.get(cast, ExtraCharge::fCharge)
}

// Typed from 00432000
/// `ExtraScript::ExtraScript` (Xbox PDB): extra-data type 0x0d, storing
/// `script` and no script variables. Returns `this`.
pub fn extra_script_extra_script(
    e: &mut Engine,
    this: Ptr<ExtraScript>,
    script: Ptr,
) -> Ptr<ExtraScript> {
    let s = store(e);
    construct_base(e, this.cast(), TYPE_SCRIPT, EXTRA_SCRIPT_VTABLE);
    s.set(e, this, ExtraScript::pScript, script);
    s.set(e, this, ExtraScript::pScriptVars, Ptr::NULL);
    this
}

// Typed from 00432040
/// `ExtraScript`'s destructor body (the engine map has no name for it): resets
/// the vtable, deletes the owned script variables if there are any and clears
/// the pointer, then runs the base destructor. The exception-unwinding frame
/// is not translated.
pub fn fn_00432040(e: &mut Engine, this: Ptr<ExtraScript>) {
    let s = store(e);
    e.mem.set_u32(this.addr(), EXTRA_SCRIPT_VTABLE);
    let variables = s.get(e, this, ExtraScript::pScriptVars);
    if !variables.is_null() {
        e.call(SCRIPT_LOCALS_DELETE, &args![variables, 1u32]);
    }
    s.set(e, this, ExtraScript::pScriptVars, Ptr::NULL);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 004320d0
/// `ExtraScript::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraScript`, when `BSExtraData::Compare` returns true, or when `pScript`
/// differs.
pub fn extra_script_compare(e: &mut Engine, this: Ptr<ExtraScript>, other: Ptr) -> bool {
    let s = store(e);
    let Some(cast) = compare_prologue::<_, ExtraScript>(e, this, other, EXTRA_SCRIPT_TYPE) else {
        return true;
    };
    s.get(e, this, ExtraScript::pScript) != s.get(e, cast, ExtraScript::pScript)
}

// Typed from 00432140
/// `ExtraWeaponModFlags::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraWeaponModFlags`, when `BSExtraData::Compare` returns true, or when
/// `cWeaponModsActive` differs.
pub fn extra_weapon_mod_flags_compare(
    e: &mut Engine,
    this: Ptr<ExtraWeaponModFlags>,
    other: Ptr,
) -> bool {
    let Some(cast) =
        compare_prologue::<_, ExtraWeaponModFlags>(e, this, other, EXTRA_WEAPON_MOD_FLAGS_TYPE)
    else {
        return true;
    };
    e.get(this, ExtraWeaponModFlags::cWeaponModsActive)
        != e.get(cast, ExtraWeaponModFlags::cWeaponModsActive)
}

// Typed from 004321b0
/// `ExtraModdingItem::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraModdingItem`, when `BSExtraData::Compare` returns true, or when
/// `bIsModding` differs (compared as a byte).
pub fn extra_modding_item_compare(e: &mut Engine, this: Ptr<ExtraModdingItem>, other: Ptr) -> bool {
    let Some(cast) =
        compare_prologue::<_, ExtraModdingItem>(e, this, other, EXTRA_MODDING_ITEM_TYPE)
    else {
        return true;
    };
    e.get(this, ExtraModdingItem::bIsModding) != e.get(cast, ExtraModdingItem::bIsModding)
}

// Typed from 00432220
/// `ExtraScale`'s constructor (the engine map has no name for it; the class is
/// the one whose vtable `01015920` it stores): extra-data type 0x30, storing
/// `scale`. Returns `this`.
pub fn fn_00432220(e: &mut Engine, this: Ptr<ExtraScale>, scale: f32) -> Ptr<ExtraScale> {
    construct_base(e, this.cast(), TYPE_SCALE, EXTRA_SCALE_VTABLE);
    e.set(this, ExtraScale::fScale, scale);
    this
}

// Typed from 00432250
/// `ExtraScale::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraScale`, when `BSExtraData::Compare` returns true, or when `fScale`
/// differs (as `float`s: a NaN differs from everything, `0.0` equals `-0.0`).
pub fn extra_scale_compare(e: &mut Engine, this: Ptr<ExtraScale>, other: Ptr) -> bool {
    let Some(cast) = compare_prologue::<_, ExtraScale>(e, this, other, EXTRA_SCALE_TYPE) else {
        return true;
    };
    e.get(this, ExtraScale::fScale) != e.get(cast, ExtraScale::fScale)
}

// Typed from 004322c0
/// `ExtraGhost`'s constructor (the engine map has no name for it; the class is
/// the one whose vtable `01015bd0` it stores): extra-data type 0x1f, no data
/// of its own. Returns `this`.
pub fn fn_004322c0(e: &mut Engine, this: Ptr<ExtraGhost>) -> Ptr<ExtraGhost> {
    construct_base(e, this.cast(), TYPE_GHOST, EXTRA_GHOST_VTABLE);
    this
}

// Typed from 004322f0
/// `ExtraWorn`'s constructor (the engine map has no name for it; the class is
/// the one whose vtable `01015bdc` it stores): extra-data type 0x16, no data
/// of its own. Returns `this`.
pub fn fn_004322f0(e: &mut Engine, this: Ptr<ExtraWorn>) -> Ptr<ExtraWorn> {
    construct_base(e, this.cast(), TYPE_WORN, EXTRA_WORN_VTABLE);
    this
}

// Typed from 00432320
/// `ExtraWornLeft`'s constructor (the engine map has no name for it; the class
/// is the one whose vtable `01015be8` it stores): extra-data type 0x17, no
/// data of its own. Returns `this`.
pub fn fn_00432320(e: &mut Engine, this: Ptr<ExtraWornLeft>) -> Ptr<ExtraWornLeft> {
    construct_base(e, this.cast(), TYPE_WORN_LEFT, EXTRA_WORN_LEFT_VTABLE);
    this
}

// Typed from 00432350
/// `ExtraCannotWear`'s constructor (the engine map has no name for it; the
/// class is the one whose vtable `01015bf4` it stores): extra-data type 0x3e,
/// no data of its own. Returns `this`.
pub fn fn_00432350(e: &mut Engine, this: Ptr<ExtraCannotWear>) -> Ptr<ExtraCannotWear> {
    construct_base(e, this.cast(), TYPE_CANNOT_WEAR, EXTRA_CANNOT_WEAR_VTABLE);
    this
}

// Typed from 00432380
/// `ExtraHotKey`'s constructor (the engine map has no name for it; the class is
/// the one whose vtable `0101592c` it stores): extra-data type 0x4a, storing
/// the byte `hotkey`. Returns `this`.
pub fn fn_00432380(e: &mut Engine, this: Ptr<ExtraHotKey>, hotkey: i8) -> Ptr<ExtraHotKey> {
    construct_base(e, this.cast(), TYPE_HOT_KEY, EXTRA_HOT_KEY_VTABLE);
    e.set(this, ExtraHotKey::chotkey, hotkey);
    this
}

// Typed from 004323b0
/// `ExtraInfoGeneralTopic::~ExtraInfoGeneralTopic` (Xbox PDB): resets the
/// vtable, clears `MenuTopic::bGeneralTopic` (+0x24) of the owned topic, then
/// runs the topic's scalar deleting destructor (with its flag set to 1) and
/// the base destructor. The byte store comes before the null test, as
/// compiled: with no topic the game would fault, and the memory model
/// panics. The exception-unwinding frame is not translated.
pub fn extra_info_general_topic_destructor(e: &mut Engine, this: Ptr<ExtraInfoGeneralTopic>) {
    let s = store(e);
    e.mem.set_u32(this.addr(), EXTRA_INFO_GENERAL_TOPIC_VTABLE);
    let topic = s.get(e, this, ExtraInfoGeneralTopic::pInfoGen);
    // MenuTopic::bGeneralTopic (Xbox PDB) +0x24
    e.mem.set_u8(topic.addr().wrapping_add(0x24), 0);
    if !topic.is_null() {
        e.call(MENU_TOPIC_DELETE, &args![topic, 1u32]);
    }
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 00432440
/// `ExtraInfoGeneralTopic::_scalar_deleting_destructor_` (Xbox PDB): the
/// destructor, then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_info_general_topic_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraInfoGeneralTopic>,
    flags: u32,
) -> Ptr<ExtraInfoGeneralTopic> {
    typed_call!(
        FAMILY,
        0x0043_23b0,
        extra_info_general_topic_destructor(e, this)
    );
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00432470
/// `ExtraInfoGeneralTopic`'s constructor taking the topic (the engine map has
/// no name for it; Ghidra's library match calls it `CPrintDialog`): extra-data
/// type 0x4d, storing `topic`. Returns `this`.
pub fn fn_00432470(
    e: &mut Engine,
    this: Ptr<ExtraInfoGeneralTopic>,
    topic: Ptr,
) -> Ptr<ExtraInfoGeneralTopic> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_INFO_GENERAL_TOPIC,
        EXTRA_INFO_GENERAL_TOPIC_VTABLE,
    );
    s.set(e, this, ExtraInfoGeneralTopic::pInfoGen, topic);
    this
}

// Typed from 004324a0
/// `ExtraInfoGeneralTopic`'s default constructor (the engine map has no name
/// for it): extra-data type 0x4d and a newly allocated, constructed 0x2c-byte
/// `MenuTopic` (null when the allocation fails). Returns `this`. The
/// exception-unwinding frame is not translated.
pub fn fn_004324a0(e: &mut Engine, this: Ptr<ExtraInfoGeneralTopic>) -> Ptr<ExtraInfoGeneralTopic> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_INFO_GENERAL_TOPIC,
        EXTRA_INFO_GENERAL_TOPIC_VTABLE,
    );
    let memory = e.call(OPERATOR_NEW, &args![0x2cu32]).u32();
    let topic: Ptr = if memory != 0 {
        e.call(MENU_TOPIC_CONSTRUCT, &args![memory]).ptr()
    } else {
        Ptr::NULL
    };
    s.set(e, this, ExtraInfoGeneralTopic::pInfoGen, topic);
    this
}

// Typed from 00432540
/// `ExtraHotKey::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraHotKey`, when `BSExtraData::Compare` returns true, or when
/// `chotkey` differs.
pub fn extra_hot_key_compare(e: &mut Engine, this: Ptr<ExtraHotKey>, other: Ptr) -> bool {
    let Some(cast) = compare_prologue::<_, ExtraHotKey>(e, this, other, EXTRA_HOT_KEY_TYPE) else {
        return true;
    };
    e.get(this, ExtraHotKey::chotkey) != e.get(cast, ExtraHotKey::chotkey)
}

// Typed from 004325b0
/// `ExtraSeed`'s constructor (the engine map has no name for it; the class is
/// the one whose vtable `01015c0c` it stores): extra-data type 0x31, storing
/// the byte `seed`. Returns `this`.
pub fn fn_004325b0(e: &mut Engine, this: Ptr<ExtraSeed>, seed: u8) -> Ptr<ExtraSeed> {
    construct_base(e, this.cast(), TYPE_SEED, EXTRA_SEED_VTABLE);
    e.set(this, ExtraSeed::iSeed, seed);
    this
}

// Typed from 004325e0
/// `ExtraSeed::Compare` (Xbox PDB): true when `other` is not an `ExtraSeed`,
/// when `BSExtraData::Compare` returns true, or when `iSeed` differs.
pub fn extra_seed_compare(e: &mut Engine, this: Ptr<ExtraSeed>, other: Ptr) -> bool {
    let Some(cast) = compare_prologue::<_, ExtraSeed>(e, this, other, EXTRA_SEED_TYPE) else {
        return true;
    };
    e.get(this, ExtraSeed::iSeed) != e.get(cast, ExtraSeed::iSeed)
}

// Typed from 00432650
/// `ExtraPackageStartLocation`'s default constructor (the engine map has no
/// name for it): extra-data type 0x18 and a default-constructed
/// `WORLD_LOCATION`. Returns `this`. The exception-unwinding frame is not
/// translated.
pub fn fn_00432650(
    e: &mut Engine,
    this: Ptr<ExtraPackageStartLocation>,
) -> Ptr<ExtraPackageStartLocation> {
    construct_base(
        e,
        this.cast(),
        TYPE_PACKAGE_START_LOCATION,
        EXTRA_PACKAGE_START_LOCATION_VTABLE,
    );
    let location = this.at(ExtraPackageStartLocation::worldLoc);
    e.call(WORLD_LOCATION_CONSTRUCT, &args![location]);
    this
}

// Typed from 004326c0
/// `ExtraPackageStartLocation`'s constructor from a place (the engine map has
/// no name for it): the default construction, then `pLocationForm` is `form`
/// (or `fallback_form` when `form` is null), `locPt` is a copy of the
/// `NiPoint3` at `position` and `fZRot` is `z_rot`. Returns `this`. The
/// exception-unwinding frame is not translated.
pub fn fn_004326c0(
    e: &mut Engine,
    this: Ptr<ExtraPackageStartLocation>,
    form: Ptr,
    fallback_form: Ptr,
    position: Ptr<NiPoint3>,
    z_rot: f32,
) -> Ptr<ExtraPackageStartLocation> {
    let s = store(e);
    typed_call!(FAMILY, 0x0043_2650, fn_00432650(e, this));
    let location = this.at(ExtraPackageStartLocation::worldLoc);
    let chosen = if form.is_null() { fallback_form } else { form };
    s.set(e, location, WorldLocation::pLocationForm, chosen);
    let point = location.at(WorldLocation::locPt);
    let x = e.get(position, NiPoint3::x);
    let y = e.get(position, NiPoint3::y);
    let z = e.get(position, NiPoint3::z);
    e.set(point, NiPoint3::x, x);
    e.set(point, NiPoint3::y, y);
    e.set(point, NiPoint3::z, z);
    e.set(location, WorldLocation::fZRot, z_rot);
    this
}

// Typed from 00432770
/// `ExtraPackageStartLocation::Compare` (Xbox PDB): true when `other` is not
/// an `ExtraPackageStartLocation`, when `BSExtraData::Compare` returns true,
/// or when the 0x14 bytes of `worldLoc` differ (`memcmp`).
pub fn extra_package_start_location_compare(
    e: &mut Engine,
    this: Ptr<ExtraPackageStartLocation>,
    other: Ptr,
) -> bool {
    let Some(cast) = compare_prologue::<_, ExtraPackageStartLocation>(
        e,
        this,
        other,
        EXTRA_PACKAGE_START_LOCATION_TYPE,
    ) else {
        return true;
    };
    let mine = this.at(ExtraPackageStartLocation::worldLoc);
    let theirs = cast.at(ExtraPackageStartLocation::worldLoc);
    e.call(MEMCMP, &args![mine, theirs, 0x14u32]).u32() != 0
}

// Typed from 004327e0
/// `ExtraReferencePointer`'s constructor (the engine map has no name for it;
/// Ghidra's library match calls it `CPrintDialog`): extra-data type 0x1c,
/// storing `reference`. Returns `this`.
pub fn fn_004327e0(
    e: &mut Engine,
    this: Ptr<ExtraReferencePointer>,
    reference: Ptr,
) -> Ptr<ExtraReferencePointer> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_REFERENCE_POINTER,
        EXTRA_REFERENCE_POINTER_VTABLE,
    );
    s.set(e, this, ExtraReferencePointer::pRef, reference);
    this
}

// Typed from 00432810
/// `ExtraPackage`'s default constructor (the engine map has no name for it):
/// extra-data type 0x19, no package (`pPack` null), `iindex` -1, no target
/// and the three flag bytes clear. Returns `this`.
pub fn fn_00432810(e: &mut Engine, this: Ptr<ExtraPackage>) -> Ptr<ExtraPackage> {
    let s = store(e);
    construct_base(e, this.cast(), TYPE_PACKAGE, EXTRA_PACKAGE_VTABLE);
    s.set(e, this, ExtraPackage::pPack, Ptr::NULL);
    s.set(e, this, ExtraPackage::iindex, -1);
    s.set(e, this, ExtraPackage::pTarg, Ptr::NULL);
    e.set(this, ExtraPackage::bActionComplete, 0);
    e.set(this, ExtraPackage::bActivated, 0);
    e.set(this, ExtraPackage::bDoneOnce, 0);
    this
}

// Typed from 00432870
/// `ExtraPackage`'s constructor from its six fields (the engine map has no
/// name for it): extra-data type 0x19, storing the package, index, target and
/// the low bytes of the three flag arguments. Returns `this`.
#[allow(clippy::too_many_arguments)]
pub fn fn_00432870(
    e: &mut Engine,
    this: Ptr<ExtraPackage>,
    package: Ptr,
    index: i32,
    target: Ptr,
    action_complete: u8,
    activated: u8,
    done_once: u8,
) -> Ptr<ExtraPackage> {
    let s = store(e);
    construct_base(e, this.cast(), TYPE_PACKAGE, EXTRA_PACKAGE_VTABLE);
    s.set(e, this, ExtraPackage::pPack, package);
    s.set(e, this, ExtraPackage::iindex, index);
    s.set(e, this, ExtraPackage::pTarg, target);
    e.set(this, ExtraPackage::bActionComplete, action_complete);
    e.set(this, ExtraPackage::bActivated, activated);
    e.set(this, ExtraPackage::bDoneOnce, done_once);
    this
}

// Typed from 004328d0
/// `ExtraTresPassPackage`'s constructor (the engine map has no name for it;
/// Ghidra's library match calls it `CPrintDialog`): extra-data type 0x1a,
/// storing `package`. Returns `this`.
pub fn fn_004328d0(
    e: &mut Engine,
    this: Ptr<ExtraTresPassPackage>,
    package: Ptr,
) -> Ptr<ExtraTresPassPackage> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_TRES_PASS_PACKAGE,
        EXTRA_TRES_PASS_PACKAGE_VTABLE,
    );
    s.set(e, this, ExtraTresPassPackage::pPack, package);
    this
}

// Typed from 00432900
/// `ExtraTresPassPackage::~ExtraTresPassPackage` (Xbox PDB): resets the
/// vtable; when there is a package, marks it created
/// (`TESPackage::SetIsCreated(true)`), then deletes it: through
/// `TESSaveLoadGame::DeleteForm` when the save/load object's stub
/// (`0047c850`) answers true, else through the form's virtual scalar deleting
/// destructor (slot +0x10, flag 1). The stub always answers false, so the
/// virtual destructor is what runs in the game. Then the base destructor. The
/// exception-unwinding frame is not translated.
pub fn extra_tres_pass_package_destructor(e: &mut Engine, this: Ptr<ExtraTresPassPackage>) {
    let s = store(e);
    e.mem.set_u32(this.addr(), EXTRA_TRES_PASS_PACKAGE_VTABLE);
    let package = s.get(e, this, ExtraTresPassPackage::pPack);
    if !package.is_null() {
        e.call(PACKAGE_SET_IS_CREATED, &args![package, 1u32]);
        let save_load: u32 = e.global(SAVE_LOAD_GAME);
        if e.call(SAVE_LOAD_GAME_ALWAYS_FALSE, &args![save_load])
            .bool()
        {
            let package = s.get(e, this, ExtraTresPassPackage::pPack);
            e.call(SAVE_LOAD_GAME_DELETE_FORM, &args![save_load, package]);
        } else {
            let package = s.get(e, this, ExtraTresPassPackage::pPack);
            if !package.is_null() {
                e.vcall(package.addr(), 0x10, &args![1u32]);
            }
        }
    }
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

/// The start the two `ExtraPlayerCrimeList` constructors share: the base
/// constructor, then a newly allocated, constructed 8-byte list (null when the
/// allocation fails) stored in `pCrime`. `ACTOR_LIST_CONSTRUCT` is the
/// constructor of an empty `BSSimpleList` of any element type.
pub(crate) fn construct_player_crime_list(e: &mut Engine, this: Ptr<ExtraPlayerCrimeList>) {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_PLAYER_CRIME_LIST,
        EXTRA_PLAYER_CRIME_LIST_VTABLE,
    );
    let memory = e.call(OPERATOR_NEW, &args![8u32]).u32();
    let list: Ptr = if memory != 0 {
        e.call(ACTOR_LIST_CONSTRUCT, &args![memory]).ptr()
    } else {
        Ptr::NULL
    };
    s.set(e, this, ExtraPlayerCrimeList::pCrime, list);
}

// Typed from 004329d0
/// `ExtraPlayerCrimeList`'s default constructor (the engine map has no name
/// for it): extra-data type 0x35 and a newly allocated, constructed 8-byte
/// crime list (null when the allocation fails). Returns `this`. The
/// exception-unwinding frame is not translated.
pub fn fn_004329d0(e: &mut Engine, this: Ptr<ExtraPlayerCrimeList>) -> Ptr<ExtraPlayerCrimeList> {
    construct_player_crime_list(e, this);
    this
}

// Typed from 00432a70
/// `ExtraPlayerCrimeList::_scalar_deleting_destructor_` (Xbox PDB): the
/// destructor, then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_player_crime_list_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraPlayerCrimeList>,
    flags: u32,
) -> Ptr<ExtraPlayerCrimeList> {
    typed_call!(FAMILY, 0x0043_2b50, fn_00432b50(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00432aa0
/// `ExtraPlayerCrimeList`'s constructor taking a crime (the engine map has no
/// name for it): the default construction, then the crime is added to the new
/// list (`005ae3d0` takes the address of a variable holding it, and does
/// nothing for a null crime). Returns `this`. The exception-unwinding frame is
/// not translated.
pub fn fn_00432aa0(
    e: &mut Engine,
    this: Ptr<ExtraPlayerCrimeList>,
    crime: Ptr,
) -> Ptr<ExtraPlayerCrimeList> {
    let s = store(e);
    construct_player_crime_list(e, this);
    let list = s.get(e, this, ExtraPlayerCrimeList::pCrime);
    e.with_stack(4, |e, argument| {
        e.mem.set_u32(argument.addr(), crime.addr());
        e.call(CRIME_LIST_ADD, &args![list, argument]);
    });
    this
}

// Typed from 00432b50
/// `ExtraPlayerCrimeList`'s destructor body (the engine map has no name for
/// it): resets the vtable, runs the owned list's scalar deleting destructor
/// (flag 1) when there is one, then the base destructor. The
/// exception-unwinding frame is not translated.
pub fn fn_00432b50(e: &mut Engine, this: Ptr<ExtraPlayerCrimeList>) {
    destroy_owner(
        e,
        this.cast(),
        EXTRA_PLAYER_CRIME_LIST_VTABLE,
        ACTOR_LIST_DELETE,
    );
}

// Typed from 00432be0
/// `ExtraLeveledItem`'s constructor (the engine map has no name for it; Ghidra
/// does not name it): extra-data type 0x2f, storing `index` and setting
/// `bdefault`. Returns `this`.
pub fn fn_00432be0(
    e: &mut Engine,
    this: Ptr<ExtraLeveledItem>,
    index: i32,
) -> Ptr<ExtraLeveledItem> {
    let s = store(e);
    construct_base(e, this.cast(), TYPE_LEVELED_ITEM, EXTRA_LEVELED_ITEM_VTABLE);
    s.set(e, this, ExtraLeveledItem::iIndex, index);
    e.set(this, ExtraLeveledItem::bdefault, true);
    this
}

// Typed from 00432c20
/// `ExtraPersistentCell`'s constructor (the engine map has no name for it):
/// extra-data type 0x0c, storing `cell` (null stays null). Returns `this`.
pub fn fn_00432c20(
    e: &mut Engine,
    this: Ptr<ExtraPersistentCell>,
    cell: Ptr,
) -> Ptr<ExtraPersistentCell> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_PERSISTENT_CELL,
        EXTRA_PERSISTENT_CELL_VTABLE,
    );
    s.set(e, this, ExtraPersistentCell::pPersistentCell, cell);
    this
}

// Typed from 00432c60
/// `ExtraPersistentCell`'s scalar deleting destructor (the engine map has no
/// name for it): the destructor, then `operator delete` when `flags & 1`.
/// Returns `this`.
pub fn fn_00432c60(
    e: &mut Engine,
    this: Ptr<ExtraPersistentCell>,
    flags: u32,
) -> Ptr<ExtraPersistentCell> {
    typed_call!(FAMILY, 0x0043_2c90, fn_00432c90(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00432c90
/// `ExtraPersistentCell`'s destructor body (the engine map has no name for
/// it): resets the vtable, then runs the base destructor; the cell is not
/// owned.
pub fn fn_00432c90(e: &mut Engine, this: Ptr<ExtraPersistentCell>) {
    e.mem.set_u32(this.addr(), EXTRA_PERSISTENT_CELL_VTABLE);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 00432cb0
/// `ExtraRagDollData::ExtraRagDollData` (Xbox PDB): extra-data type 0x14 and
/// no ragdoll data. Returns `this`.
pub fn extra_rag_doll_data_extra_rag_doll_data(
    e: &mut Engine,
    this: Ptr<ExtraRagDollData>,
) -> Ptr<ExtraRagDollData> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_RAG_DOLL_DATA,
        EXTRA_RAG_DOLL_DATA_VTABLE,
    );
    s.set(e, this, ExtraRagDollData::pRagDollData, Ptr::NULL);
    this
}

// Typed from 00432ce0
/// `ExtraRagDollData::_scalar_deleting_destructor_` (Xbox PDB): the
/// destructor, then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_rag_doll_data_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraRagDollData>,
    flags: u32,
) -> Ptr<ExtraRagDollData> {
    typed_call!(FAMILY, 0x0043_2d10, fn_00432d10(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00432d10
/// `ExtraRagDollData`'s destructor body (the engine map has no name for it):
/// resets the vtable, runs the owned `RagDollData`'s scalar deleting
/// destructor (flag 1) when there is one, then the base destructor. The
/// exception-unwinding frame is not translated.
pub fn fn_00432d10(e: &mut Engine, this: Ptr<ExtraRagDollData>) {
    let s = store(e);
    e.mem.set_u32(this.addr(), EXTRA_RAG_DOLL_DATA_VTABLE);
    let data = s.get(e, this, ExtraRagDollData::pRagDollData);
    if !data.is_null() {
        typed_call!(FAMILY, 0x0043_2da0, fn_00432da0(e, data, 1));
    }
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 00432da0
/// `RagDollData`'s scalar deleting destructor (the engine map has no name for
/// it): the destructor body (`004d9380`, in `the ragdolldata unit`), then
/// `operator delete` when `flags & 1`. Returns `this`.
pub fn fn_00432da0(e: &mut Engine, this: Ptr, flags: u32) -> Ptr {
    e.call(RAG_DOLL_DATA_DESTRUCT, &args![this]);
    delete_when_asked(e, this, flags);
    this
}

/// The `__RTDynamicCast` the `Compare` and `Copy` methods start with: `other`
/// as the class with the RTTI descriptor `target_type`, null when it is not
/// one.
pub(crate) fn checked_cast_extra<U>(e: &mut Engine, other: Ptr, target_type: u32) -> Ptr<U> {
    e.call(
        DYNAMIC_CAST,
        &args![other, 0u32, BS_EXTRA_DATA_TYPE, target_type, 0u32],
    )
    .ptr()
}

/// Whether the list node holds no item and has no successor.
pub(crate) fn list_node_is_empty(e: &mut Engine, node: Ptr<BSSimpleList>) -> bool {
    typed_call!(leaf::FAMILY, 0x0082_56d0, leaf::both_words_zero(e, node))
}

/// The item of a list node (the word its item slot holds).
pub(crate) fn list_node_item(e: &mut Engine, node: Ptr<BSSimpleList>) -> u32 {
    let slot = typed_call!(leaf::FAMILY, 0x0068_15c0, leaf::same_word(e, node));
    e.mem.u32(slot)
}

/// The node after `node` (null at the end).
pub(crate) fn list_node_next(e: &mut Engine, node: Ptr<BSSimpleList>) -> Ptr<BSSimpleList> {
    typed_call!(leaf::FAMILY, 0x0072_6070, leaf::word_at_4(e, node))
        .into_ret()
        .ptr()
}

/// The constructor of a class that holds a list of its own at +0xc and
/// nothing else: the base constructor, the vtable, then the list's
/// constructor.
pub(crate) fn construct_with_list(e: &mut Engine, this: Ptr, extra_type: u32, vtable: u32) {
    construct_base(e, this, extra_type, vtable);
    e.call(
        SIMPLE_LIST_CONSTRUCT,
        &args![Ptr::<BSSimpleList>::new(this.addr() + 0xc)],
    );
}

/// The destructor body of the classes that hold a list at +0xc and free its
/// nodes themselves: resets the vtable, runs the list's clear and its
/// destructor body, then the base destructor.
pub(crate) fn destroy_with_list(e: &mut Engine, this: Ptr, vtable: u32) {
    e.mem.set_u32(this.addr(), vtable);
    let list = Ptr::<BSSimpleList>::new(this.addr() + 0xc);
    e.call(SIMPLE_LIST_CLEAR, &args![list]);
    e.call(SIMPLE_LIST_DESTRUCT, &args![list]);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 00432dd0
/// `ExtraRagDollData::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraRagDollData`, when `BSExtraData::Compare` returns true, when exactly
/// one of the two has ragdoll data, or when `RagDollData::Compare` says the
/// data differ.
pub fn extra_rag_doll_data_compare(
    e: &mut Engine,
    this: Ptr<ExtraRagDollData>,
    other: Ptr,
) -> bool {
    let s = store(e);
    let Some(cast) =
        compare_prologue::<_, ExtraRagDollData>(e, this, other, EXTRA_RAG_DOLL_DATA_TYPE)
    else {
        return true;
    };
    let mine = s.get(e, this, ExtraRagDollData::pRagDollData);
    let theirs = s.get(e, cast, ExtraRagDollData::pRagDollData);
    if mine.is_null() {
        return !theirs.is_null();
    }
    e.call(RAG_DOLL_DATA_COMPARE, &args![mine, theirs]).bool()
}

// Typed from 00432e60
/// `ExtraEncounterZone::ExtraEncounterZone` (Xbox PDB): extra-data type 0x74
/// and no zone. Returns `this`.
pub fn extra_encounter_zone_extra_encounter_zone(
    e: &mut Engine,
    this: Ptr<ExtraEncounterZone>,
) -> Ptr<ExtraEncounterZone> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_ENCOUNTER_ZONE,
        EXTRA_ENCOUNTER_ZONE_VTABLE,
    );
    s.set(e, this, ExtraEncounterZone::pZone, Ptr::NULL);
    this
}

// Typed from 00432e90
/// `ExtraEncounterZone::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraEncounterZone`, when `BSExtraData::Compare` returns true, or when
/// the zones differ.
pub fn extra_encounter_zone_compare(
    e: &mut Engine,
    this: Ptr<ExtraEncounterZone>,
    other: Ptr,
) -> bool {
    let s = store(e);
    let Some(cast) =
        compare_prologue::<_, ExtraEncounterZone>(e, this, other, EXTRA_ENCOUNTER_ZONE_TYPE)
    else {
        return true;
    };
    s.get(e, this, ExtraEncounterZone::pZone) != s.get(e, cast, ExtraEncounterZone::pZone)
}

// Typed from 00432f00
/// `ExtraUsedMarkers`' constructor (the engine map has no name for it; the
/// class is the one whose vtable `01015c60` it stores): extra-data type 0x12
/// and no marker used. Returns `this`.
pub fn fn_00432f00(e: &mut Engine, this: Ptr<ExtraUsedMarkers>) -> Ptr<ExtraUsedMarkers> {
    let s = store(e);
    construct_base(e, this.cast(), TYPE_USED_MARKERS, EXTRA_USED_MARKERS_VTABLE);
    s.set(e, this, ExtraUsedMarkers::iUsedMarkers, 0);
    this
}

// Typed from 00432f30
/// A destructor body (the engine map has no name for it) that only runs the
/// base destructor: it does not reset the vtable.
pub fn fn_00432f30(e: &mut Engine, this: Ptr) {
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 00432f50
/// `ExtraReservedMarkers`' constructor (the engine map has no name for it;
/// the class is the one whose vtable `01015c6c` it stores): extra-data type
/// 0x82 and no marker reserved. Returns `this`.
pub fn fn_00432f50(e: &mut Engine, this: Ptr<ExtraReservedMarkers>) -> Ptr<ExtraReservedMarkers> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_RESERVED_MARKERS,
        EXTRA_RESERVED_MARKERS_VTABLE,
    );
    s.set(e, this, ExtraReservedMarkers::iReservedMarkers, 0);
    this
}

// Typed from 00432f80
/// `ExtraUsedMarkers::GetMarkerUsed` (Xbox PDB): whether bit `index` of the
/// used markers is set; false for an index of 30 or more.
pub fn extra_used_markers_get_marker_used(
    e: &mut Engine,
    this: Ptr<ExtraUsedMarkers>,
    index: u32,
) -> bool {
    let s = store(e);
    index < 0x1e && s.get(e, this, ExtraUsedMarkers::iUsedMarkers) & (1 << (index & 0x1f)) != 0
}

// Typed from 00432fc0
/// `ExtraUsedMarkers::SetMarkerUsed` (Xbox PDB): sets bit `index` of the used
/// markers when `used` is not 0, clears it otherwise; nothing happens for an
/// index of 30 or more.
pub fn extra_used_markers_set_marker_used(
    e: &mut Engine,
    this: Ptr<ExtraUsedMarkers>,
    index: u32,
    used: u8,
) {
    let s = store(e);
    if index >= 0x1e {
        return;
    }
    let bit = 1u32 << (index & 0x1f);
    let markers = s.get(e, this, ExtraUsedMarkers::iUsedMarkers);
    let markers = if used != 0 {
        markers | bit
    } else {
        markers & !bit
    };
    s.set(e, this, ExtraUsedMarkers::iUsedMarkers, markers);
}

// Typed from 00433010
/// `ExtraRunOncePacks`' constructor (the engine map has no name for it):
/// extra-data type 0x1b and a newly allocated, constructed 8-byte list (null
/// when the allocation fails). Returns `this`. The exception-unwinding frame
/// is not translated.
pub fn fn_00433010(e: &mut Engine, this: Ptr<ExtraRunOncePacks>) -> Ptr<ExtraRunOncePacks> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_RUN_ONCE_PACKS,
        EXTRA_RUN_ONCE_PACKS_VTABLE,
    );
    let memory = e.call(OPERATOR_NEW, &args![8u32]).u32();
    let list: Ptr = if memory != 0 {
        e.call(SIMPLE_LIST_CONSTRUCT, &args![memory]).ptr()
    } else {
        Ptr::NULL
    };
    s.set(e, this, ExtraRunOncePacks::pPackageList, list);
    this
}

// Typed from 004330b0
/// `ExtraRunOncePacks::_scalar_deleting_destructor_` (Xbox PDB): the
/// destructor, then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_run_once_packs_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraRunOncePacks>,
    flags: u32,
) -> Ptr<ExtraRunOncePacks> {
    typed_call!(FAMILY, 0x0043_30e0, fn_004330e0(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 004330e0
/// `ExtraRunOncePacks`' destructor body (the engine map has no name for it):
/// resets the vtable, deletes the item of every node up to the first node
/// without an item, clears the list (called even when the pointer is null),
/// deletes it if there is one, then runs the base destructor. The
/// exception-unwinding frame is not translated.
pub fn fn_004330e0(e: &mut Engine, this: Ptr<ExtraRunOncePacks>) {
    let s = store(e);
    e.mem.set_u32(this.addr(), EXTRA_RUN_ONCE_PACKS_VTABLE);
    let mut node = e
        .get(this, ExtraRunOncePacks::pPackageList)
        .cast::<BSSimpleList>();
    while !node.is_null() {
        if list_node_item(e, node) == 0 {
            break;
        }
        let item = list_node_item(e, node);
        e.call(OPERATOR_DELETE, &args![item]);
        node = list_node_next(e, node);
    }
    let list = s.get(e, this, ExtraRunOncePacks::pPackageList);
    e.call(SIMPLE_LIST_CLEAR, &args![list]);
    let list = s.get(e, this, ExtraRunOncePacks::pPackageList);
    if !list.is_null() {
        e.call(SIMPLE_LIST_DELETE, &args![list, 1u32]);
    }
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 004331c0
/// A method of `ExtraRunOncePacks` (the engine map has no name for it): sets
/// the flag byte of the 8-byte entry whose first word is `key`. The entry is
/// searched in the package list (the search ends at the first node without
/// an item); a missing one is allocated and added to the list first. The
/// entry's first word is then `key` and its byte at +4 `flag`.
pub fn fn_004331c0(e: &mut Engine, this: Ptr<ExtraRunOncePacks>, key: u32, flag: u8) {
    let s = store(e);
    let mut found = 0u32;
    let mut node = e
        .get(this, ExtraRunOncePacks::pPackageList)
        .cast::<BSSimpleList>();
    while !node.is_null() {
        let item = list_node_item(e, node);
        if item == 0 {
            node = Ptr::NULL;
        } else if e.mem.u32(item) == key {
            found = item;
            break;
        } else {
            node = list_node_next(e, node);
        }
    }
    if found == 0 {
        let entry = e.call(OPERATOR_NEW, &args![8u32]).u32();
        let list = s.get(e, this, ExtraRunOncePacks::pPackageList);
        // The list is given the address of the variable that holds the new
        // entry; the entry is read back from it afterwards.
        found = e.with_stack(4, |e, local| {
            e.mem.set_u32(local.addr(), entry);
            e.call(SIMPLE_LIST_ADD_HEAD, &args![list, local]);
            e.mem.u32(local.addr())
        });
    }
    e.mem.set_u32(found, key);
    e.mem.set_u8(found + 4, flag);
}

// Typed from 00433260
/// `ExtraDistantData`'s constructor (the engine map has no name for it; the
/// class is the one whose vtable `01015c84` it stores): extra-data type 0x13
/// and the land normal `(0, 0, z)` with `z` the float constant at `010145a8`
/// (built by `NiPoint3`'s constructor in a temporary, then copied). Returns
/// `this`. The exception-unwinding frame is not translated.
pub fn fn_00433260(e: &mut Engine, this: Ptr<ExtraDistantData>) -> Ptr<ExtraDistantData> {
    construct_base(e, this.cast(), TYPE_DISTANT_DATA, EXTRA_DISTANT_DATA_VTABLE);
    let normal = this.at(ExtraDistantData::LandNormal);
    // The call on the member's address does nothing (it returns its argument).
    typed_call!(leaf::FAMILY, 0x0068_15c0, leaf::same_word(e, normal));
    let z: f32 = e.global(DISTANT_DATA_NORMAL_Z);
    let temporary = e.with_stack(0xc, |e, point| {
        let made = typed_call!(
            leaf::FAMILY,
            0x0041_6870,
            leaf::point3_new(e, point, 0.0f32, 0.0f32, z)
        );
        [e.mem.u32(made), e.mem.u32(made + 4), e.mem.u32(made + 8)]
    });
    for (i, word) in (0u32..).zip(temporary.iter()) {
        e.mem.set_u32(normal.addr() + 4 * i, *word);
    }
    this
}

// Typed from 00433300
/// `ExtraEnableStateParent`'s constructor (the engine map has no name for
/// it): extra-data type 0x37, no parent and no flags. Returns `this`.
pub fn fn_00433300(
    e: &mut Engine,
    this: Ptr<ExtraEnableStateParent>,
) -> Ptr<ExtraEnableStateParent> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_ENABLE_STATE_PARENT,
        EXTRA_ENABLE_STATE_PARENT_VTABLE,
    );
    s.set(e, this, ExtraEnableStateParent::pParent, Ptr::NULL);
    e.set(this, ExtraEnableStateParent::cFlags, 0);
    this
}

// Typed from 00433340
/// `ExtraEnableStateParent::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraEnableStateParent`, or when the parent or the flags differ.
/// (`BSExtraData::Compare` is not asked.)
pub fn extra_enable_state_parent_compare(
    e: &mut Engine,
    this: Ptr<ExtraEnableStateParent>,
    other: Ptr,
) -> bool {
    let s = store(e);
    let cast: Ptr<ExtraEnableStateParent> =
        checked_cast_extra(e, other, EXTRA_ENABLE_STATE_PARENT_TYPE);
    if cast.is_null() {
        return true;
    }
    s.get(e, cast, ExtraEnableStateParent::pParent)
        != s.get(e, this, ExtraEnableStateParent::pParent)
        || e.get(cast, ExtraEnableStateParent::cFlags)
            != e.get(this, ExtraEnableStateParent::cFlags)
}

// Typed from 004333a0
/// `ExtraEnableStateChildren`'s constructor (the engine map has no name for
/// it): extra-data type 0x38 and an empty child list. Returns `this`. The
/// exception-unwinding frame is not translated.
pub fn fn_004333a0(
    e: &mut Engine,
    this: Ptr<ExtraEnableStateChildren>,
) -> Ptr<ExtraEnableStateChildren> {
    construct_with_list(
        e,
        this.cast(),
        TYPE_ENABLE_STATE_CHILDREN,
        EXTRA_ENABLE_STATE_CHILDREN_VTABLE,
    );
    this
}

// Typed from 00433410
/// `ExtraEnableStateChildren::_scalar_deleting_destructor_` (Xbox PDB): the
/// destructor, then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_enable_state_children_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraEnableStateChildren>,
    flags: u32,
) -> Ptr<ExtraEnableStateChildren> {
    typed_call!(FAMILY, 0x0043_3440, fn_00433440(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00433440
/// `ExtraEnableStateChildren`'s destructor body (the decompiler names it
/// `CMFCRibbonInfo::XQAT::~XQAT` by mistake; the body is this class's):
/// resets the vtable, clears the child list, runs the list's destructor body,
/// then the base destructor. The exception-unwinding frame is not translated.
pub fn fn_00433440(e: &mut Engine, this: Ptr<ExtraEnableStateChildren>) {
    destroy_with_list(e, this.cast(), EXTRA_ENABLE_STATE_CHILDREN_VTABLE);
}

// Typed from 004334b0
/// `ExtraRandomTeleportMarker`'s constructor (the engine map has no name for
/// it): extra-data type 0x3b and no marker. Returns `this`.
pub fn fn_004334b0(
    e: &mut Engine,
    this: Ptr<ExtraRandomTeleportMarker>,
) -> Ptr<ExtraRandomTeleportMarker> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_RANDOM_TELEPORT_MARKER,
        EXTRA_RANDOM_TELEPORT_MARKER_VTABLE,
    );
    s.set(e, this, ExtraRandomTeleportMarker::pMarker, Ptr::NULL);
    this
}

// Typed from 004334e0
/// `ExtraRandomTeleportMarker::Compare` (Xbox PDB): true when `other` is not
/// an `ExtraRandomTeleportMarker` or when the markers differ.
/// (`BSExtraData::Compare` is not asked.)
pub fn extra_random_teleport_marker_compare(
    e: &mut Engine,
    this: Ptr<ExtraRandomTeleportMarker>,
    other: Ptr,
) -> bool {
    let s = store(e);
    let cast: Ptr<ExtraRandomTeleportMarker> =
        checked_cast_extra(e, other, EXTRA_RANDOM_TELEPORT_MARKER_TYPE);
    if cast.is_null() {
        return true;
    }
    s.get(e, cast, ExtraRandomTeleportMarker::pMarker)
        != s.get(e, this, ExtraRandomTeleportMarker::pMarker)
}

// Typed from 00433530
/// `ExtraLinkedRefChildren`'s constructor (the engine map has no name for
/// it): extra-data type 0x52 and an empty child list. Returns `this`. The
/// exception-unwinding frame is not translated.
pub fn fn_00433530(
    e: &mut Engine,
    this: Ptr<ExtraLinkedRefChildren>,
) -> Ptr<ExtraLinkedRefChildren> {
    construct_with_list(
        e,
        this.cast(),
        TYPE_LINKED_REF_CHILDREN,
        EXTRA_LINKED_REF_CHILDREN_VTABLE,
    );
    this
}

// Typed from 004335a0
/// `ExtraLinkedRefChildren::_scalar_deleting_destructor_` (Xbox PDB): the
/// destructor, then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_linked_ref_children_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraLinkedRefChildren>,
    flags: u32,
) -> Ptr<ExtraLinkedRefChildren> {
    typed_call!(FAMILY, 0x0043_35d0, fn_004335d0(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 004335d0
/// `ExtraLinkedRefChildren`'s destructor body (the decompiler names it
/// `CMFCRibbonInfo::XQAT::~XQAT` by mistake; the body is this class's):
/// resets the vtable, clears the child list, runs the list's destructor body,
/// then the base destructor. The exception-unwinding frame is not translated.
pub fn fn_004335d0(e: &mut Engine, this: Ptr<ExtraLinkedRefChildren>) {
    destroy_with_list(e, this.cast(), EXTRA_LINKED_REF_CHILDREN_VTABLE);
}

// Typed from 00433640
/// `ExtraLinkedRef`'s constructor (the engine map has no name for it):
/// extra-data type 0x51 and no linked reference. Returns `this`.
pub fn fn_00433640(e: &mut Engine, this: Ptr<ExtraLinkedRef>) -> Ptr<ExtraLinkedRef> {
    let s = store(e);
    construct_base(e, this.cast(), TYPE_LINKED_REF, EXTRA_LINKED_REF_VTABLE);
    s.set(e, this, ExtraLinkedRef::pLinkedRef, Ptr::NULL);
    this
}

// Typed from 00433670
/// `ExtraLinkedRef::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraLinkedRef` or when the linked references differ.
/// (`BSExtraData::Compare` is not asked.)
pub fn extra_linked_ref_compare(e: &mut Engine, this: Ptr<ExtraLinkedRef>, other: Ptr) -> bool {
    let s = store(e);
    let cast: Ptr<ExtraLinkedRef> = checked_cast_extra(e, other, EXTRA_LINKED_REF_TYPE);
    if cast.is_null() {
        return true;
    }
    s.get(e, cast, ExtraLinkedRef::pLinkedRef) != s.get(e, this, ExtraLinkedRef::pLinkedRef)
}

// Typed from 004336c0
/// `ExtraAshPileRef`'s constructor (the engine map has no name for it):
/// extra-data type 0x89 and no reference. Returns `this`.
pub fn fn_004336c0(e: &mut Engine, this: Ptr<ExtraAshPileRef>) -> Ptr<ExtraAshPileRef> {
    let s = store(e);
    construct_base(e, this.cast(), TYPE_ASH_PILE_REF, EXTRA_ASH_PILE_REF_VTABLE);
    s.set(e, this, ExtraAshPileRef::pAshPileRef, Ptr::NULL);
    this
}

// Typed from 004336f0
/// A cdecl search in a list of `REF_ACTIVATE_DATA` pointers (the engine map
/// has no name for it): the first item whose `pActivateRef` is `reference`,
/// null if there is none. Stops at the first empty node.
pub fn fn_004336f0(
    e: &mut Engine,
    reference: u32,
    list: Ptr<BSSimpleList>,
) -> Ptr<RefActivateData> {
    let mut node = list;
    while !node.is_null() && !list_node_is_empty(e, node) {
        let item = list_node_item(e, node);
        if e.mem.u32(item) == reference {
            return Ptr::new(item);
        }
        node = list_node_next(e, node);
    }
    Ptr::NULL
}

// Typed from 00433750
/// A cdecl function (the engine map has no name for it): deletes the item of
/// the list's head and removes the head, until the list is empty.
pub fn fn_00433750(e: &mut Engine, list: Ptr<BSSimpleList>) {
    while !list.is_null() && !list_node_is_empty(e, list) {
        let item = list_node_item(e, list);
        e.call(OPERATOR_DELETE, &args![item]);
        e.call(SIMPLE_LIST_REMOVE_HEAD, &args![list]);
    }
}

// Typed from 00433790
/// `ExtraActivateRefChildren`'s constructor (the engine map has no name for
/// it): extra-data type 0x54, an empty child list and a timer of 0. Returns
/// `this`. The exception-unwinding frame is not translated.
pub fn fn_00433790(
    e: &mut Engine,
    this: Ptr<ExtraActivateRefChildren>,
) -> Ptr<ExtraActivateRefChildren> {
    construct_with_list(
        e,
        this.cast(),
        TYPE_ACTIVATE_REF_CHILDREN,
        EXTRA_ACTIVATE_REF_CHILDREN_VTABLE,
    );
    e.set(this, ExtraActivateRefChildren::fActivateChildrenTimer, 0.0);
    this
}

// Typed from 00433800
/// `ExtraActivateRefChildren::_scalar_deleting_destructor_` (Xbox PDB): the
/// destructor, then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_activate_ref_children_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraActivateRefChildren>,
    flags: u32,
) -> Ptr<ExtraActivateRefChildren> {
    typed_call!(FAMILY, 0x0043_3830, fn_00433830(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00433830
/// `ExtraActivateRefChildren`'s destructor body (the engine map has no name
/// for it): resets the vtable, deletes the child list's items and nodes
/// (`00433750`), runs the list's destructor body, then the base destructor.
/// The exception-unwinding frame is not translated.
pub fn fn_00433830(e: &mut Engine, this: Ptr<ExtraActivateRefChildren>) {
    e.mem
        .set_u32(this.addr(), EXTRA_ACTIVATE_REF_CHILDREN_VTABLE);
    let list = this.at(ExtraActivateRefChildren::ChildList);
    typed_call!(FAMILY, 0x0043_3750, fn_00433750(e, list));
    e.call(SIMPLE_LIST_DESTRUCT, &args![list]);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 004338b0
/// `ExtraActivateRef`'s constructor (the engine map has no name for it):
/// extra-data type 0x53, an empty parent list, no flags, and an
/// `ActivateTextOverride` string that is built and then `Set` to the empty string
/// at `01011584` (an empty C string; length 0 means all of it). Returns `this`. The
/// exception-unwinding frame is not translated.
pub fn fn_004338b0(e: &mut Engine, this: Ptr<ExtraActivateRef>) -> Ptr<ExtraActivateRef> {
    construct_with_list(e, this.cast(), TYPE_ACTIVATE_REF, EXTRA_ACTIVATE_REF_VTABLE);
    let text = this.at(ExtraActivateRef::ActivateTextOverride);
    e.call(BS_STRING_CONSTRUCT, &args![text]);
    e.set(this, ExtraActivateRef::cActivateFlags, 0);
    e.call(BS_STRING_SET, &args![text, EMPTY_STRING, 0u32]);
    this
}

// Typed from 00433940
/// `ExtraActivateRef::_scalar_deleting_destructor_` (Xbox PDB): the
/// destructor, then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_activate_ref_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraActivateRef>,
    flags: u32,
) -> Ptr<ExtraActivateRef> {
    typed_call!(FAMILY, 0x0043_3970, fn_00433970(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00433970
/// `ExtraActivateRef`'s destructor body (the engine map has no name for it):
/// resets the vtable, deletes the parent list's items and nodes (`00433750`),
/// clears the `ActivateTextOverride` string, runs the list's destructor body,
/// then the base destructor. The exception-unwinding frame is not translated.
pub fn fn_00433970(e: &mut Engine, this: Ptr<ExtraActivateRef>) {
    e.mem.set_u32(this.addr(), EXTRA_ACTIVATE_REF_VTABLE);
    let list = this.at(ExtraActivateRef::ParentList);
    typed_call!(FAMILY, 0x0043_3750, fn_00433750(e, list));
    e.call(
        BS_STRING_CLEAR,
        &args![this.at(ExtraActivateRef::ActivateTextOverride)],
    );
    e.call(SIMPLE_LIST_DESTRUCT, &args![list]);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 00433a00
/// A method of `ExtraActivateRef` (the engine map has no name for it): the
/// entry of the parent list for `reference` (`004336f0`), null if there is
/// none.
pub fn fn_00433a00(
    e: &mut Engine,
    this: Ptr<ExtraActivateRef>,
    reference: u32,
) -> Ptr<RefActivateData> {
    typed_call!(
        FAMILY,
        0x0043_36f0,
        fn_004336f0(e, reference, this.at(ExtraActivateRef::ParentList))
    )
}

// Typed from 00433a20
/// `ExtraActivateRef::Compare` (Xbox PDB): true when `other` is not an
/// `ExtraActivateRef`, when the parent lists have a different number of
/// entries, when an entry of this list has no entry for its reference in the
/// other list or differs from it in its 8 bytes, when the flags differ, or
/// when the activate texts differ (an empty text equals only an empty text,
/// otherwise the strings are compared with `strcmp`). `BSExtraData::Compare`
/// is not asked.
pub fn extra_activate_ref_compare(e: &mut Engine, this: Ptr<ExtraActivateRef>, other: Ptr) -> bool {
    let cast: Ptr<ExtraActivateRef> = checked_cast_extra(e, other, EXTRA_ACTIVATE_REF_TYPE);
    if cast.is_null() {
        return true;
    }
    let mine = this.at(ExtraActivateRef::ParentList);
    let theirs = cast.at(ExtraActivateRef::ParentList);
    let my_count = e.call(SIMPLE_LIST_COUNT, &args![mine]).u32();
    let their_count = e.call(SIMPLE_LIST_COUNT, &args![theirs]).u32();
    if my_count != their_count {
        return true;
    }
    let mut node = mine;
    while !node.is_null() && !list_node_is_empty(e, node) {
        let item = list_node_item(e, node);
        let reference = e.mem.u32(item);
        let counterpart = typed_call!(FAMILY, 0x0043_3a00, fn_00433a00(e, cast, reference));
        if counterpart.is_null() || e.call(MEMCMP, &args![counterpart, item, 8u32]).u32() != 0 {
            return true;
        }
        node = list_node_next(e, node);
    }
    if e.get(this, ExtraActivateRef::cActivateFlags)
        != e.get(cast, ExtraActivateRef::cActivateFlags)
    {
        return true;
    }
    let my_text = this.at(ExtraActivateRef::ActivateTextOverride);
    let their_text = cast.at(ExtraActivateRef::ActivateTextOverride);
    if e.call(BS_STRING_LENGTH, &args![my_text]).u32() == 0
        && e.call(BS_STRING_LENGTH, &args![their_text]).u32() == 0
    {
        return false;
    }
    if e.call(BS_STRING_LENGTH, &args![my_text]).u32() == 0
        || e.call(BS_STRING_LENGTH, &args![their_text]).u32() == 0
    {
        return true;
    }
    let their_chars = typed_call!(leaf::FAMILY, 0x0055_9450, leaf::word_at_0(e, their_text));
    let my_chars = typed_call!(leaf::FAMILY, 0x0055_9450, leaf::word_at_0(e, my_text));
    e.call(STRCMP, &args![my_chars, their_chars]).u32() != 0
}

// Typed from 00433b70
/// `ExtraActivateRef::Copy` (Xbox PDB): when `other` is an `ExtraActivateRef`,
/// deletes this parent list's entries, copies each of the other list's
/// 8-byte entries into a new one added to the list (at the head, so the order
/// is reversed), and copies the flags and the activate text (`BSStringT::Set`).
/// Nothing happens when `other` is another class. The exception-unwinding
/// frame is not translated.
pub fn extra_activate_ref_copy(e: &mut Engine, this: Ptr<ExtraActivateRef>, other: Ptr) {
    let cast: Ptr<ExtraActivateRef> = checked_cast_extra(e, other, EXTRA_ACTIVATE_REF_TYPE);
    if cast.is_null() {
        return;
    }
    let list = this.at(ExtraActivateRef::ParentList);
    typed_call!(FAMILY, 0x0043_3750, fn_00433750(e, list));
    let mut node = cast.at(ExtraActivateRef::ParentList);
    while !node.is_null() && !list_node_is_empty(e, node) {
        let item = list_node_item(e, node);
        node = list_node_next(e, node);
        let memory = e.call(OPERATOR_NEW, &args![8u32]).u32();
        let copy = if memory != 0 {
            e.call(REF_ACTIVATE_DATA_CONSTRUCT, &args![memory]).u32()
        } else {
            0
        };
        e.call(MEMCPY, &args![copy, item, 8u32]);
        e.with_stack(4, |e, local| {
            e.mem.set_u32(local.addr(), copy);
            e.call(SIMPLE_LIST_ADD_HEAD, &args![list, local]);
        });
    }
    let flags = e.get(cast, ExtraActivateRef::cActivateFlags);
    e.set(this, ExtraActivateRef::cActivateFlags, flags);
    e.call(
        BS_STRING_COPY,
        &args![
            this.at(ExtraActivateRef::ActivateTextOverride),
            cast.at(ExtraActivateRef::ActivateTextOverride)
        ],
    );
}

// Typed from 00433ca0
/// `ExtraDecalRefs`' constructor (the engine map has no name for it):
/// extra-data type 0x57 and an empty decal list. Returns `this`. The
/// exception-unwinding frame is not translated.
pub fn fn_00433ca0(e: &mut Engine, this: Ptr<ExtraDecalRefs>) -> Ptr<ExtraDecalRefs> {
    construct_with_list(e, this.cast(), TYPE_DECAL_REFS, EXTRA_DECAL_REFS_VTABLE);
    this
}

// Typed from 00433d10
/// `ExtraDecalRefs::_scalar_deleting_destructor_` (Xbox PDB): the
/// destructor, then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_decal_refs_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraDecalRefs>,
    flags: u32,
) -> Ptr<ExtraDecalRefs> {
    typed_call!(FAMILY, 0x0043_3d40, fn_00433d40(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 00433d40
/// `ExtraDecalRefs`' destructor body (the decompiler names it
/// `CMFCFilterChunkValueImpl::~CMFCFilterChunkValueImpl` by mistake; the body
/// is this class's): resets the vtable, frees the decal list's entries
/// (`004343c0`), runs the list's destructor body, then the base destructor.
/// The exception-unwinding frame is not translated.
pub fn fn_00433d40(e: &mut Engine, this: Ptr<ExtraDecalRefs>) {
    e.mem.set_u32(this.addr(), EXTRA_DECAL_REFS_VTABLE);
    typed_call!(FAMILY, 0x0043_43c0, fn_004343c0(e, this));
    e.call(
        SIMPLE_LIST_DESTRUCT,
        &args![this.at(ExtraDecalRefs::DecalRefList)],
    );
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

/// The frame helper of the three `InitItem`s: asks the owner's file for its
/// compile index, adds it to the form id held at `id_slot` (a word on the
/// game's stack, modified in place), looks the form up and casts it to a
/// `TESObjectREFR`. Returns the reference, or null when there is none.
pub(crate) fn resolve_reference(e: &mut Engine, owner: Ptr, id_slot: Ptr) -> Ptr {
    let file = e.call(FORM_GET_FILE, &args![owner, 0xffff_ffffu32]).u32();
    e.call(FORM_ADD_COMPILE_INDEX, &args![id_slot, file]);
    let id = e.mem.u32(id_slot.addr());
    let form = e.call(LOOKUP_FORM_BY_ID, &args![id]).u32();
    e.call(
        DYNAMIC_CAST,
        &args![form, 0u32, TES_FORM_TYPE, TES_OBJECT_REFR_TYPE, 0u32],
    )
    .ptr()
}

/// Drops the entry of a list that an `InitItem` could not resolve: the head
/// is removed (remove-head, `0063f7b0`) when there is no previous node, otherwise the
/// entry (the pointer held at `item_slot`) is removed after `previous`, and
/// the walk continues at the node after `previous`. The entry is then
/// deleted. Returns the node the walk goes on with.
pub(crate) fn drop_unresolved_entry(
    e: &mut Engine,
    node: Ptr<BSSimpleList>,
    previous: Ptr<BSSimpleList>,
    item_slot: Ptr,
) -> Ptr<BSSimpleList> {
    let next = if previous.is_null() {
        e.call(SIMPLE_LIST_REMOVE_HEAD, &args![node]);
        node
    } else {
        e.call(SIMPLE_LIST_REMOVE_ITEM, &args![previous, item_slot]);
        list_node_next(e, previous)
    };
    let item = e.mem.u32(item_slot.addr());
    e.call(OPERATOR_DELETE, &args![item]);
    e.mem.set_u32(item_slot.addr(), 0);
    next
}

// Typed from 00433db0
/// `ExtraDecalRefs::InitItem` (engine map name, not in the Xbox PDB; `this`,
/// then the reference the extra data belongs to, whose file is asked for the
/// compile index): resolves the form id of every decal entry to the
/// reference. An entry whose form cannot be found (not a `TESObjectREFR`) is
/// logged ("MASTERFILE: Unable to find decal reference %08X."), removed from
/// the list and deleted; the others have their first word replaced by the
/// reference pointer. The exception-unwinding frame is not translated.
pub fn extra_decal_refs_init_item(e: &mut Engine, this: Ptr<ExtraDecalRefs>, owner: Ptr) {
    e.with_stack(8, |e, frame| {
        // `frame` holds the entry pointer (its address goes to `00905330`),
        // `frame + 4` the form id (its address goes to `AddCompileIndex`).
        let item_slot = frame;
        let id_slot = frame.byte_add(4);
        let mut node = this.at(ExtraDecalRefs::DecalRefList);
        let mut previous: Ptr<BSSimpleList> = Ptr::NULL;
        while !node.is_null() && !list_node_is_empty(e, node) {
            let mut removed = false;
            let item = list_node_item(e, node);
            e.mem.set_u32(item_slot.addr(), item);
            let id = e.mem.u32(item);
            e.mem.set_u32(id_slot.addr(), id);
            let reference = resolve_reference(e, owner, id_slot);
            if reference.is_null() {
                let id = e.mem.u32(id_slot.addr());
                e.call(MASTERFILE_LOG, &args![MESSAGE_DECAL_NOT_FOUND, id]);
                node = drop_unresolved_entry(e, node, previous, item_slot);
                removed = true;
            } else {
                e.mem.set_u32(item, reference.addr());
            }
            if !removed {
                previous = node;
                node = list_node_next(e, node);
            }
        }
    });
}

// Typed from 00433ed0
/// `ExtraReflectorRefs::InitItem` (engine map name, not in the Xbox PDB;
/// `this`, then the reference the extra data belongs to): resolves the form
/// id of every entry to a persistent reference. An entry whose form is not a
/// reference, or whose reference is not persistent, is logged ("Unable to find
/// water reflector reference" / "is not persistent so cannot reflect or
/// refract other references"), removed and deleted. For the others, the
/// reference's extra-data list is told about the owner once for each of the
/// entry's effect flags (bit 0: `0041f4c0`, bit 1: `0041f650`), and the first
/// word of the entry becomes the reference pointer. The exception-unwinding
/// frame is not translated.
pub fn extra_reflector_refs_init_item(e: &mut Engine, this: Ptr<ExtraReflectorRefs>, owner: Ptr) {
    e.with_stack(8, |e, frame| {
        let item_slot = frame;
        let id_slot = frame.byte_add(4);
        let mut node = this.at(ExtraReflectorRefs::RefList);
        let mut previous: Ptr<BSSimpleList> = Ptr::NULL;
        while !node.is_null() && !list_node_is_empty(e, node) {
            let mut removed = false;
            let item = list_node_item(e, node);
            e.mem.set_u32(item_slot.addr(), item);
            let id = e.mem.u32(item);
            e.mem.set_u32(id_slot.addr(), id);
            let reference = resolve_reference(e, owner, id_slot);
            if reference.is_null() || !e.call(REFR_GET_REF_PERSISTS, &args![reference]).bool() {
                let id = e.mem.u32(id_slot.addr());
                if reference.is_null() {
                    e.call(MASTERFILE_LOG, &args![MESSAGE_REFLECTOR_NOT_FOUND, id]);
                } else {
                    e.call(MASTERFILE_LOG, &args![MESSAGE_NOT_PERSISTENT, id]);
                }
                node = drop_unresolved_entry(e, node, previous, item_slot);
                removed = true;
            } else {
                // REF_REFLECTOR_DATA::iEffectFlags (Xbox PDB) +0x04
                if e.mem.u32(item + 4) & 1 != 0 {
                    let list = e.call(REFR_EXTRA_LIST, &args![reference]).u32();
                    e.call(REFLECTOR_ADD_FIRST, &args![list, owner, 1u32]);
                }
                if e.mem.u32(item + 4) & 2 != 0 {
                    let list = e.call(REFR_EXTRA_LIST, &args![reference]).u32();
                    e.call(REFLECTOR_ADD_SECOND, &args![list, owner, 1u32]);
                }
                e.mem.set_u32(item, reference.addr());
            }
            if !removed {
                previous = node;
                node = list_node_next(e, node);
            }
        }
    });
}

// Typed from 00434050
/// `ExtraLitWaterRefs::InitItem` (engine map name, not in the Xbox PDB;
/// `this`, then the reference the extra data belongs to): resolves the form
/// id of every entry to a persistent reference and rebuilds the list from
/// the ones that resolve (the temporary list reverses their order, the
/// refill reverses it again, so the order is kept). An entry whose form is not a
/// reference, or whose reference is not persistent, is logged ("Unable to find
/// lit water reference" / "is not persistent so cannot reflect or refract
/// other references") and dropped. For each good reference, its extra-data
/// list is told about the owner (`ExtraDataList::SetWaterLightRef`, adding)
/// and the reference is added at the head of a temporary list; the list is
/// then cleared and refilled from the temporary one, which is cleared and
/// destroyed. The exception-unwinding frame is not translated.
pub fn extra_lit_water_refs_init_item(e: &mut Engine, this: Ptr<ExtraLitWaterRefs>, owner: Ptr) {
    // `frame`: the temporary list (8 bytes), then the form id and the
    // reference, whose addresses are passed on.
    e.with_stack(0x10, |e, frame| {
        let temporary: Ptr<BSSimpleList> = frame.cast();
        let id_slot = frame.byte_add(8);
        let reference_slot = frame.byte_add(0xc);
        e.call(SIMPLE_LIST_CONSTRUCT, &args![temporary]);
        let list = this.at(ExtraLitWaterRefs::RefList);
        let mut node = list;
        while !node.is_null() && !list_node_is_empty(e, node) {
            let id = list_node_item(e, node);
            e.mem.set_u32(id_slot.addr(), id);
            let reference = resolve_reference(e, owner, id_slot);
            e.mem.set_u32(reference_slot.addr(), reference.addr());
            if reference.is_null() || !e.call(REFR_GET_REF_PERSISTS, &args![reference]).bool() {
                let id = e.mem.u32(id_slot.addr());
                if reference.is_null() {
                    e.call(MASTERFILE_LOG, &args![MESSAGE_LIT_WATER_NOT_FOUND, id]);
                } else {
                    e.call(MASTERFILE_LOG, &args![MESSAGE_NOT_PERSISTENT, id]);
                }
            } else {
                let extra_list = e.call(REFR_EXTRA_LIST, &args![reference]).u32();
                e.call(WATER_LIGHT_REF_SET, &args![extra_list, owner, 1u32]);
                e.call(SIMPLE_LIST_ADD_HEAD, &args![temporary, reference_slot]);
            }
            node = list_node_next(e, node);
        }
        e.call(SIMPLE_LIST_CLEAR, &args![list]);
        let mut walk = temporary;
        while !walk.is_null() {
            let slot = typed_call!(leaf::FAMILY, 0x0068_15c0, leaf::same_word(e, walk));
            if e.mem.u32(slot) == 0 {
                break;
            }
            let slot = typed_call!(leaf::FAMILY, 0x0068_15c0, leaf::same_word(e, walk));
            e.call(SIMPLE_LIST_ADD_HEAD, &args![list, slot]);
            walk = list_node_next(e, walk);
        }
        e.call(SIMPLE_LIST_CLEAR, &args![temporary]);
        e.call(SIMPLE_LIST_DESTRUCT, &args![temporary]);
    });
}

/// The entries of two decal, reflected or reflector lists, compared the way
/// the three `Compare` methods do after their cast: the lists must have the
/// same number of entries, and every entry of `other_list` must have an entry
/// of the same first word in `this` (`GetRefDecalData`) whose first
/// `entry_size` bytes are equal. Returns true when they differ.
pub(crate) fn entry_lists_differ(
    e: &mut Engine,
    this: Ptr<ExtraDecalRefs>,
    other_list: Ptr<BSSimpleList>,
    entry_size: u32,
) -> bool {
    let my_count = e
        .call(
            SIMPLE_LIST_COUNT,
            &args![this.at(ExtraDecalRefs::DecalRefList)],
        )
        .u32();
    let their_count = e.call(SIMPLE_LIST_COUNT, &args![other_list]).u32();
    if my_count != their_count {
        return true;
    }
    let mut node = other_list;
    while !node.is_null() && !list_node_is_empty(e, node) {
        let item = list_node_item(e, node);
        let reference = e.mem.u32(item);
        let counterpart = typed_call!(
            FAMILY,
            0x0043_4410,
            extra_decal_refs_get_ref_decal_data(e, this, reference)
        );
        if counterpart.is_null() {
            return true;
        }
        if e.call(MEMCMP, &args![counterpart, item, entry_size]).u32() != 0 {
            return true;
        }
        node = list_node_next(e, node);
    }
    false
}

// Typed from 004341e0
/// `ExtraDecalRefs::Compare` (engine map name, `vt` slot): true when `other`
/// is not an `ExtraDecalRefs`, when the lists have a different number of
/// entries, or when an entry of the other list has no entry with the same
/// first word here or differs from it in its 0x1c bytes. `BSExtraData::Compare`
/// is not asked.
pub fn extra_decal_refs_compare(e: &mut Engine, this: Ptr<ExtraDecalRefs>, other: Ptr) -> bool {
    let cast: Ptr<ExtraDecalRefs> = checked_cast_extra(e, other, EXTRA_DECAL_REFS_TYPE);
    if cast.is_null() {
        return true;
    }
    entry_lists_differ(e, this, cast.at(ExtraDecalRefs::DecalRefList), 0x1c)
}

// Typed from 004342b0
/// `ExtraDecalRefs::Copy` (engine map name, `vt` slot): when `other` is an
/// `ExtraDecalRefs`, deletes this list's entries (`004343c0`) and adds a copy
/// (0x1c bytes, built with the `REF_DECAL_DATA` constructor) of each of the
/// other list's entries at the head, so the order is reversed. Nothing
/// happens when `other` is another class. The exception-unwinding frame is not
/// translated.
pub fn extra_decal_refs_copy(e: &mut Engine, this: Ptr<ExtraDecalRefs>, other: Ptr) {
    let cast: Ptr<ExtraDecalRefs> = checked_cast_extra(e, other, EXTRA_DECAL_REFS_TYPE);
    if cast.is_null() {
        return;
    }
    typed_call!(FAMILY, 0x0043_43c0, fn_004343c0(e, this));
    let mut node = cast.at(ExtraDecalRefs::DecalRefList);
    while !node.is_null() && !list_node_is_empty(e, node) {
        let item = list_node_item(e, node);
        let memory = e.call(OPERATOR_NEW, &args![0x1cu32]).u32();
        let copy = if memory != 0 {
            e.call(REF_DECAL_DATA_CONSTRUCT, &args![memory]).u32()
        } else {
            0
        };
        e.call(MEMCPY, &args![copy, item, 0x1cu32]);
        let list = this.at(ExtraDecalRefs::DecalRefList);
        e.with_stack(4, |e, local| {
            e.mem.set_u32(local.addr(), copy);
            e.call(SIMPLE_LIST_ADD_HEAD, &args![list, local]);
        });
        node = list_node_next(e, node);
    }
}

// Typed from 004343c0
/// A method of `ExtraDecalRefs` (the engine map has no name for it): deletes
/// every decal entry and empties the list (removes the head, then deletes
/// the entry it held, until the list is empty).
pub fn fn_004343c0(e: &mut Engine, this: Ptr<ExtraDecalRefs>) {
    let list = this.at(ExtraDecalRefs::DecalRefList);
    while !list_node_is_empty(e, list) {
        let item = list_node_item(e, list);
        e.call(SIMPLE_LIST_REMOVE_HEAD, &args![list]);
        e.call(OPERATOR_DELETE, &args![item]);
    }
}

// Typed from 00434410
/// `ExtraDecalRefs::GetRefDecalData` (engine map name; the body is shared,
/// by identical-code folding, with the reflected and reflector classes, whose
/// entries also start with the reference): the first entry of the list whose
/// first word is `reference`, null if there is none.
pub fn extra_decal_refs_get_ref_decal_data(
    e: &mut Engine,
    this: Ptr<ExtraDecalRefs>,
    reference: u32,
) -> Ptr<RefDecalData> {
    let mut node = this.at(ExtraDecalRefs::DecalRefList);
    while !node.is_null() && !list_node_is_empty(e, node) {
        let item = list_node_item(e, node);
        node = list_node_next(e, node);
        if e.mem.u32(item) == reference {
            return Ptr::new(item);
        }
    }
    Ptr::NULL
}

// Typed from 00434480
/// A method of `ExtraDecalRefs` (the engine map has no name for it): sets the
/// decal data of `reference`. When the list has no entry whose first word is
/// `reference`, a 0x1c-byte entry is built (`REF_DECAL_DATA` constructor),
/// given `reference` as its first word and added at the head of the list. The
/// entry's `Intersect` is then set from the vector at `intersect` and its
/// `Normal` from the vector at `normal`. The exception-unwinding frame is not
/// translated.
pub fn fn_00434480(
    e: &mut Engine,
    this: Ptr<ExtraDecalRefs>,
    reference: u32,
    intersect: Ptr,
    normal: Ptr,
) {
    let mut entry = typed_call!(
        FAMILY,
        0x0043_4410,
        extra_decal_refs_get_ref_decal_data(e, this, reference)
    )
    .addr();
    if entry == 0 {
        let memory = e.call(OPERATOR_NEW, &args![0x1cu32]).u32();
        entry = if memory != 0 {
            e.call(REF_DECAL_DATA_CONSTRUCT, &args![memory]).u32()
        } else {
            0
        };
        e.mem.set_u32(entry, reference);
        let list = this.at(ExtraDecalRefs::DecalRefList);
        e.with_stack(4, |e, local| {
            e.mem.set_u32(local.addr(), entry);
            e.call(SIMPLE_LIST_ADD_HEAD, &args![list, local]);
        });
    }
    // REF_DECAL_DATA::Intersect (Xbox PDB) +0x04, ::Normal +0x10
    copy_words(e, intersect.addr(), entry + 4);
    copy_words(e, normal.addr(), entry + 0x10);
}

/// The destructor body of the classes that own the entries of the list at +0xc
/// (`ExtraReflectedRefs`, `ExtraReflectorRefs`): resets the vtable, deletes
/// every entry, clears the list, runs the list's destructor body, then the
/// base destructor. The exception-unwinding frame is not translated.
pub(crate) fn destroy_with_owned_entries(e: &mut Engine, this: Ptr, vtable: u32) {
    e.mem.set_u32(this.addr(), vtable);
    let list = Ptr::<BSSimpleList>::new(this.addr() + 0xc);
    let mut node = list;
    while !node.is_null() && !list_node_is_empty(e, node) {
        let item = list_node_item(e, node);
        e.call(OPERATOR_DELETE, &args![item]);
        node = list_node_next(e, node);
    }
    e.call(SIMPLE_LIST_CLEAR, &args![list]);
    e.call(SIMPLE_LIST_DESTRUCT, &args![list]);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 00434560
/// `ExtraReflectedRefs`' destructor body (the decompiler names it
/// `CMFCRibbonInfo::XQAT::~XQAT` by mistake; the vtable `01014428` is this
/// class's): deletes every entry, clears the list, then the base destructor.
/// The exception-unwinding frame is not translated.
pub fn fn_00434560(e: &mut Engine, this: Ptr<ExtraReflectedRefs>) {
    destroy_with_owned_entries(e, this.cast(), EXTRA_REFLECTED_REFS_VTABLE);
}

// Typed from 00434620
/// `ExtraReflectedRefs::Compare` (engine map name, `vt` slot): true when `other`
/// is not an `ExtraReflectedRefs`, when the lists have a different number of
/// entries, or when an entry of the other list has no entry with the same
/// first word here (`ExtraDecalRefs::GetRefDecalData`, whose body is shared)
/// or differs from it in its 8 bytes. `BSExtraData::Compare` is not asked.
pub fn extra_reflected_refs_compare(
    e: &mut Engine,
    this: Ptr<ExtraReflectedRefs>,
    other: Ptr,
) -> bool {
    let cast: Ptr<ExtraReflectedRefs> = checked_cast_extra(e, other, EXTRA_REFLECTED_REFS_TYPE);
    if cast.is_null() {
        return true;
    }
    entry_lists_differ(e, this.cast(), cast.at(ExtraReflectedRefs::RefList), 8)
}

// Typed from 004346f0
/// `ExtraReflectedRefs::Copy` (engine map name, `vt` slot): when `other` is an
/// `ExtraReflectedRefs`, removes every entry from this list without deleting
/// them (`ClearReferenceData`) and adds a copy (8 bytes) of each of the other
/// list's entries at the head, so the order is reversed. Nothing happens when
/// `other` is another class. The exception-unwinding frame is not translated.
pub fn extra_reflected_refs_copy(e: &mut Engine, this: Ptr<ExtraReflectedRefs>, other: Ptr) {
    let cast: Ptr<ExtraReflectedRefs> = checked_cast_extra(e, other, EXTRA_REFLECTED_REFS_TYPE);
    if cast.is_null() {
        return;
    }
    typed_call!(
        FAMILY,
        0x0043_4d90,
        extra_reflected_refs_clear_reference_data(e, this)
    );
    copy_reference_entries(
        e,
        this.at(ExtraReflectedRefs::RefList),
        cast.at(ExtraReflectedRefs::RefList),
    );
}

/// The loop of the reflected and reflector `Copy` methods: adds at the head of
/// `list` a copy (8 bytes, built like `REF_REFLECTED_DATA`: the list-node
/// constructor `0096a2d0` clears it) of each entry of `source`.
pub(crate) fn copy_reference_entries(
    e: &mut Engine,
    list: Ptr<BSSimpleList>,
    source: Ptr<BSSimpleList>,
) {
    let mut node = source;
    while !node.is_null() && !list_node_is_empty(e, node) {
        let item = list_node_item(e, node);
        let memory = e.call(OPERATOR_NEW, &args![8u32]).u32();
        let copy = if memory != 0 {
            e.call(SIMPLE_LIST_CONSTRUCT, &args![memory]).u32()
        } else {
            0
        };
        e.call(MEMCPY, &args![copy, item, 8u32]);
        e.with_stack(4, |e, local| {
            e.mem.set_u32(local.addr(), copy);
            e.call(SIMPLE_LIST_ADD_HEAD, &args![list, local]);
        });
        node = list_node_next(e, node);
    }
}

// Typed from 00434800
/// A method of `ExtraReflectedRefs` (the engine map has no name for it; its
/// callers in `the extradatalist unit` use it for the reflected and the reflector
/// classes alike, so the body is shared): removes the first entry whose
/// first word is `reference` from the list (the head with remove-head `0063f7b0`, any
/// other node after its predecessor) and deletes the entry. Nothing happens
/// when there is none.
pub fn fn_00434800(e: &mut Engine, this: Ptr<ExtraReflectedRefs>, reference: u32) {
    e.with_stack(4, |e, item_slot| {
        let mut node = this.at(ExtraReflectedRefs::RefList);
        let mut previous: Ptr<BSSimpleList> = Ptr::NULL;
        while !node.is_null() && !list_node_is_empty(e, node) {
            let item = list_node_item(e, node);
            e.mem.set_u32(item_slot.addr(), item);
            if e.mem.u32(item) == reference {
                if previous.is_null() {
                    e.call(SIMPLE_LIST_REMOVE_HEAD, &args![node]);
                } else {
                    e.call(SIMPLE_LIST_REMOVE_ITEM, &args![previous, item_slot]);
                }
                let removed = e.mem.u32(item_slot.addr());
                e.call(OPERATOR_DELETE, &args![removed]);
                e.mem.set_u32(item_slot.addr(), 0);
                return;
            }
            previous = node;
            node = list_node_next(e, node);
        }
    });
}

// Typed from 004348a0
/// A method of `ExtraReflectedRefs` (the engine map has no name for it; the
/// body is shared with the reflector class): sets the effect flags of the
/// entry for `reference`. When the list has no entry whose first word is
/// `reference` (`ExtraDecalRefs::GetRefDecalData`), an 8-byte entry is built
/// (`0096a2d0` clears it), given `reference` as its first word and added at
/// the head of the list. The entry's `iEffectFlags` is then set to `flags`.
/// The exception-unwinding frame is not translated.
pub fn fn_004348a0(e: &mut Engine, this: Ptr<ExtraReflectedRefs>, reference: u32, flags: u32) {
    let mut entry = typed_call!(
        FAMILY,
        0x0043_4410,
        extra_decal_refs_get_ref_decal_data(e, this.cast(), reference)
    )
    .addr();
    if entry == 0 {
        let memory = e.call(OPERATOR_NEW, &args![8u32]).u32();
        entry = if memory != 0 {
            e.call(SIMPLE_LIST_CONSTRUCT, &args![memory]).u32()
        } else {
            0
        };
        e.mem.set_u32(entry, reference);
        let list = this.at(ExtraReflectedRefs::RefList);
        e.with_stack(4, |e, local| {
            e.mem.set_u32(local.addr(), entry);
            e.call(SIMPLE_LIST_ADD_HEAD, &args![list, local]);
        });
    }
    // REF_REFLECTED_DATA::iEffectFlags (Xbox PDB) +0x04
    e.mem.set_u32(entry + 4, flags);
}

// Typed from 00434950
/// `ExtraWaterLightRefs`' destructor body (the decompiler names it
/// `CMFCRibbonInfo::XQAT::~XQAT` by mistake; the vtable `01014440` is this
/// class's): resets the vtable, clears the list (the references are not
/// owned), runs the list's destructor body, then the base destructor. The
/// exception-unwinding frame is not translated.
pub fn fn_00434950(e: &mut Engine, this: Ptr<ExtraWaterLightRefs>) {
    destroy_with_list(e, this.cast(), EXTRA_WATER_LIGHT_REFS_VTABLE);
}

/// The `Compare` of the two reference-list classes `ExtraWaterLightRefs` and
/// `ExtraLitWaterRefs` after their cast: true when the lists have a different
/// number of entries or when an entry of the other list is not in this list.
pub(crate) fn reference_lists_differ(
    e: &mut Engine,
    this_list: Ptr<BSSimpleList>,
    other_list: Ptr<BSSimpleList>,
) -> bool {
    let my_count = e.call(SIMPLE_LIST_COUNT, &args![this_list]).u32();
    let their_count = e.call(SIMPLE_LIST_COUNT, &args![other_list]).u32();
    if my_count != their_count {
        return true;
    }
    let mut node = other_list;
    while !node.is_null() && !list_node_is_empty(e, node) {
        let slot = typed_call!(leaf::FAMILY, 0x0068_15c0, leaf::same_word(e, node));
        if !e.call(SIMPLE_LIST_CONTAINS, &args![this_list, slot]).bool() {
            return true;
        }
        node = list_node_next(e, node);
    }
    false
}

// Typed from 004349c0
/// `ExtraWaterLightRefs::Compare` (engine map name, `vt` slot): true when
/// `other` is not an `ExtraWaterLightRefs`, when the lists have a different
/// number of entries, or when an entry of the other list is not in this list.
/// `BSExtraData::Compare` is not asked.
pub fn extra_water_light_refs_compare(
    e: &mut Engine,
    this: Ptr<ExtraWaterLightRefs>,
    other: Ptr,
) -> bool {
    let cast: Ptr<ExtraWaterLightRefs> = checked_cast_extra(e, other, EXTRA_WATER_LIGHT_REFS_TYPE);
    if cast.is_null() {
        return true;
    }
    reference_lists_differ(
        e,
        this.at(ExtraWaterLightRefs::RefList),
        cast.at(ExtraWaterLightRefs::RefList),
    )
}

// Typed from 00434a70
/// `ExtraWaterLightRefs::Copy` (engine map name, `vt` slot): when `other` is an
/// `ExtraWaterLightRefs`, clears this list (`00434fa0`) and adds each reference
/// of the other list that is not already there (`00434f60`). Nothing happens
/// when `other` is another class.
pub fn extra_water_light_refs_copy(e: &mut Engine, this: Ptr<ExtraWaterLightRefs>, other: Ptr) {
    let cast: Ptr<ExtraWaterLightRefs> = checked_cast_extra(e, other, EXTRA_WATER_LIGHT_REFS_TYPE);
    if cast.is_null() {
        return;
    }
    typed_call!(FAMILY, 0x0043_4fa0, fn_00434fa0(e, this.cast()));
    let mut node = cast.at(ExtraWaterLightRefs::RefList);
    while !node.is_null() && !list_node_is_empty(e, node) {
        let reference = list_node_item(e, node);
        typed_call!(FAMILY, 0x0043_4f60, fn_00434f60(e, this.cast(), reference));
        node = list_node_next(e, node);
    }
}

// Typed from 00434af0
/// `ExtraReflectorRefs`' destructor body (the decompiler names it
/// `CMFCRibbonInfo::XQAT::~XQAT` by mistake; the vtable `01014434` is this
/// class's): deletes every entry, clears the list, then the base destructor.
/// The exception-unwinding frame is not translated.
pub fn fn_00434af0(e: &mut Engine, this: Ptr<ExtraReflectorRefs>) {
    destroy_with_owned_entries(e, this.cast(), EXTRA_REFLECTOR_REFS_VTABLE);
}

// Typed from 00434bb0
/// `ExtraReflectorRefs::Compare` (engine map name, `vt` slot): true when `other`
/// is not an `ExtraReflectorRefs`, when the lists have a different number of
/// entries, or when an entry of the other list has no entry with the same
/// first word here (`ExtraDecalRefs::GetRefDecalData`, whose body is shared)
/// or differs from it in its 8 bytes. `BSExtraData::Compare` is not asked.
pub fn extra_reflector_refs_compare(
    e: &mut Engine,
    this: Ptr<ExtraReflectorRefs>,
    other: Ptr,
) -> bool {
    let cast: Ptr<ExtraReflectorRefs> = checked_cast_extra(e, other, EXTRA_REFLECTOR_REFS_TYPE);
    if cast.is_null() {
        return true;
    }
    entry_lists_differ(e, this.cast(), cast.at(ExtraReflectorRefs::RefList), 8)
}

// Typed from 00434c80
/// `ExtraReflectorRefs::Copy` (engine map name, `vt` slot): when `other` is an
/// `ExtraReflectorRefs`, removes every entry from this list without deleting
/// them (`ExtraReflectedRefs::ClearReferenceData`, whose body is shared) and
/// adds a copy (8 bytes) of each of the other list's entries at the head, so
/// the order is reversed. Nothing happens when `other` is another class. The
/// exception-unwinding frame is not translated.
pub fn extra_reflector_refs_copy(e: &mut Engine, this: Ptr<ExtraReflectorRefs>, other: Ptr) {
    let cast: Ptr<ExtraReflectorRefs> = checked_cast_extra(e, other, EXTRA_REFLECTOR_REFS_TYPE);
    if cast.is_null() {
        return;
    }
    typed_call!(
        FAMILY,
        0x0043_4d90,
        extra_reflected_refs_clear_reference_data(e, this.cast())
    );
    copy_reference_entries(
        e,
        this.at(ExtraReflectorRefs::RefList),
        cast.at(ExtraReflectorRefs::RefList),
    );
}

// Typed from 00434d90
/// `ExtraReflectedRefs::ClearReferenceData` (engine map name; the body is
/// shared with `ExtraReflectorRefs`): removes the head of the list until it is
/// empty. The entries are not deleted.
pub fn extra_reflected_refs_clear_reference_data(e: &mut Engine, this: Ptr<ExtraReflectedRefs>) {
    let list = this.at(ExtraReflectedRefs::RefList);
    while !list_node_is_empty(e, list) {
        e.call(SIMPLE_LIST_REMOVE_HEAD, &args![list]);
    }
}

// Typed from 00434dc0
/// `ExtraLitWaterRefs`' destructor body (the decompiler names it
/// `CMFCRibbonInfo::XQAT::~XQAT` by mistake; the vtable `0101444c` is this
/// class's): resets the vtable, clears the list (the references are not
/// owned), runs the list's destructor body, then the base destructor. The
/// exception-unwinding frame is not translated.
pub fn fn_00434dc0(e: &mut Engine, this: Ptr<ExtraLitWaterRefs>) {
    destroy_with_list(e, this.cast(), EXTRA_LIT_WATER_REFS_VTABLE);
}

// Typed from 00434e30
/// `ExtraLitWaterRefs::Compare` (engine map name, `vt` slot): true when `other`
/// is not an `ExtraLitWaterRefs`, when the lists have a different number of
/// entries, or when an entry of the other list is not in this list.
/// `BSExtraData::Compare` is not asked.
pub fn extra_lit_water_refs_compare(
    e: &mut Engine,
    this: Ptr<ExtraLitWaterRefs>,
    other: Ptr,
) -> bool {
    let cast: Ptr<ExtraLitWaterRefs> = checked_cast_extra(e, other, EXTRA_LIT_WATER_REFS_TYPE);
    if cast.is_null() {
        return true;
    }
    reference_lists_differ(
        e,
        this.at(ExtraLitWaterRefs::RefList),
        cast.at(ExtraLitWaterRefs::RefList),
    )
}

// Typed from 00434ee0
/// `ExtraLitWaterRefs::Copy` (engine map name, `vt` slot): when `other` is an
/// `ExtraLitWaterRefs`, clears this list (`00434fa0`) and adds each reference
/// of the other list that is not already there (`00434f60`). Nothing happens
/// when `other` is another class.
pub fn extra_lit_water_refs_copy(e: &mut Engine, this: Ptr<ExtraLitWaterRefs>, other: Ptr) {
    let cast: Ptr<ExtraLitWaterRefs> = checked_cast_extra(e, other, EXTRA_LIT_WATER_REFS_TYPE);
    if cast.is_null() {
        return;
    }
    typed_call!(FAMILY, 0x0043_4fa0, fn_00434fa0(e, this.cast()));
    let mut node = cast.at(ExtraLitWaterRefs::RefList);
    while !node.is_null() && !list_node_is_empty(e, node) {
        let reference = list_node_item(e, node);
        typed_call!(FAMILY, 0x0043_4f60, fn_00434f60(e, this.cast(), reference));
        node = list_node_next(e, node);
    }
}

// Typed from 00434f60
/// A method of `ExtraWaterLightRefs` (the engine map has no name for it; the
/// body is shared with `ExtraLitWaterRefs`): adds `reference` at the head of
/// the list unless the list already holds it.
pub fn fn_00434f60(e: &mut Engine, this: Ptr<ExtraWaterLightRefs>, reference: u32) {
    let list = this.at(ExtraWaterLightRefs::RefList);
    e.with_stack(4, |e, local| {
        e.mem.set_u32(local.addr(), reference);
        if !e.call(SIMPLE_LIST_CONTAINS, &args![list, local]).bool() {
            e.call(SIMPLE_LIST_ADD_HEAD, &args![list, local]);
        }
    });
}

// Typed from 00434fa0
/// A method of `ExtraWaterLightRefs` (the engine map has no name for it; the
/// body is shared with `ExtraLitWaterRefs`): clears the list (`BSSimpleList`
/// clear: frees the nodes after the head and empties the head).
pub fn fn_00434fa0(e: &mut Engine, this: Ptr<ExtraWaterLightRefs>) {
    e.call(
        SIMPLE_LIST_CLEAR,
        &args![this.at(ExtraWaterLightRefs::RefList)],
    );
}

// Typed from 00434fc0
/// `ExtraMerchantContainer`'s constructor (the engine map has no name for it):
/// extra-data type 0x3c and no container. Returns `this`.
pub fn fn_00434fc0(
    e: &mut Engine,
    this: Ptr<ExtraMerchantContainer>,
) -> Ptr<ExtraMerchantContainer> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_MERCHANT_CONTAINER,
        EXTRA_MERCHANT_CONTAINER_VTABLE,
    );
    s.set(e, this, ExtraMerchantContainer::pContainer, Ptr::NULL);
    this
}

// Typed from 00434ff0
/// `ExtraMerchantContainer::Compare` (engine map name, `vt` slot): true when
/// `other` is not an `ExtraMerchantContainer` or when the containers differ.
/// (`BSExtraData::Compare` is not asked.)
pub fn extra_merchant_container_compare(
    e: &mut Engine,
    this: Ptr<ExtraMerchantContainer>,
    other: Ptr,
) -> bool {
    let s = store(e);
    let cast: Ptr<ExtraMerchantContainer> =
        checked_cast_extra(e, other, EXTRA_MERCHANT_CONTAINER_TYPE);
    if cast.is_null() {
        return true;
    }
    s.get(e, cast, ExtraMerchantContainer::pContainer)
        != s.get(e, this, ExtraMerchantContainer::pContainer)
}

// Typed from 00435040
/// `ExtraLevCreaModifier`'s constructor (the engine map has no name for it):
/// extra-data type 0x1e and the modifier 4 (`LCM_NONE`, Xbox PDB). Returns
/// `this`.
pub fn fn_00435040(e: &mut Engine, this: Ptr<ExtraLevCreaModifier>) -> Ptr<ExtraLevCreaModifier> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_LEV_CREA_MODIFIER,
        EXTRA_LEV_CREA_MODIFIER_VTABLE,
    );
    s.set(
        e,
        this,
        ExtraLevCreaModifier::eModifier,
        LEV_CREA_MODIFIER_NONE,
    );
    this
}

// Typed from 00435070
/// `ExtraLevCreaModifier::Compare` (engine map name, `vt` slot): true when
/// `other` is not an `ExtraLevCreaModifier` or when the modifiers differ.
/// (`BSExtraData::Compare` is not asked.)
pub fn extra_lev_crea_modifier_compare(
    e: &mut Engine,
    this: Ptr<ExtraLevCreaModifier>,
    other: Ptr,
) -> bool {
    let s = store(e);
    let cast: Ptr<ExtraLevCreaModifier> =
        checked_cast_extra(e, other, EXTRA_LEV_CREA_MODIFIER_TYPE);
    if cast.is_null() {
        return true;
    }
    s.get(e, cast, ExtraLevCreaModifier::eModifier)
        != s.get(e, this, ExtraLevCreaModifier::eModifier)
}

// Typed from 004350c0
/// A method of `ExtraLevCreaModifier` (the engine map has no name for it): the
/// modifier's multiplier. 1.0 for `LCM_NONE` (4); otherwise the float game
/// setting the modifier selects in the table at `01184ab0` (one setting
/// pointer per modifier), read through the setting accessor. Returned in ST0.
pub fn fn_004350c0(e: &mut Engine, this: Ptr<ExtraLevCreaModifier>) -> f32 {
    let s = store(e);
    let modifier = s.get(e, this, ExtraLevCreaModifier::eModifier);
    if modifier == LEV_CREA_MODIFIER_NONE {
        return 1.0;
    }
    let setting = e.mem.u32(LEV_CREA_SETTING_TABLE + modifier.wrapping_mul(4));
    let value = e.call(SETTING_FLOAT_VALUE, &args![setting]).u32();
    e.mem.f32(value)
}

// Typed from 00435100
/// A method of `ExtraLevCreaModifier` (the engine map has no name for it): 1
/// for the modifier 0 (`LCM_EASY`), 0 for 4 (`LCM_NONE`) and -1 for any other.
pub fn fn_00435100(e: &mut Engine, this: Ptr<ExtraLevCreaModifier>) -> i32 {
    let s = store(e);
    match s.get(e, this, ExtraLevCreaModifier::eModifier) {
        0 => 1,
        LEV_CREA_MODIFIER_NONE => 0,
        _ => -1,
    }
}

// Typed from 00435150
/// `ExtraPoison`'s constructor (the decompiler names it
/// `CPrintDialog::CPrintDialog` by mistake; the engine map has no name for it):
/// extra-data type 0x3f and the poison given. Returns `this`.
pub fn fn_00435150(e: &mut Engine, this: Ptr<ExtraPoison>, poison: Ptr) -> Ptr<ExtraPoison> {
    let s = store(e);
    construct_base(e, this.cast(), TYPE_POISON, EXTRA_POISON_VTABLE);
    s.set(e, this, ExtraPoison::pPoison, poison);
    this
}

// Typed from 00435180
/// `ExtraPoison::Compare` (engine map name, `vt` slot): true when `other` is
/// not an `ExtraPoison` or when the poisons differ. (`BSExtraData::Compare` is
/// not asked.)
pub fn extra_poison_compare(e: &mut Engine, this: Ptr<ExtraPoison>, other: Ptr) -> bool {
    let s = store(e);
    let cast: Ptr<ExtraPoison> = checked_cast_extra(e, other, EXTRA_POISON_TYPE);
    if cast.is_null() {
        return true;
    }
    s.get(e, cast, ExtraPoison::pPoison) != s.get(e, this, ExtraPoison::pPoison)
}

// Typed from 004351d0
/// `ExtraLastFinishedSequence`'s constructor (the engine map has no name for
/// it): extra-data type 0x41 and a copy of the C string given, in a new
/// buffer of `strlen + 1` bytes (`operator new`, then the checked string copy
/// `00406d30(destination, size, source)`). Returns `this`. The
/// exception-unwinding frame is not translated.
pub fn fn_004351d0(
    e: &mut Engine,
    this: Ptr<ExtraLastFinishedSequence>,
    name: Ptr,
) -> Ptr<ExtraLastFinishedSequence> {
    let s = store(e);
    construct_base(
        e,
        this.cast(),
        TYPE_LAST_FINISHED_SEQUENCE,
        EXTRA_LAST_FINISHED_SEQUENCE_VTABLE,
    );
    let size = e.call(STRLEN, &args![name]).u32().wrapping_add(1);
    let buffer = e.call(OPERATOR_NEW, &args![size]).ptr();
    s.set(
        e,
        this,
        ExtraLastFinishedSequence::pLastSequenceName,
        buffer,
    );
    e.call(STRING_COPY_CHECKED, &args![buffer, size, name]);
    this
}

// Typed from 00435270
/// `ExtraLastFinishedSequence::_scalar_deleting_destructor_` (Xbox PDB): the
/// destructor, then `operator delete` when `flags & 1`. Returns `this`.
pub fn extra_last_finished_sequence_scalar_deleting_destructor(
    e: &mut Engine,
    this: Ptr<ExtraLastFinishedSequence>,
    flags: u32,
) -> Ptr<ExtraLastFinishedSequence> {
    typed_call!(FAMILY, 0x0043_52a0, fn_004352a0(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 004352a0
/// `ExtraLastFinishedSequence`'s destructor body (the engine map has no name
/// for it): resets the vtable, deletes the name buffer (without testing it for
/// null), then runs the base destructor. The exception-unwinding frame is not
/// translated.
pub fn fn_004352a0(e: &mut Engine, this: Ptr<ExtraLastFinishedSequence>) {
    let s = store(e);
    e.mem
        .set_u32(this.addr(), EXTRA_LAST_FINISHED_SEQUENCE_VTABLE);
    let name = s.get(e, this, ExtraLastFinishedSequence::pLastSequenceName);
    e.call(OPERATOR_DELETE, &args![name]);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 00435310
/// `ExtraLastFinishedSequence::Compare` (Xbox PDB): true when `other` is not
/// an `ExtraLastFinishedSequence` or when `strcmp` of the two names is not 0.
/// (`BSExtraData::Compare` is not asked.)
pub fn extra_last_finished_sequence_compare(
    e: &mut Engine,
    this: Ptr<ExtraLastFinishedSequence>,
    other: Ptr,
) -> bool {
    let s = store(e);
    let cast: Ptr<ExtraLastFinishedSequence> =
        checked_cast_extra(e, other, EXTRA_LAST_FINISHED_SEQUENCE_TYPE);
    if cast.is_null() {
        return true;
    }
    let mine = s.get(e, this, ExtraLastFinishedSequence::pLastSequenceName);
    let theirs = s.get(e, cast, ExtraLastFinishedSequence::pLastSequenceName);
    e.call(STRCMP, &args![mine, theirs]).u32() != 0
}

// Typed from 00435370
/// `ExtraXTarget`'s constructor (the engine map has no name for it):
/// extra-data type 0x44 and no target. Returns `this`.
pub fn fn_00435370(e: &mut Engine, this: Ptr<ExtraXTarget>) -> Ptr<ExtraXTarget> {
    let s = store(e);
    construct_base(e, this.cast(), TYPE_X_TARGET, EXTRA_X_TARGET_VTABLE);
    s.set(e, this, ExtraXTarget::pTarget, Ptr::NULL);
    this
}

// Typed from 004353a0
/// `ExtraXTarget::_scalar_deleting_destructor_` (the engine map has no name
/// for it): the destructor, then `operator delete` when `flags & 1`. Returns
/// `this`.
pub fn fn_004353a0(e: &mut Engine, this: Ptr<ExtraXTarget>, flags: u32) -> Ptr<ExtraXTarget> {
    typed_call!(FAMILY, 0x0043_53d0, fn_004353d0(e, this));
    delete_when_asked(e, this.cast(), flags);
    this
}

// Typed from 004353d0
/// `ExtraXTarget`'s destructor body (the engine map has no name for it):
/// resets the vtable, then runs the base destructor. The target is not owned.
pub fn fn_004353d0(e: &mut Engine, this: Ptr<ExtraXTarget>) {
    e.mem.set_u32(this.addr(), EXTRA_X_TARGET_VTABLE);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

/// `00435c10` (`thiscall`, no stack word; engine map `ExtraSavedAnimation::DeleteBuffer`):
/// releases the buffer of the extra data.
const SAVED_DATA_DELETE_BUFFER: u32 = 0x0043_5c10;

// Typed from 00435ac0
// Translation incomplete: 00435ac0 - the C++ exception-unwinding frame (FS:[0] chain, handler at 00eeb968, state variable) is not translated
/// The `ExtraSavedAnimation` destructor (no Ghidra function; `thiscall`, no stack word; called
/// by its scalar deleting destructor `00435a90`): sets the vtable `01015ea0`, releases the
/// buffer (`00435c10`) and ends with the `BSExtraData` destructor (`0040ecb0`).
pub fn fn_00435ac0(e: &mut Engine, this: Ptr) {
    e.mem.set_u32(this.addr(), 0x0101_5ea0);
    e.call(SAVED_DATA_DELETE_BUFFER, &args![this]);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

// Typed from 00435bb0
// Translation incomplete: 00435bb0 - the C++ exception-unwinding frame (FS:[0] chain, handler at 00eeb968, state variable) is not translated
/// The `ExtraSavedHavokData` destructor (no Ghidra function; `thiscall`, no stack word;
/// called by its scalar deleting destructor `00435b80`): sets the vtable `01015eac`, releases
/// the buffer (`00435c10`) and ends with the `BSExtraData` destructor (`0040ecb0`).
pub fn fn_00435bb0(e: &mut Engine, this: Ptr) {
    e.mem.set_u32(this.addr(), 0x0101_5eac);
    e.call(SAVED_DATA_DELETE_BUFFER, &args![this]);
    e.call(BS_EXTRA_DATA_DESTRUCT, &args![this]);
}

/// This file's functions, by exe address.
#[rustfmt::skip]
pub fn entries() -> Vec<(u32, AbiFn)> {
    vec![
        entry!(0x0043_00f0,
            extra_anim_scalar_deleting_destructor(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_0120, fn_00430120(Ptr<_>)),
        entry!(0x0043_01b0, fn_004301b0(Ptr<_>, i32) -> u16),
        entry!(0x0043_0200,
            extra_dismembered_limbs_extra_dismembered_limbs(
                Ptr<_>,
            )
                -> Ptr<_>
        ),
        entry!(0x0043_02a0,
            extra_dismembered_limbs_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            )
                -> Ptr<_>
        ),
        entry!(0x0043_02d0, fn_004302d0(Ptr<_>)),
        entry!(0x0043_0390,
            fn_00430390(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_03c0, fn_004303c0(Ptr<_>)),
        entry!(0x0043_03e0,
            extra_dismembered_limbs_dismembered(Ptr<_>, u8) -> bool
        ),
        entry!(0x0043_0410,
            extra_dismembered_limbs_dismember(Ptr<_>, Ptr<_>, u8, u8)
        ),
        entry!(0x0043_05f0, fn_004305f0(Ptr<_>, u32, u8)),
        entry!(0x0043_0660,
            extra_dismembered_limbs_check_dismembered_limbs_identical(
                Ptr<_>,
                Ptr<_>,
            ) -> bool
        ),
        entry!(0x0043_0780,
            fn_00430780(Ptr<_>, Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_0830, fn_00430830(Ptr<_>) -> Ptr<_>),
        entry!(0x0043_0850,
            extra_starting_position_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_08c0,
            fn_004308c0(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_08f0,
            fn_004308f0(Ptr<_>, Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_0920,
            extra_light_scalar_deleting_destructor(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_0950, fn_00430950(Ptr<_>)),
        entry!(0x0043_09e0,
            refr_lock_get_lock_level(Ptr<_>, Ptr<_>) -> i32
        ),
        entry!(0x0043_0a10, refr_lock_get_level(Ptr<_>, Ptr<_>) -> i32),
        entry!(0x0043_0a90, refr_lock_set_locked(Ptr<_>, bool)),
        entry!(0x0043_0ae0, refr_lock_is_broken(Ptr<_>) -> bool),
        entry!(0x0043_0b40, refr_lock_numeric_value_to_enum(i32) -> i32),
        entry!(0x0043_0bc0, fn_00430bc0(u32) -> i32),
        entry!(0x0043_0c70,
            extra_lock_extra_lock(Ptr<_>, Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_0ca0, fn_00430ca0(Ptr<_>)),
        entry!(0x0043_0ce0, extra_lock_compare(Ptr<_>, Ptr<_>) -> bool),
        entry!(0x0043_0dd0,
            extra_follower_extra_follower(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_0e70,
            extra_follower_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            ) -> Ptr<_>
        ),
        entry!(0x0043_0ea0, fn_00430ea0(Ptr<_>)),
        entry!(0x0043_0f30,
            extra_guarded_ref_data_extra_guarded_ref_data(
                Ptr<_>,
            ) -> Ptr<_>
        ),
        entry!(0x0043_0fa0,
            extra_guarded_ref_data_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            )
                -> Ptr<_>
        ),
        entry!(0x0043_0fd0, fn_00430fd0(Ptr<_>)),
        entry!(0x0043_1030,
            extra_guarded_ref_data_compare(
                Ptr<_>,
                Ptr<_>,
            ) -> bool
        ),
        entry!(0x0043_10d0,
            extra_guarded_ref_data_add_guard(Ptr<_>, Ptr<_>)
        ),
        entry!(0x0043_1120, fn_00431120(Ptr<_>, u32, u32)),
        entry!(0x0043_11f0,
            fn_004311f0(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_1230,
            fn_00431230(Ptr<_>, Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_1260, fn_00431260(Ptr<_>)),
        entry!(0x0043_12f0,
            extra_teleport_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_1360,
            fn_00431360(Ptr<_>, Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_1390,
            extra_map_marker_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            ) -> Ptr<_>
        ),
        entry!(0x0043_13c0, fn_004313c0(Ptr<_>)),
        entry!(0x0043_1450,
            extra_map_marker_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_14c0,
            fn_004314c0(Ptr<_>, Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_14f0,
            extra_audio_marker_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            ) -> Ptr<_>
        ),
        entry!(0x0043_1520, fn_00431520(Ptr<_>)),
        entry!(0x0043_15b0,
            extra_audio_marker_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_1620,
            fn_00431620(Ptr<_>, Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_1650,
            extra_audio_buoy_marker_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            )
                -> Ptr<_>
        ),
        entry!(0x0043_1680, fn_00431680(Ptr<_>)),
        entry!(0x0043_1710,
            extra_audio_buoy_marker_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_1780,
            fn_00431780(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_17c0,
            extra_action_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_1830,
            fn_00431830(Ptr<_>, Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_1860,
            extra_container_changes_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            )
                -> Ptr<_>
        ),
        entry!(0x0043_1890, fn_00431890(Ptr<_>)),
        entry!(0x0043_1920, fn_00431920(Ptr<_>, u32) -> Ptr<_>),
        entry!(0x0043_1950,
            fn_00431950(Ptr<_>, Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_1980,
            fn_00431980(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_19b0, fn_004319b0(Ptr<_>)),
        entry!(0x0043_19d0,
            extra_original_reference_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_1a40,
            fn_00431a40(Ptr<_>, Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_1a70,
            extra_ownership_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_1ae0,
            fn_00431ae0(Ptr<_>, Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_1b10,
            extra_global_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_1b80,
            fn_00431b80(Ptr<_>, i32) -> Ptr<_>
        ),
        entry!(0x0043_1bb0, extra_rank_compare(Ptr<_>, Ptr<_>) -> bool),
        entry!(0x0043_1c20,
            fn_00431c20(Ptr<_>, i16) -> Ptr<_>
        ),
        entry!(0x0043_1c50,
            extra_count_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_1cc0,
            extra_leveled_item_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_1d10,
            fn_00431d10(Ptr<_>, f32) -> Ptr<_>
        ),
        entry!(0x0043_1d40,
            extra_health_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_1db0,
            extra_health_perc_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_1e20,
            fn_00431e20(Ptr<_>, u8) -> Ptr<_>
        ),
        entry!(0x0043_1e50, extra_uses_compare(Ptr<_>, Ptr<_>) -> bool),
        entry!(0x0043_1ec0,
            fn_00431ec0(Ptr<_>, f32) -> Ptr<_>
        ),
        entry!(0x0043_1ef0,
            extra_time_left_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_1f60,
            fn_00431f60(Ptr<_>, f32) -> Ptr<_>
        ),
        entry!(0x0043_1f90,
            extra_charge_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_2000,
            extra_script_extra_script(Ptr<_>, Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_2040, fn_00432040(Ptr<_>)),
        entry!(0x0043_20d0,
            extra_script_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_2140,
            extra_weapon_mod_flags_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_21b0,
            extra_modding_item_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_2220,
            fn_00432220(Ptr<_>, f32) -> Ptr<_>
        ),
        entry!(0x0043_2250,
            extra_scale_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_22c0, fn_004322c0(Ptr<_>) -> Ptr<_>),
        entry!(0x0043_22f0, fn_004322f0(Ptr<_>) -> Ptr<_>),
        entry!(0x0043_2320,
            fn_00432320(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_2350,
            fn_00432350(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_2380,
            fn_00432380(Ptr<_>, i8) -> Ptr<_>
        ),
        entry!(0x0043_23b0,
            extra_info_general_topic_destructor(Ptr<_>)
        ),
        entry!(0x0043_2440,
            extra_info_general_topic_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            )
                -> Ptr<_>
        ),
        entry!(0x0043_2470,
            fn_00432470(Ptr<_>, Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_24a0,
            fn_004324a0(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_2540,
            extra_hot_key_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_25b0,
            fn_004325b0(Ptr<_>, u8) -> Ptr<_>
        ),
        entry!(0x0043_25e0, extra_seed_compare(Ptr<_>, Ptr<_>) -> bool),
        entry!(0x0043_2650,
            fn_00432650(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_26c0,
            fn_004326c0(
                Ptr<_>,
                Ptr<_>,
                Ptr<_>,
                Ptr<_>,
                f32,
            ) -> Ptr<_>
        ),
        entry!(0x0043_2770,
            extra_package_start_location_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_27e0,
            fn_004327e0(Ptr<_>, Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_2810,
            fn_00432810(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_2870,
            fn_00432870(Ptr<_>, Ptr<_>, i32, Ptr<_>, u8, u8, u8) -> Ptr<_>
        ),
        entry!(0x0043_28d0,
            fn_004328d0(Ptr<_>, Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_2900,
            extra_tres_pass_package_destructor(Ptr<_>)
        ),
        entry!(0x0043_29d0,
            fn_004329d0(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_2a70,
            extra_player_crime_list_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            )
                -> Ptr<_>
        ),
        entry!(0x0043_2aa0,
            fn_00432aa0(Ptr<_>, Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_2b50, fn_00432b50(Ptr<_>)),
        entry!(0x0043_2be0,
            fn_00432be0(Ptr<_>, i32) -> Ptr<_>
        ),
        entry!(0x0043_2c20,
            fn_00432c20(Ptr<_>, Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_2c60,
            fn_00432c60(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_2c90, fn_00432c90(Ptr<_>)),
        entry!(0x0043_2cb0,
            extra_rag_doll_data_extra_rag_doll_data(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_2ce0,
            extra_rag_doll_data_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            ) -> Ptr<_>
        ),
        entry!(0x0043_2d10, fn_00432d10(Ptr<_>)),
        entry!(0x0043_2da0, fn_00432da0(Ptr<_>, u32) -> Ptr<_>),
        entry!(0x0043_2dd0,
            extra_rag_doll_data_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_2e60,
            extra_encounter_zone_extra_encounter_zone(
                Ptr<_>,
            ) -> Ptr<_>
        ),
        entry!(0x0043_2e90,
            extra_encounter_zone_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_2f00,
            fn_00432f00(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_2f30, fn_00432f30(Ptr<_>)),
        entry!(0x0043_2f50,
            fn_00432f50(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_2f80,
            extra_used_markers_get_marker_used(Ptr<_>, u32) -> bool
        ),
        entry!(0x0043_2fc0,
            extra_used_markers_set_marker_used(Ptr<_>, u32, u8)
        ),
        entry!(0x0043_3010,
            fn_00433010(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_30b0,
            extra_run_once_packs_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            ) -> Ptr<_>
        ),
        entry!(0x0043_30e0, fn_004330e0(Ptr<_>)),
        entry!(0x0043_31c0, fn_004331c0(Ptr<_>, u32, u8)),
        entry!(0x0043_3260,
            fn_00433260(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_3300,
            fn_00433300(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_3340,
            extra_enable_state_parent_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_33a0,
            fn_004333a0(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_3410,
            extra_enable_state_children_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            )
                -> Ptr<_>
        ),
        entry!(0x0043_3440, fn_00433440(Ptr<_>)),
        entry!(0x0043_34b0,
            fn_004334b0(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_34e0,
            extra_random_teleport_marker_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_3530,
            fn_00433530(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_35a0,
            extra_linked_ref_children_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            )
                -> Ptr<_>
        ),
        entry!(0x0043_35d0, fn_004335d0(Ptr<_>)),
        entry!(0x0043_3640,
            fn_00433640(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_3670,
            extra_linked_ref_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_36c0,
            fn_004336c0(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_36f0,
            fn_004336f0(u32, Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_3750, fn_00433750(Ptr<_>)),
        entry!(0x0043_3790,
            fn_00433790(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_3800,
            extra_activate_ref_children_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            )
                -> Ptr<_>
        ),
        entry!(0x0043_3830, fn_00433830(Ptr<_>)),
        entry!(0x0043_38b0,
            fn_004338b0(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_3940,
            extra_activate_ref_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            ) -> Ptr<_>
        ),
        entry!(0x0043_3970, fn_00433970(Ptr<_>)),
        entry!(0x0043_3a00,
            fn_00433a00(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_3a20,
            extra_activate_ref_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_3b70,
            extra_activate_ref_copy(Ptr<_>, Ptr<_>)
        ),
        entry!(0x0043_3ca0,
            fn_00433ca0(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_3d10,
            extra_decal_refs_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            ) -> Ptr<_>
        ),
        entry!(0x0043_3d40, fn_00433d40(Ptr<_>)),
        entry!(0x0043_3db0,
            extra_decal_refs_init_item(Ptr<_>, Ptr<_>)
        ),
        entry!(0x0043_3ed0,
            extra_reflector_refs_init_item(Ptr<_>, Ptr<_>)
        ),
        entry!(0x0043_4050,
            extra_lit_water_refs_init_item(Ptr<_>, Ptr<_>)
        ),
        entry!(0x0043_41e0,
            extra_decal_refs_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_42b0, extra_decal_refs_copy(Ptr<_>, Ptr<_>)),
        entry!(0x0043_43c0, fn_004343c0(Ptr<_>)),
        entry!(0x0043_4410,
            extra_decal_refs_get_ref_decal_data(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_4480, fn_00434480(Ptr<_>, u32, Ptr<_>, Ptr<_>)),
        entry!(0x0043_4560, fn_00434560(Ptr<_>)),
        entry!(0x0043_4620,
            extra_reflected_refs_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_46f0,
            extra_reflected_refs_copy(Ptr<_>, Ptr<_>)
        ),
        entry!(0x0043_4800, fn_00434800(Ptr<_>, u32)),
        entry!(0x0043_48a0, fn_004348a0(Ptr<_>, u32, u32)),
        entry!(0x0043_4950, fn_00434950(Ptr<_>)),
        entry!(0x0043_49c0,
            extra_water_light_refs_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_4a70,
            extra_water_light_refs_copy(Ptr<_>, Ptr<_>)
        ),
        entry!(0x0043_4af0, fn_00434af0(Ptr<_>)),
        entry!(0x0043_4bb0,
            extra_reflector_refs_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_4c80,
            extra_reflector_refs_copy(Ptr<_>, Ptr<_>)
        ),
        entry!(0x0043_4d90,
            extra_reflected_refs_clear_reference_data(Ptr<_>)
        ),
        entry!(0x0043_4dc0, fn_00434dc0(Ptr<_>)),
        entry!(0x0043_4e30,
            extra_lit_water_refs_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_4ee0,
            extra_lit_water_refs_copy(Ptr<_>, Ptr<_>)
        ),
        entry!(0x0043_4f60, fn_00434f60(Ptr<_>, u32)),
        entry!(0x0043_4fa0, fn_00434fa0(Ptr<_>)),
        entry!(0x0043_4fc0,
            fn_00434fc0(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_4ff0,
            extra_merchant_container_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_5040,
            fn_00435040(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_5070,
            extra_lev_crea_modifier_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_50c0, fn_004350c0(Ptr<_>) -> f32),
        entry!(0x0043_5100, fn_00435100(Ptr<_>) -> i32),
        entry!(0x0043_5150,
            fn_00435150(Ptr<_>, Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_5180,
            extra_poison_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_51d0,
            fn_004351d0(Ptr<_>, Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_5270,
            extra_last_finished_sequence_scalar_deleting_destructor(
                Ptr<_>,
                u32,
            ) -> Ptr<_>
        ),
        entry!(0x0043_52a0, fn_004352a0(Ptr<_>)),
        entry!(0x0043_5310,
            extra_last_finished_sequence_compare(Ptr<_>, Ptr<_>) -> bool
        ),
        entry!(0x0043_5370,
            fn_00435370(Ptr<_>) -> Ptr<_>
        ),
        entry!(0x0043_53a0,
            fn_004353a0(Ptr<_>, u32) -> Ptr<_>
        ),
        entry!(0x0043_53d0, fn_004353d0(Ptr<_>)),
        entry!(0x0043_5ac0, fn_00435ac0(Ptr<_>)),
        entry!(0x0043_5bb0, fn_00435bb0(Ptr<_>)),
    ]
}
