// SPDX-License-Identifier: GPL-3.0-only
//! The extra data classes, once each, at their PC offsets (Xbox PDB names where the prototype has
//! them). Declared with `image!`: the marker types typed code reads through, and a `#[repr(C)]`
//! image of the guest bytes for the later move to a native layout.

use crate::image;
use engine_core::ptr::{Inline, Ptr};
use engine_core::types::*;

image! {
    /// `BSExtraData` (Xbox PDB), 0x0C bytes: vtable at +0 (slot 0 the scalar
    /// deleting destructor, slot 1 `Compare`), then the type and the next
    /// extra data of the list.
    pub struct BSExtraData / BSExtraDataImage: 0xc {
        0x00 gap_00: [u8; 4],
        /// `cEtype` (Xbox PDB): the extra data type (`0x43` is
        /// `ExtraNorthRotation`, `0x65` `ExtraReflectedRefs`, ...).
        0x04 cEtype: u8,
        0x05 gap_05: [u8; 3],
        /// `pNext` (Xbox PDB): the next extra data of the list.
        0x08 pNext: Ptr<()>,
    }

    /// `ExtraDataList` (Xbox PDB): a `BaseExtraList` with its own vtable and
    /// no fields of its own.
    pub struct ExtraDataList / ExtraDataListImage: 0x20 {
        0x00 gap_00: [u8; 4],
        /// `pHead` (Xbox PDB), as in `BaseExtraList`.
        0x04 pHead: Ptr<()>,
        0x08 gap_08: [u8; 24],
    }

    /// The statics of `BaseExtraList` that are `__declspec(thread)` in the PC
    /// build, as laid out in the TLS block (`e.tls()`): the last list looked
    /// up, the dirty counter it was valid for, and a cache of the extra data
    /// found in it by type. The Xbox PDB names these `pLastExtraList`,
    /// `iThreadDirty` and `ppLastExtraData`.
    pub struct BaseExtraListThreadCache / BaseExtraListThreadCacheImage: 0x25c {
        0x00 gap_00: [u8; 8],
        /// `pLastExtraList` (Xbox PDB).
        0x08 pLastExtraList: Ptr<()>,
        /// `iThreadDirty` (Xbox PDB): the value of the global `iDirty` the
        /// cache was last cleared at.
        0x0c iThreadDirty: u32,
        /// `ppLastExtraData` (Xbox PDB): first of `0x93` pointers, one per
        /// extra data type, 0 for not cached.
        0x10 ppLastExtraData: Ptr<()>,
        0x14 gap_14: [u8; 584],
    }

    /// `ExtraNorthRotation` (Xbox PDB), type `0x43`, 0x10 bytes.
    pub struct ExtraNorthRotation / ExtraNorthRotationImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `fNorthRot` (Xbox PDB).
        0x0c fNorthRot: f32,
    }

    /// `ExtraDetachTime` (Xbox PDB), 0x10 bytes: the 0x0B-type extra data
    /// `fn_0040f6b0` constructs has this shape (one 32-bit value at +0x0C).
    pub struct ExtraDetachTime / ExtraDetachTimeImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `iTime` (Xbox PDB).
        0x0c iTime: u32,
    }

    /// `ExtraReflectedRefs` (type `0x65`), `ExtraReflectorRefs` (`0x66`),
    /// `ExtraWaterLightRefs` (`0x84`) and `ExtraLitWaterRefs` (`0x85`)
    /// (Xbox PDB), 0x14 bytes each: a `BSExtraData` and the head node of a
    /// `BSSimpleList` of references.
    pub struct ExtraRefList / ExtraRefListImage: 0x14 {
        0x00 gap_00: [u8; 12],
        /// `RefList` (Xbox PDB): the inline head node (item, next).
        0x0c RefList: Inline<BSSimpleList> = [u8; 8],
    }

    /// `BSSoundHandle` (Xbox PDB), 0x0C bytes: the id of the playing sound,
    /// whether success is assumed, and the assumed state.
    pub struct BSSoundHandle / BSSoundHandleImage: 0xc {
        /// `iSoundID` (Xbox PDB); `0xFFFFFFFF` for no sound.
        0x00 iSoundID: u32,
        /// `bAssumeSuccess` (Xbox PDB).
        0x04 bAssumeSuccess: u8,
        0x05 gap_05: [u8; 3],
        /// `eState` (Xbox PDB), `BSSoundHandle::ASSUMED_STATE`.
        0x08 eState: u32,
    }

    /// `ExtraItemDropper` (Xbox PDB), type `0x39`, 0x10 bytes.
    pub struct ExtraItemDropper / ExtraItemDropperImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pDropper` (Xbox PDB): `TESObjectREFR*`.
        0x0c pDropper: Ptr<()>,
    }

    /// `ExtraDroppedItemList` (Xbox PDB), type `0x3A`, 0x14 bytes.
    pub struct ExtraDroppedItemList / ExtraDroppedItemListImage: 0x14 {
        0x00 gap_00: [u8; 12],
        /// `DroppedItemList` (Xbox PDB): `BSSimpleList<TESObjectREFR *>`.
        0x0c DroppedItemList: Inline<BSSimpleList> = [u8; 8],
    }

    /// `ExtraCellWaterType` (Xbox PDB), type 3, 0x10 bytes.
    pub struct ExtraCellWaterType / ExtraCellWaterTypeImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pWater` (Xbox PDB): `TESWaterForm*`.
        0x0c pWater: Ptr<()>,
    }

    /// `ExtraOpenCloseActivateRef` (Xbox PDB), type `0x6C`, 0x10 bytes.
    pub struct ExtraOpenCloseActivateRef / ExtraOpenCloseActivateRefImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pActivateRef` (Xbox PDB): `TESObjectREFR*`.
        0x0c pActivateRef: Ptr<()>,
    }

    /// `ExtraPrimitive` (Xbox PDB), type `0x6B`, 0x10 bytes: owns its
    /// primitive.
    pub struct ExtraPrimitive / ExtraPrimitiveImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pPrimitive` (Xbox PDB): `BGSPrimitive*`.
        0x0c pPrimitive: Ptr<()>,
    }

    /// `ExtraPatrolRefData` (Xbox PDB), type `0x6F`, 0x10 bytes: owns its
    /// patrol data.
    pub struct ExtraPatrolRefData / ExtraPatrolRefDataImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pPatrolData` (Xbox PDB): `PatrolRefData*`.
        0x0c pPatrolData: Ptr<()>,
    }

    /// `ExtraOcclusionPlaneRefData` (Xbox PDB), type `0x76`, 0x10 bytes:
    /// owns its data, four words (the references of the plane).
    pub struct ExtraOcclusionPlaneRefData / ExtraOcclusionPlaneRefDataImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pData` (Xbox PDB): `OcclusionPlaneLinkedRefData*`, 0x10 bytes.
        0x0c pData: Ptr<()>,
    }

    /// `ExtraPortalRefData` (Xbox PDB), type `0x77`, 0x10 bytes: owns its
    /// data, two words (the references of the portal).
    pub struct ExtraPortalRefData / ExtraPortalRefDataImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pData` (Xbox PDB): `PortalLinkedRefData*`, 8 bytes
        /// (`pLinkedRefs`, two `TESObjectREFR*` on PC).
        0x0c pData: Ptr<()>,
    }

    /// `ExtraRoomRefData` (Xbox PDB), type `0x7B`, 0x10 bytes: owns its data.
    pub struct ExtraRoomRefData / ExtraRoomRefDataImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pData` (Xbox PDB): `RoomLinkedRefData*`.
        0x0c pData: Ptr<()>,
    }

    /// `RoomLinkedRefData` (Xbox PDB), 0x14 bytes.
    pub struct RoomLinkedRefData / RoomLinkedRefDataImage: 0x14 {
        /// `PortalList` (Xbox PDB): `BSSimpleList<TESObjectREFR *>`.
        0x00 PortalList: Inline<BSSimpleList> = [u8; 8],
        /// `RoomList` (Xbox PDB): `BSSimpleList<TESObjectREFR *>`.
        0x08 RoomList: Inline<BSSimpleList> = [u8; 8],
        /// `cMaster` (Xbox PDB): the room is a master room.
        0x10 cMaster: u8,
        0x11 gap_11: [u8; 3],
    }

    /// `ExtraCollisionData` (Xbox PDB), type `0x72`, 0x10 bytes: owns its
    /// collision data (freed with `operator delete`).
    pub struct ExtraCollisionData / ExtraCollisionDataImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pCollisionData` (Xbox PDB): `CollisionData*`.
        0x0c pCollisionData: Ptr<()>,
    }

    /// `ExtraPackageData` (Xbox PDB), type `0x70`, 0x10 bytes: the Xbox PDB
    /// names the extra data class of `SetActorPackageData` so.
    pub struct ExtraPackageData / ExtraPackageDataImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pActorPackageData` (Xbox PDB): `ActorPackageData*`.
        0x0c pActorPackageData: Ptr<()>,
    }

    /// `FILE_POS_ROT` (Xbox PDB): a position and a rotation.
    pub struct FilePosRot / FilePosRotImage: 0x18 {
        /// `pos` (Xbox PDB).
        0x00 pos: Inline<NiPoint3> = [u8; 12],
        /// `rot` (Xbox PDB).
        0x0c rot: Inline<NiPoint3> = [u8; 12],
    }

    /// `ExtraAnim` (Xbox PDB), 0x10 bytes. The `BSExtraData` base holds the
    /// vtable at +0, `cEtype` at +4 and `pNext` at +8.
    pub struct ExtraAnim / ExtraAnimImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pAnimation` (Xbox PDB): `Animation*`, owned.
        0x0c pAnimation: Ptr<()>,
    }

    /// `ExtraDismemberedLimbs` (Xbox PDB), 0x30 bytes.
    pub struct ExtraDismemberedLimbs / ExtraDismemberedLimbsImage: 0x30 {
        0x00 gap_00: [u8; 12],
        /// `sLimbs` (Xbox PDB): bit `n` is set when limb `n` is dismembered.
        0x0c sLimbs: u16,
        0x0e gap_0e: [u8; 2],
        /// `eCauseofDeath` (Xbox PDB).
        0x10 eCauseofDeath: u32,
        /// `pDeathObject` (Xbox PDB): `TESForm*`.
        0x14 pDeathObject: Ptr<()>,
        /// `eLastHitLimb` (Xbox PDB).
        0x18 eLastHitLimb: u32,
        /// `bEaten` (Xbox PDB).
        0x1c bEaten: bool,
        0x1d gap_1d: [u8; 3],
        /// `DismemberedLimbs` (Xbox PDB): `BSSimpleArray<DismemberedLimb *,1024>`.
        0x20 DismemberedLimbs: Inline<BSSimpleArray> = [u8; 16],
    }

    /// `DismemberedLimb` (Xbox PDB), 0x14 bytes.
    pub struct DismemberedLimb / DismemberedLimbImage: 0x14 {
        /// `cLimb` (Xbox PDB).
        0x00 cLimb: u8,
        /// `bLimbExploded` (Xbox PDB; a `bool`, kept as the byte the caller
        /// passed).
        0x01 bLimbExploded: u8,
        /// `bObjectArrayIdentical` (Xbox PDB; a `bool`, kept as a byte).
        0x02 bObjectArrayIdentical: u8,
        /// `bLimbRemoved` (Xbox PDB; a `bool`, kept as the byte the caller
        /// passed).
        0x03 bLimbRemoved: u8,
        /// `ObjectArray` (Xbox PDB): `BSSimpleArray<TESBoundObject *,1024>`.
        0x04 ObjectArray: Inline<BSSimpleArray> = [u8; 16],
    }

    /// `ExtraStartingPosition` (Xbox PDB), 0x24 bytes.
    pub struct ExtraStartingPosition / ExtraStartingPositionImage: 0x24 {
        0x00 gap_00: [u8; 12],
        /// `startPosition` (Xbox PDB).
        0x0c startPosition: Inline<FilePosRot> = [u8; 24],
    }

    /// The `BSExtraData` subclass of type 0x49 (vtable `01015b4c`), 0x10
    /// bytes; the Xbox PDB name is not known.
    pub struct ExtraType49 / ExtraType49Image: 0x10 {
        0x00 gap_00: [u8; 12],
        /// The word `004308c0` clears.
        0x0c unnamed0C: u32,
    }

    /// `ExtraLight` (Xbox PDB), 0x10 bytes.
    pub struct ExtraLight / ExtraLightImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pLight` (Xbox PDB): `REFR_LIGHT*`, owned.
        0x0c pLight: Ptr<()>,
    }

    /// `REFR_LOCK` (Xbox PDB), 0x14 bytes.
    pub struct RefrLock / RefrLockImage: 0x14 {
        /// `cBaseLevel` (Xbox PDB).
        0x00 cBaseLevel: u8,
        0x01 gap_01: [u8; 3],
        /// `pKey` (Xbox PDB): `TESKey*`.
        0x04 pKey: Ptr<()>,
        /// `cFlags` (Xbox PDB): bit 0 locked, bit 2 leveled.
        0x08 cFlags: i8,
        0x09 gap_09: [u8; 3],
        /// `uiNumTries` (Xbox PDB).
        0x0c uiNumTries: u32,
        /// `uiTimesUnlocked` (Xbox PDB).
        0x10 uiTimesUnlocked: u32,
    }

    /// `ExtraLock` (Xbox PDB), 0x10 bytes.
    pub struct ExtraLock / ExtraLockImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pLock` (Xbox PDB): `REFR_LOCK*`, owned.
        0x0c pLock: Ptr<RefrLock>,
    }

    /// `ExtraFollower` (Xbox PDB), 0x10 bytes.
    pub struct ExtraFollower / ExtraFollowerImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pActorlist` (Xbox PDB): `BSSimpleList<Actor *>*`, owned.
        0x0c pActorlist: Ptr<()>,
    }

    /// `ExtraGuardedRefData` (Xbox PDB), 0x1C bytes.
    pub struct ExtraGuardedRefData / ExtraGuardedRefDataImage: 0x1c {
        0x00 gap_00: [u8; 12],
        /// `Guards` (Xbox PDB): `BSSimpleArray<unsigned int,1024>`.
        0x0c Guards: Inline<BSSimpleArray> = [u8; 16],
    }

    /// The `BSExtraData` subclass of type 0x2e (vtable `01015b7c`), 0x14
    /// bytes; the Xbox PDB name is not known.
    pub struct ExtraType2e / ExtraType2eImage: 0x14 {
        0x00 gap_00: [u8; 12],
        /// The first word `004311f0` clears.
        0x0c unnamed0C: u32,
        /// The second word `004311f0` clears.
        0x10 unnamed10: u32,
    }

    /// `ExtraTeleport` (Xbox PDB), 0x10 bytes.
    pub struct ExtraTeleport / ExtraTeleportImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pData` (Xbox PDB): the teleport data, owned.
        0x0c pData: Ptr<()>,
    }

    /// `ExtraMapMarker` (Xbox PDB), 0x10 bytes.
    pub struct ExtraMapMarker / ExtraMapMarkerImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pMapData` (Xbox PDB): `MapMarkerData*`, owned.
        0x0c pMapData: Ptr<()>,
    }

    /// `ExtraAudioMarker` (Xbox PDB), 0x10 bytes.
    pub struct ExtraAudioMarker / ExtraAudioMarkerImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pAudioData` (Xbox PDB): `AudioMarkerData*`, owned.
        0x0c pAudioData: Ptr<()>,
    }

    /// `ExtraAudioBuoyMarker` (Xbox PDB), 0x10 bytes.
    pub struct ExtraAudioBuoyMarker / ExtraAudioBuoyMarkerImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pAudioBuoyData` (Xbox PDB): `AudioBuoyMarkerData*`, owned.
        0x0c pAudioBuoyData: Ptr<()>,
    }

    /// `ExtraAction` (Xbox PDB), 0x14 bytes.
    pub struct ExtraAction / ExtraActionImage: 0x14 {
        0x00 gap_00: [u8; 12],
        /// `eAction` (Xbox PDB).
        0x0c eAction: u8,
        0x0d gap_0d: [u8; 3],
        /// `pActionRef` (Xbox PDB): `TESObjectREFR*`.
        0x10 pActionRef: Ptr<()>,
    }

    /// `ExtraContainerChanges` (Xbox PDB), 0x10 bytes.
    pub struct ExtraContainerChanges / ExtraContainerChangesImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pChanges` (Xbox PDB): `InventoryChanges*`, owned.
        0x0c pChanges: Ptr<()>,
    }

    /// `ExtraOriginalReference` (Xbox PDB), 0x10 bytes.
    pub struct ExtraOriginalReference / ExtraOriginalReferenceImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pReference` (Xbox PDB): `TESObjectREFR*`.
        0x0c pReference: Ptr<()>,
    }

    /// `ExtraOwnership` (Xbox PDB), 0x10 bytes.
    pub struct ExtraOwnership / ExtraOwnershipImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pOwner` (Xbox PDB): `TESForm*`.
        0x0c pOwner: Ptr<()>,
    }

    /// `ExtraGlobal` (Xbox PDB), 0x10 bytes.
    pub struct ExtraGlobal / ExtraGlobalImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pGlobal` (Xbox PDB): `TESGlobal*`.
        0x0c pGlobal: Ptr<()>,
    }

    /// `ExtraRank` (Xbox PDB), 0x10 bytes.
    pub struct ExtraRank / ExtraRankImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `iRank` (Xbox PDB).
        0x0c iRank: i32,
    }

    /// `ExtraCount` (Xbox PDB), 0x10 bytes.
    pub struct ExtraCount / ExtraCountImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `iCount` (Xbox PDB): a signed 16-bit count.
        0x0c iCount: i16,
        0x0e gap_0e: [u8; 2],
    }

    /// `ExtraLeveledItem` (Xbox PDB), 0x14 bytes.
    pub struct ExtraLeveledItem / ExtraLeveledItemImage: 0x14 {
        0x00 gap_00: [u8; 12],
        /// `iIndex` (Xbox PDB).
        0x0c iIndex: i32,
        /// `bdefault` (Xbox PDB).
        0x10 bdefault: bool,
        0x11 gap_11: [u8; 3],
    }

    /// `ExtraHealth` (Xbox PDB), 0x10 bytes.
    pub struct ExtraHealth / ExtraHealthImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `fHealth` (Xbox PDB).
        0x0c fHealth: f32,
    }

    /// `ExtraHealthPerc` (Xbox PDB), 0x10 bytes.
    pub struct ExtraHealthPerc / ExtraHealthPercImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `fHealthPerc` (Xbox PDB).
        0x0c fHealthPerc: f32,
    }

    /// `ExtraUses` (Xbox PDB), 0x10 bytes.
    pub struct ExtraUses / ExtraUsesImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `cUses` (Xbox PDB).
        0x0c cUses: u8,
        0x0d gap_0d: [u8; 3],
    }

    /// `ExtraTimeLeft` (Xbox PDB), 0x10 bytes.
    pub struct ExtraTimeLeft / ExtraTimeLeftImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `fTime` (Xbox PDB).
        0x0c fTime: f32,
    }

    /// `ExtraCharge` (Xbox PDB), 0x10 bytes.
    pub struct ExtraCharge / ExtraChargeImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `fCharge` (Xbox PDB).
        0x0c fCharge: f32,
    }

    /// `ExtraScript` (Xbox PDB), 0x14 bytes.
    pub struct ExtraScript / ExtraScriptImage: 0x14 {
        0x00 gap_00: [u8; 12],
        /// `pScript` (Xbox PDB): `Script*`, not owned.
        0x0c pScript: Ptr<()>,
        /// `pScriptVars` (Xbox PDB): `ScriptLocals*`, owned.
        0x10 pScriptVars: Ptr<()>,
    }

    /// `ExtraWeaponModFlags` (Xbox PDB), 0x10 bytes.
    pub struct ExtraWeaponModFlags / ExtraWeaponModFlagsImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `cWeaponModsActive` (Xbox PDB).
        0x0c cWeaponModsActive: u8,
        0x0d gap_0d: [u8; 3],
    }

    /// `ExtraModdingItem` (Xbox PDB), 0x10 bytes.
    pub struct ExtraModdingItem / ExtraModdingItemImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `bIsModding` (Xbox PDB; a `bool`, compared as a byte).
        0x0c bIsModding: u8,
        0x0d gap_0d: [u8; 3],
    }

    /// `ExtraScale` (Xbox PDB), 0x10 bytes.
    pub struct ExtraScale / ExtraScaleImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `fScale` (Xbox PDB).
        0x0c fScale: f32,
    }

    /// `ExtraGhost` (Xbox PDB), 0x0C bytes: the base only.
    pub struct ExtraGhost / ExtraGhostImage: 0xc {
        0x00 gap_00: [u8; 12],
    }

    /// `ExtraWorn` (Xbox PDB), 0x0C bytes: the base only.
    pub struct ExtraWorn / ExtraWornImage: 0xc {
        0x00 gap_00: [u8; 12],
    }

    /// `ExtraWornLeft` (Xbox PDB), 0x0C bytes: the base only.
    pub struct ExtraWornLeft / ExtraWornLeftImage: 0xc {
        0x00 gap_00: [u8; 12],
    }

    /// `ExtraCannotWear` (Xbox PDB), 0x0C bytes: the base only.
    pub struct ExtraCannotWear / ExtraCannotWearImage: 0xc {
        0x00 gap_00: [u8; 12],
    }

    /// `ExtraHotKey` (Xbox PDB), 0x10 bytes.
    pub struct ExtraHotKey / ExtraHotKeyImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `chotkey` (Xbox PDB): a `char`.
        0x0c chotkey: i8,
        0x0d gap_0d: [u8; 3],
    }

    /// `ExtraInfoGeneralTopic` (Xbox PDB), 0x10 bytes.
    pub struct ExtraInfoGeneralTopic / ExtraInfoGeneralTopicImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pInfoGen` (Xbox PDB): `MenuTopic*`, owned.
        0x0c pInfoGen: Ptr<()>,
    }

    /// `ExtraSeed` (Xbox PDB), 0x10 bytes.
    pub struct ExtraSeed / ExtraSeedImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `iSeed` (Xbox PDB): a byte.
        0x0c iSeed: u8,
        0x0d gap_0d: [u8; 3],
    }

    /// `WORLD_LOCATION` (Xbox PDB), 0x14 bytes.
    pub struct WorldLocation / WorldLocationImage: 0x14 {
        /// `pLocationForm` (Xbox PDB): `TESForm*`.
        0x00 pLocationForm: Ptr<()>,
        /// `locPt` (Xbox PDB).
        0x04 locPt: Inline<NiPoint3> = [u8; 12],
        /// `fZRot` (Xbox PDB).
        0x10 fZRot: f32,
    }

    /// `ExtraPackageStartLocation` (Xbox PDB), 0x20 bytes.
    pub struct ExtraPackageStartLocation / ExtraPackageStartLocationImage: 0x20 {
        0x00 gap_00: [u8; 12],
        /// `worldLoc` (Xbox PDB).
        0x0c worldLoc: Inline<WorldLocation> = [u8; 20],
    }

    /// `ExtraReferencePointer` (Xbox PDB), 0x10 bytes.
    pub struct ExtraReferencePointer / ExtraReferencePointerImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pRef` (Xbox PDB): `TESObjectREFR*`.
        0x0c pRef: Ptr<()>,
    }

    /// `ExtraPackage` (Xbox PDB), 0x1C bytes.
    pub struct ExtraPackage / ExtraPackageImage: 0x1c {
        0x00 gap_00: [u8; 12],
        /// `pPack` (Xbox PDB): `TESPackage*`.
        0x0c pPack: Ptr<()>,
        /// `iindex` (Xbox PDB).
        0x10 iindex: i32,
        /// `pTarg` (Xbox PDB): `TESObjectREFR*`.
        0x14 pTarg: Ptr<()>,
        /// `bActionComplete` (Xbox PDB; a `bool`, kept as the byte passed).
        0x18 bActionComplete: u8,
        /// `bActivated` (Xbox PDB; a `bool`, kept as the byte passed).
        0x19 bActivated: u8,
        /// `bDoneOnce` (Xbox PDB; a `bool`, kept as the byte passed).
        0x1a bDoneOnce: u8,
        0x1b gap_1b: [u8; 1],
    }

    /// `ExtraTresPassPackage` (Xbox PDB), 0x10 bytes.
    pub struct ExtraTresPassPackage / ExtraTresPassPackageImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pPack` (Xbox PDB): `TrespassPackage*`, owned.
        0x0c pPack: Ptr<()>,
    }

    /// `ExtraPlayerCrimeList` (Xbox PDB), 0x10 bytes.
    pub struct ExtraPlayerCrimeList / ExtraPlayerCrimeListImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pCrime` (Xbox PDB): `BSSimpleList<Crime *>*`, owned.
        0x0c pCrime: Ptr<()>,
    }

    /// `ExtraPersistentCell` (Xbox PDB), 0x10 bytes.
    pub struct ExtraPersistentCell / ExtraPersistentCellImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pPersistentCell` (Xbox PDB): `TESObjectCELL*`.
        0x0c pPersistentCell: Ptr<()>,
    }

    /// `ExtraRagDollData` (Xbox PDB), 0x10 bytes.
    pub struct ExtraRagDollData / ExtraRagDollDataImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pRagDollData` (Xbox PDB): `RagDollData*`, owned.
        0x0c pRagDollData: Ptr<()>,
    }

    /// `ExtraEncounterZone` (Xbox PDB), 0x10 bytes.
    pub struct ExtraEncounterZone / ExtraEncounterZoneImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pZone` (Xbox PDB): `BGSEncounterZone*`.
        0x0c pZone: Ptr<()>,
    }

    /// `ExtraUsedMarkers` (Xbox PDB), 0x10 bytes.
    pub struct ExtraUsedMarkers / ExtraUsedMarkersImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `iUsedMarkers` (Xbox PDB): bit `n` is set when marker `n` is used.
        0x0c iUsedMarkers: u32,
    }

    /// `ExtraReservedMarkers` (Xbox PDB), 0x10 bytes.
    pub struct ExtraReservedMarkers / ExtraReservedMarkersImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `iReservedMarkers` (Xbox PDB).
        0x0c iReservedMarkers: u32,
    }

    /// `ExtraRunOncePacks` (Xbox PDB), 0x10 bytes.
    pub struct ExtraRunOncePacks / ExtraRunOncePacksImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pPackageList` (Xbox PDB): `BSSimpleList<RunOncePackage *>*`,
        /// owned (the PC build allocates it).
        0x0c pPackageList: Ptr<()>,
    }

    /// `ExtraDistantData` (Xbox PDB), 0x18 bytes.
    pub struct ExtraDistantData / ExtraDistantDataImage: 0x18 {
        0x00 gap_00: [u8; 12],
        /// `LandNormal` (Xbox PDB).
        0x0c LandNormal: Inline<NiPoint3> = [u8; 12],
    }

    /// `ExtraEnableStateParent` (Xbox PDB), 0x14 bytes.
    pub struct ExtraEnableStateParent / ExtraEnableStateParentImage: 0x14 {
        0x00 gap_00: [u8; 12],
        /// `pParent` (Xbox PDB): `TESObjectREFR*`.
        0x0c pParent: Ptr<()>,
        /// `cFlags` (Xbox PDB).
        0x10 cFlags: u8,
        0x11 gap_11: [u8; 3],
    }

    /// `ExtraEnableStateChildren` (Xbox PDB), 0x14 bytes.
    pub struct ExtraEnableStateChildren / ExtraEnableStateChildrenImage: 0x14 {
        0x00 gap_00: [u8; 12],
        /// `ChildList` (Xbox PDB): `BSSimpleList<TESObjectREFR *>`.
        0x0c ChildList: Inline<BSSimpleList> = [u8; 8],
    }

    /// `ExtraRandomTeleportMarker` (Xbox PDB), 0x10 bytes.
    pub struct ExtraRandomTeleportMarker / ExtraRandomTeleportMarkerImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pMarker` (Xbox PDB): `TESObjectREFR*`.
        0x0c pMarker: Ptr<()>,
    }

    /// `ExtraLinkedRefChildren` (Xbox PDB), 0x14 bytes.
    pub struct ExtraLinkedRefChildren / ExtraLinkedRefChildrenImage: 0x14 {
        0x00 gap_00: [u8; 12],
        /// `ChildList` (Xbox PDB): `BSSimpleList<TESObjectREFR *>`.
        0x0c ChildList: Inline<BSSimpleList> = [u8; 8],
    }

    /// `ExtraLinkedRef` (Xbox PDB), 0x10 bytes.
    pub struct ExtraLinkedRef / ExtraLinkedRefImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pLinkedRef` (Xbox PDB): `TESObjectREFR*`.
        0x0c pLinkedRef: Ptr<()>,
    }

    /// `ExtraAshPileRef` (Xbox PDB), 0x10 bytes.
    pub struct ExtraAshPileRef / ExtraAshPileRefImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pAshPileRef` (Xbox PDB): `TESObjectREFR*`.
        0x0c pAshPileRef: Ptr<()>,
    }

    /// `REF_ACTIVATE_DATA` (Xbox PDB), 8 bytes.
    pub struct RefActivateData / RefActivateDataImage: 0x8 {
        /// `pActivateRef` (Xbox PDB): `TESObjectREFR*`.
        0x00 pActivateRef: Ptr<()>,
        /// `fActivateDelay` (Xbox PDB).
        0x04 fActivateDelay: f32,
    }

    /// `ExtraActivateRefChildren` (Xbox PDB), 0x18 bytes.
    pub struct ExtraActivateRefChildren / ExtraActivateRefChildrenImage: 0x18 {
        0x00 gap_00: [u8; 12],
        /// `ChildList` (Xbox PDB): `BSSimpleList<REF_ACTIVATE_DATA *>`.
        0x0c ChildList: Inline<BSSimpleList> = [u8; 8],
        /// `fActivateChildrenTimer` (Xbox PDB).
        0x14 fActivateChildrenTimer: f32,
    }

    /// `ExtraActivateRef` (Xbox PDB), 0x20 bytes.
    pub struct ExtraActivateRef / ExtraActivateRefImage: 0x20 {
        0x00 gap_00: [u8; 12],
        /// `ParentList` (Xbox PDB): `BSSimpleList<REF_ACTIVATE_DATA *>`.
        0x0c ParentList: Inline<BSSimpleList> = [u8; 8],
        /// `cActivateFlags` (Xbox PDB).
        0x14 cActivateFlags: u8,
        0x15 gap_15: [u8; 3],
        /// `ActivateTextOverride` (Xbox PDB): `BSStringT<char>`.
        0x18 ActivateTextOverride: Inline<BSStringT> = [u8; 8],
    }

    /// `ExtraDecalRefs` (Xbox PDB), 0x14 bytes.
    pub struct ExtraDecalRefs / ExtraDecalRefsImage: 0x14 {
        0x00 gap_00: [u8; 12],
        /// `DecalRefList` (Xbox PDB): `BSSimpleList<REF_DECAL_DATA *>`.
        0x0c DecalRefList: Inline<BSSimpleList> = [u8; 8],
    }

    /// `REF_DECAL_DATA` (Xbox PDB), 0x1c bytes.
    pub struct RefDecalData / RefDecalDataImage: 0x1c {
        /// `pDecalRef` (Xbox PDB): `TESObjectREFR*`; holds the form id until
        /// `ExtraDecalRefs::InitItem` resolves it.
        0x00 pDecalRef: Ptr<()>,
        /// `Intersect` (Xbox PDB).
        0x04 Intersect: Inline<NiPoint3> = [u8; 12],
        /// `Normal` (Xbox PDB).
        0x10 Normal: Inline<NiPoint3> = [u8; 12],
    }

    /// `REF_REFLECTED_DATA` and `REF_REFLECTOR_DATA` (Xbox PDB, same layout),
    /// 8 bytes.
    pub struct RefReflectData / RefReflectDataImage: 0x8 {
        /// `pRef` (Xbox PDB): `TESObjectREFR*`; holds the form id until
        /// `InitItem` resolves it.
        0x00 pRef: Ptr<()>,
        /// `iEffectFlags` (Xbox PDB).
        0x04 iEffectFlags: u32,
    }

    /// `ExtraReflectedRefs` (Xbox PDB), 0x14 bytes.
    pub struct ExtraReflectedRefs / ExtraReflectedRefsImage: 0x14 {
        0x00 gap_00: [u8; 12],
        /// `RefList` (Xbox PDB): `BSSimpleList<REF_REFLECTED_DATA *>`, the
        /// entries owned.
        0x0c RefList: Inline<BSSimpleList> = [u8; 8],
    }

    /// `ExtraReflectorRefs` (Xbox PDB), 0x14 bytes.
    pub struct ExtraReflectorRefs / ExtraReflectorRefsImage: 0x14 {
        0x00 gap_00: [u8; 12],
        /// `RefList` (Xbox PDB): `BSSimpleList<REF_REFLECTOR_DATA *>`, the
        /// entries owned.
        0x0c RefList: Inline<BSSimpleList> = [u8; 8],
    }

    /// `ExtraWaterLightRefs` (Xbox PDB), 0x14 bytes.
    pub struct ExtraWaterLightRefs / ExtraWaterLightRefsImage: 0x14 {
        0x00 gap_00: [u8; 12],
        /// `RefList` (Xbox PDB): `BSSimpleList<TESObjectREFR *>`.
        0x0c RefList: Inline<BSSimpleList> = [u8; 8],
    }

    /// `ExtraLitWaterRefs` (Xbox PDB), 0x14 bytes.
    pub struct ExtraLitWaterRefs / ExtraLitWaterRefsImage: 0x14 {
        0x00 gap_00: [u8; 12],
        /// `RefList` (Xbox PDB): `BSSimpleList<TESObjectREFR *>`.
        0x0c RefList: Inline<BSSimpleList> = [u8; 8],
    }

    /// `ExtraMerchantContainer` (Xbox PDB), 0x10 bytes.
    pub struct ExtraMerchantContainer / ExtraMerchantContainerImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pContainer` (Xbox PDB): `TESObjectREFR*`.
        0x0c pContainer: Ptr<()>,
    }

    /// `ExtraLevCreaModifier` (Xbox PDB), 0x10 bytes.
    pub struct ExtraLevCreaModifier / ExtraLevCreaModifierImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `eModifier` (Xbox PDB): a `LEV_CREA_MODIFIER`, read as a 32-bit word.
        0x0c eModifier: u32,
    }

    /// `ExtraPoison` (Xbox PDB), 0x10 bytes.
    pub struct ExtraPoison / ExtraPoisonImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pPoison` (Xbox PDB): `AlchemyItem*`.
        0x0c pPoison: Ptr<()>,
    }

    /// `ExtraLastFinishedSequence` (Xbox PDB), 0x10 bytes.
    pub struct ExtraLastFinishedSequence / ExtraLastFinishedSequenceImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pLastSequenceName` (Xbox PDB): `char*`, owned (allocated).
        0x0c pLastSequenceName: Ptr<()>,
    }

    /// `ExtraXTarget` (Xbox PDB), 0x10 bytes.
    pub struct ExtraXTarget / ExtraXTargetImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pTarget` (Xbox PDB): `TESObjectREFR*`.
        0x0c pTarget: Ptr<()>,
    }

    /// `ExtraEmittanceSource` (Xbox PDB), type `0x67`, 0x10 bytes.
    pub struct ExtraEmittanceSource / ExtraEmittanceSourceImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pSource` (Xbox PDB): `TESForm*`.
        0x0c pSource: Ptr<()>,
    }

    /// `ExtraMultiBoundRef` (Xbox PDB), type `0x63`, 0x10 bytes.
    pub struct ExtraMultiBoundRef / ExtraMultiBoundRefImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pBoundRef` (Xbox PDB): `TESObjectREFR*`.
        0x0c pBoundRef: Ptr<()>,
    }

    /// `ExtraMultiBoundData` (Xbox PDB), type `0x62`, 0x10 bytes.
    pub struct ExtraMultiBoundData / ExtraMultiBoundDataImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pBound` (Xbox PDB): `MultiBoundMarkerData*`, owned.
        0x0c pBound: Ptr<()>,
    }

    /// `ExtraMultiBound` (Xbox PDB), type `0x61`, 0x10 bytes.
    pub struct ExtraMultiBound / ExtraMultiBoundImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `spBound` (Xbox PDB): `NiPointer<BSMultiBound>`.
        0x0c spBound: Ptr<()>,
    }

    /// `ExtraOcclusionPlane` (Xbox PDB), type `0x71`, 0x10 bytes.
    pub struct ExtraOcclusionPlane / ExtraOcclusionPlaneImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `spPlane` (Xbox PDB): `NiPointer<BSOcclusionPlane>`.
        0x0c spPlane: Ptr<()>,
    }

    /// `ExtraPortal` (Xbox PDB), type `0x78`, 0x10 bytes.
    pub struct ExtraPortal / ExtraPortalImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `spPortal` (Xbox PDB): `NiPointer<BSPortal>`.
        0x0c spPortal: Ptr<()>,
    }

    /// `ExtraRoom` (Xbox PDB), type `0x79`, 0x10 bytes.
    pub struct ExtraRoom / ExtraRoomImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `spRoom` (Xbox PDB): `NiPointer<BSMultiBoundRoom>`.
        0x0c spRoom: Ptr<()>,
    }

    /// `ExtraSavedAnimation` (Xbox PDB), type `0x42`, 0x10 bytes.
    pub struct ExtraSavedAnimation / ExtraSavedAnimationImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pAnimationBuffer` (Xbox PDB): `char*`.
        0x0c pAnimationBuffer: Ptr<()>,
    }

    /// `ExtraSavedHavokData` (Xbox PDB), type `0x3d`, 0x10 bytes.
    pub struct ExtraSavedHavokData / ExtraSavedHavokDataImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pHavokBuffer` (Xbox PDB): `char*`.
        0x0c pHavokBuffer: Ptr<()>,
    }

    /// `ExtraFriendHits` (Xbox PDB), type `0x45`, 0x1C bytes.
    pub struct ExtraFriendHits / ExtraFriendHitsImage: 0x1c {
        0x00 gap_00: [u8; 12],
        /// `Hits` (Xbox PDB): `BSSimpleArray<CombatTimeStamp,1024>`; a
        /// `CombatTimeStamp` is one `float` (the time of the hit).
        0x0c Hits: Inline<BSSimpleArray> = [u8; 16],
    }

    /// `ExtraHeadingTarget` (Xbox PDB), type `0x46`, 0x10 bytes.
    pub struct ExtraHeadingTarget / ExtraHeadingTargetImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pheadingtarget` (Xbox PDB): `TESObjectREFR*`.
        0x0c pheadingtarget: Ptr<()>,
    }

    /// `ExtraRefractionProperty` (Xbox PDB), type `0x48`, 0x10 bytes.
    pub struct ExtraRefractionProperty / ExtraRefractionPropertyImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `fRefractionPower` (Xbox PDB).
        0x0c fRefractionPower: f32,
    }

    /// `ExtraEditorRefMoveData` (Xbox PDB), type `0x4c`, 0x30 bytes.
    pub struct ExtraEditorRefMoveData / ExtraEditorRefMoveDataImage: 0x30 {
        0x00 gap_00: [u8; 12],
        /// `realAngle` (Xbox PDB).
        0x0c realAngle: Inline<NiPoint3> = [u8; 12],
        /// `realLocation` (Xbox PDB).
        0x18 realLocation: Inline<NiPoint3> = [u8; 12],
        /// `oldLocation` (Xbox PDB).
        0x24 oldLocation: Inline<NiPoint3> = [u8; 12],
    }

    /// `ExtraHasNoRumors` (Xbox PDB), type `0x4e`, 0x10 bytes.
    pub struct ExtraHasNoRumors / ExtraHasNoRumorsImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `bNoRumors` (Xbox PDB).
        0x0c bNoRumors: u8,
        0x0d gap_0d: [u8; 3],
    }

    /// `ExtraSound` (Xbox PDB), type `0x4f`, 0x18 bytes.
    pub struct ExtraSound / ExtraSoundImage: 0x18 {
        0x00 gap_00: [u8; 12],
        /// `pHandle` (Xbox PDB): the `BSSoundHandle` held by value.
        0x0c pHandle: Inline<BSSoundHandle> = [u8; 12],
    }

    /// `ExtraCreatureAwakeSound` (Xbox PDB), type `0x7d`, 0x18 bytes.
    pub struct ExtraCreatureAwakeSound / ExtraCreatureAwakeSoundImage: 0x18 {
        0x00 gap_00: [u8; 12],
        /// `pHandle` (Xbox PDB): the `BSSoundHandle` held by value.
        0x0c pHandle: Inline<BSSoundHandle> = [u8; 12],
    }

    /// `ExtraCreatureMovementSound` (Xbox PDB), type `0x8a`, 0x18 bytes.
    pub struct ExtraCreatureMovementSound / ExtraCreatureMovementSoundImage: 0x18 {
        0x00 gap_00: [u8; 12],
        /// `pHandle` (Xbox PDB): the `BSSoundHandle` held by value.
        0x0c pHandle: Inline<BSSoundHandle> = [u8; 12],
    }

    /// `ExtraWeaponIdleSound` (Xbox PDB), type `0x83`, 0x18 bytes.
    pub struct ExtraWeaponIdleSound / ExtraWeaponIdleSoundImage: 0x18 {
        0x00 gap_00: [u8; 12],
        /// `pHandle` (Xbox PDB): the `BSSoundHandle` held by value.
        0x0c pHandle: Inline<BSSoundHandle> = [u8; 12],
    }

    /// `ExtraWeaponAttackSound` (Xbox PDB), type `0x86`, 0x18 bytes.
    pub struct ExtraWeaponAttackSound / ExtraWeaponAttackSoundImage: 0x18 {
        0x00 gap_00: [u8; 12],
        /// `pHandle` (Xbox PDB): the `BSSoundHandle` held by value.
        0x0c pHandle: Inline<BSSoundHandle> = [u8; 12],
    }

    /// `ExtraActivateLoopSound` (Xbox PDB), type `0x87`, 0x18 bytes.
    pub struct ExtraActivateLoopSound / ExtraActivateLoopSoundImage: 0x18 {
        0x00 gap_00: [u8; 12],
        /// `pHandle` (Xbox PDB): the `BSSoundHandle` held by value.
        0x0c pHandle: Inline<BSSoundHandle> = [u8; 12],
    }

    /// `ExtraTalkingActor` (Xbox PDB), type `0x55`, 0x10 bytes.
    pub struct ExtraTalkingActor / ExtraTalkingActorImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pTalkObject` (Xbox PDB): `MobileObject*` (the PC constructor
        /// stores the object `00436780` builds).
        0x0c pTalkObject: Ptr<()>,
    }

    /// `ExtraRadius` (Xbox PDB), 0x10 bytes.
    pub struct ExtraRadius / ExtraRadiusImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `fRadius` (Xbox PDB).
        0x0c fRadius: f32,
    }

    /// `ExtraRadiation` (Xbox PDB), 0x10 bytes.
    pub struct ExtraRadiation / ExtraRadiationImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `fRadiation` (Xbox PDB).
        0x0c fRadiation: f32,
    }

    pub struct FactionRank / FACTION_RANKImage: 0x8 {
        /// `pFaction` (Xbox PDB): `TESFaction*`.
        0x00 pFaction: Ptr<()>,
        /// `cRank` (Xbox PDB); `0xff` is rank -1.
        0x04 cRank: u8,
        0x05 gap_05: [u8; 3],
    }

    /// `ExtraFactionChanges` (Xbox PDB), type `0x5e`, 0x10 bytes.
    pub struct ExtraFactionChanges / ExtraFactionChangesImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pFactionChanges` (Xbox PDB): `BSSimpleList<FactionRank *>*`,
        /// owned (a list node is a pointer to the item, then the next node).
        0x0c pFactionChanges: Ptr<()>,
    }

    pub struct RadioData / RADIO_DATAImage: 0x10 {
        /// `fRadius` (Xbox PDB).
        0x00 fRadius: f32,
        /// `eRangeType` (Xbox PDB): `RADIO_RANGE_TYPE`.
        0x04 eRangeType: u32,
        /// `fStaticPct` (Xbox PDB).
        0x08 fStaticPct: f32,
        /// `pPositionRef` (Xbox PDB): `TESObjectREFR*`.
        0x0c pPositionRef: Ptr<()>,
    }

    /// `ExtraRadioData` (Xbox PDB), 0x1c bytes.
    pub struct ExtraRadioData / ExtraRadioDataImage: 0x1c {
        0x00 gap_00: [u8; 12],
        /// `Data` (Xbox PDB).
        0x0c Data: Inline<RadioData> = [u8; 16],
    }

    /// `ExtraCombatStyle` (Xbox PDB), 0x10 bytes.
    pub struct ExtraCombatStyle / ExtraCombatStyleImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pCombatStyle` (Xbox PDB): `TESCombatStyle*`.
        0x0c pCombatStyle: Ptr<()>,
    }

    /// `ExtraAmmo` (Xbox PDB), 0x14 bytes.
    pub struct ExtraAmmo / ExtraAmmoImage: 0x14 {
        0x00 gap_00: [u8; 12],
        /// `pAmmo` (Xbox PDB): `TESAmmo*`.
        0x0c pAmmo: Ptr<()>,
        /// `iCount` (Xbox PDB).
        0x10 iCount: i32,
    }

    /// `ExtraSayTopicInfoOnceADay` (Xbox PDB), type `0x73`, 0x10 bytes.
    pub struct ExtraSayTopicInfoOnceADay / ExtraSayTopicInfoOnceADayImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `pListofSaidOnceTopicInfos` (Xbox PDB):
        /// `BSSimpleList<SayOnceTopicInfos *>*`, owned.
        0x0c pListofSaidOnceTopicInfos: Ptr<()>,
    }

    /// `ExtraSayToTopicInfo` (Xbox PDB), type `0x75`, 0x1C bytes.
    pub struct ExtraSayToTopicInfo / ExtraSayToTopicInfoImage: 0x1c {
        0x00 gap_00: [u8; 12],
        /// `pInfo` (Xbox PDB): `TESTopicInfo*`.
        0x0c pInfo: Ptr<()>,
        /// `pTopic` (Xbox PDB): `TESTopic*`.
        0x10 pTopic: Ptr<()>,
        /// `nResponseID` (Xbox PDB).
        0x14 nResponseID: u32,
        /// `bVoiceDone` (Xbox PDB).
        0x18 bVoiceDone: u8,
        0x19 gap_19: [u8; 3],
    }

    /// `ExtraWaterZoneMap` (Xbox PDB), type `0x7e`, 0x20 bytes. The
    /// `NiTMap<WaterZone *,int>` `WaterZoneMap` sits at +0xc (0x10 bytes).
    pub struct ExtraWaterZoneMap / ExtraWaterZoneMapImage: 0x20 {
        0x00 gap_00: [u8; 28],
        /// `pHighestWaterZone` (Xbox PDB): `WaterZone*`.
        0x1c pHighestWaterZone: Ptr<()>,
    }

    /// `ExtraPatrolRefInUseData` (Xbox PDB), type `0x88`, 0x10 bytes.
    pub struct ExtraPatrolRefInUseData / ExtraPatrolRefInUseDataImage: 0x10 {
        0x00 gap_00: [u8; 12],
        /// `User` (Xbox PDB): the handle of the reference using the patrol
        /// reference.
        0x0c User: u32,
    }

    /// `ExtraFollowerSwimBreadcrumbs` (Xbox PDB), type `0x8b`, 0x28 bytes.
    /// `LeaderLocation` (a `NiPoint3`, +0x10) and `CrumbList` (a
    /// `BSSimpleList`, +0x20) are used by address.
    pub struct ExtraFollowerSwimBreadcrumbs / ExtraFollowerSwimBreadcrumbsImage: 0x28 {
        0x00 gap_00: [u8; 12],
        /// `eLeaderState` (Xbox PDB).
        0x0c eLeaderState: u32,
        0x10 gap_10: [u8; 12],
        /// `LeaderNavMeshID` (Xbox PDB).
        0x1c LeaderNavMeshID: u32,
        0x20 gap_20: [u8; 8],
    }

    /// `ExtraSecuritronFace` (Xbox PDB), 0x1C bytes. `msPersonality` (+0xc)
    /// and `msMood` (+0x14) are `BSStringT<char>`s, used by address.
    pub struct ExtraSecuritronFace / ExtraSecuritronFaceImage: 0x1c {
        0x00 gap_00: [u8; 28],
    }

    /// `ExtraSpecialRenderFlags` (Xbox PDB), 0x14 bytes.
    pub struct ExtraSpecialRenderFlags / ExtraSpecialRenderFlagsImage: 0x14 {
        0x00 gap_00: [u8; 12],
        /// `m_flags` (Xbox PDB).
        0x0c m_flags: i32,
        /// `m_fNodeMaxFadeDistance` (Xbox PDB).
        0x10 m_fNodeMaxFadeDistance: f32,
    }

}
