# Group 5, first part: weapon commands

Written 2026-11-22. Source: the plugin's GECK Wiki pages, read in the saved copies of 2026-11-21
(`IsWeaponMelee`, `IsWeaponRanged`, `IsEquippedWeaponMelee`, `WeaponHasFlag`, `SetWeaponFlag`).
The pages state no licence: facts only, no text copied. Opcodes: places in the plugin's public
registration list (names and order only): `IsWeaponMelee` 0x3c9d, `IsEquippedWeaponMelee` 0x3c9e,
`IsWeaponRanged` 0x3c9f, `GetWeaponHasFlag` 0x3cdf (`WeaponHasFlag`), `SetWeaponFlag` 0x3d0f
(`SetWeaponHasFlag`). Layout of the weapon form (type byte `+0xf4`, first flags byte `+0x100`,
second flags word `+0x12c`): as `crates/nvse/src/commands/weapons.rs` already reads them.

| Command | What the page gives | What it leaves open | What the code does |
|---|---|---|---|
| `IsWeaponMelee [weapon]` | type <= 2; the reference's base form without the argument; a condition | a form that is not a weapon | 1 for type 0 to 2; 0 for a non-weapon or no form |
| `IsWeaponRanged [weapon]` | type from 3 to 13 | type 14 and up | 1 for 3 to 13, else 0 |
| `IsEquippedWeaponMelee` | the equipped weapon's type is melee; 1 when none is equipped (fists); a condition | no calling reference | the weapon of equipment slot 5 as xNVSE's `GetEquippedObject 5` finds it; 1 with none; 0 without a calling reference |
| `GetWeaponHasFlag index [weapon]` | indices 0 to 21 from a table; GetWeaponFlags1 plus GetWeaponFlags2; a condition | the split of the 22 indices over the two flags | 0 to 7 = bits of the first flags byte, 8 to 21 = bits 0 to 13 of the second flags word (the order of the GECK's weapon flag lists); 0 for an index outside 0 to 21 |
| `SetWeaponFlag index on [weapon]` | sets or clears that flag; 1 for success, 0 otherwise | what failure is | 0 for an index outside 0 to 21, a non-weapon or no form, and nothing is changed; any non-zero `on` sets |

Not built from the same group: `GetEquippedWeapon` (the page answers the Fists weapon form with
no weapon equipped; the form is not in any source read), `GetIngestibleConsumeSound`,
`GetItemHotkeyIconPath`, `GetEquippedItems`, the temporary ownership and faction commands,
`SetEnableParent`, `DisableAlt`, `IsNight`, `IsLimbCrippled`, `IsAiming`, `IsBlockingOrAiming`,
`GetIsPlayerOverencumbered`.
