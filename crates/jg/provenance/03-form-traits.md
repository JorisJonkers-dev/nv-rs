# Group 3: form traits

Written 2026-11-24. Sources: the plugin's GECK Wiki pages for `GetBaseEffectAV`,
`GetBaseEffectArchetype`, `GetInteriorLightingTraitNumeric`, `SetInteriorLightingTraitNumeric`,
`GetWeaponVATSTraitNumeric` and `SetWeaponVATSTraitNumeric`, read in the saved copies of
2026-11-21. The pages state no licence: facts only, no text or example code copied.

From the plugin's public repository at 6215c70 (the progress pin), as for the earlier groups: the
opcodes (the places of the names in the registration list of `JohnnyCommands.cpp` from 0x3100:
0x3103, 0x3104, 0x310b, 0x310c, 0x3169, 0x316a), and of each declaration line its name,
needs-a-reference flag (false for all six) and parameter-list name (`kParams_OneForm`,
`kParams_OneForm_OneInt`, `kParams_OneForm_OneInt_OneFloat`). No function body was read.

The places of the values are the engine's own layouts: `EffectSetting`'s `DATA` record at +0x58
(engine-fallout-magic `effectsetting.rs`: the archetype +0x98; the actor value is the next word,
+0x9C, as JIP LN's port reads it), `TESObjectCELL`'s flags +0x24 and `pCellData` +0x48 with
`INTERIOR_DATA` (engine-fallout-cell `tesobjectcell.rs`), `TESObjectWEAP`'s
`OBJ_WEAP_VATS_SPECIAL` at +0x370 (engine-fallout-forms `tesobjectweap.rs`; its 0x14 bytes are
the `VATS` record: effect, skill, damage multiplier, action points as floats, silent and mod
required as bytes).

How the owner's scripts call them (their source text): `GetBaseEffectArchetype` and
`GetBaseEffectAV` compared with archetype and actor value codes on a base effect in a variable
(Quick Select); the lighting pair on one interior cell with the ambient channels 0 to 2, read and
set to whole numbers (Sweet Pain, 9 calls); `GetWeaponVATSTraitNumeric` with the damage multiplier
and AP (Mig Power Attack) and `SetWeaponVATSTraitNumeric` setting silent to 0 (ZL Armaments).

| Command | What the page gives | What it leaves open | What the code does |
|---|---|---|---|
| `GetBaseEffectAV effect` | the actor value code of a value-modifier base effect (example: 15) | another archetype; a form that is not a base effect | the record's actor value word as a signed number (-1 when the record has none), for any archetype; 0 for another form |
| `GetBaseEffectArchetype effect` | the archetype code (table of 19 codes) | a form that is not a base effect | the record's archetype word; 0 for another form |
| `GetInteriorLightingTraitNumeric cell trait` | the value of trait 0 to 15 (table: ambient, directional and fog colour channels, rotation XY and Z, fade, fog near, far, power, clip distance) | an exterior cell, another form, a trait out of range; a cell lit by a lighting template | the cell's own `INTERIOR_DATA`: colour channels as 0 to 255 (red the low byte), rotations as signed whole numbers, the rest as floats; 0 for an exterior cell, another form or a trait out of range |
| `SetInteriorLightingTraitNumeric cell trait value` | sets that value (example: ambient red to 35) | the same, and what a value outside a channel's range does; whether the change is drawn at once or saved | writes the same place: a channel takes the whole part held to 0 to 255, a rotation the whole part; nothing for an exterior cell, another form or a trait out of range; the form is not marked changed |
| `GetWeaponVATSTraitNumeric weapon trait` | the value of trait 1 to 5 (skill, damage multiplier, AP, silent, mod required) | another form; a trait out of range | the `VATS` record's place; 0 for another form or trait |
| `SetWeaponVATSTraitNumeric weapon trait value` | sets that value | the same; what silent and mod required take | writes the same place; silent and mod required become 1 for any value other than 0, else 0 |
