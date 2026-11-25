# Group 5: body-part string traits and cell visits

Written 2026-11-25. Sources: the plugin's GECK Wiki pages for `GetBodyPartTraitString` and
`IsCellVisited`, read in the saved copies of 2026-11-21. The page states no licence: facts only, no text or example code copied.

Facts of the plugin's public repository at 6215c70 (the progress pin), as already extracted for
the earlier groups: the opcodes 0x3192 and 0x3105 (`.fork/catalogue/ref/johnnyguitar.tsv`, from the place of
the name in the registration list from 0x3100), and of each declaration line its name,
needs-a-reference flag (false for both) and parameter list (a form and two integers; one form), from the profile's
command scan of 2026-11-22. No function body was read.

The places of the values are the engine's own layouts: `BGSBodyPartData`'s `bodyParts[15]` at
+0x34 and its form type 0x57 (crates/jip `forms.rs`, checked in the game's loader), and
`BGSBodyPart`'s five `BSStringT` names (engine-fallout-forms `bgsbodypart.rs`, Xbox PDB):
`NodeName` +0x04 (`BPNN`), `TargetName` +0x0C (`BPNT`), `IKStartNodeName` +0x14 (`BPNI`),
`PartName` +0x1C (`BPTN`), `GoreObjectName` +0x24 (`NAM4`, the editor's "gore effects target
bone"). The seen data of a cell through the game's own `ExtraDataList::GetSeenData` (`00555bc0`,
engine-fallout-cell `extradatalist_p5.rs`), which the game creates when the cell is first seen
(interior and exterior kinds, `tesobjectcell_p3.rs`).

How the owner's scripts call it (their source text): JBT Improved asks the V.A.T.S. target
(trait 2) of the hit limb of an actor's body part data, after `GetBodyPartDataHasPart`, and uses
the weapon (type 14) only when the actor has one (2 calls). Unfound Loot asks `IsCellVisited` of
interior cells to run its first-visit work once (2 calls).

| Command | What the page gives | What it leaves open | What the code does |
|---|---|---|---|
| `GetBodyPartTraitString data type trait` | the string trait of a body part data form, type 0 torso to 14 weapon, trait 1 part node, 2 V.A.T.S. target, 3 start node, 4 part name, 5 target bone (example: the torso's part name) | a form of another type, a type the form has no part for, a type or trait out of range, a name not set | the name's text from the part's `BSStringT`; an empty string for every case the page leaves open |
| `IsCellVisited cell` | 1 when the player has visited the cell, else 0; the note: 0 on the first visit when asked from an event that runs before the seen data is updated | a form that is not a cell | 1 when the game's `GetSeenData` of the cell answers seen data, else 0; 0 for another form |
