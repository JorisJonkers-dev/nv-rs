# Group 5, second part: SetOwnershipTemp

Written 2026-11-22. Source: the plugin's GECK Wiki page for `SetOwnershipTemp` (last edited
14 November 2022), read in the saved copy of 2026-11-21. The page states no licence: facts only,
no text or example code copied. Opcode: the place of the name in the plugin's public registration
list (names and order only): 0x3d10. How the owner's scripts call it was read from their compiled
text: always an owner and a cell (`SetOwnershipTemp <faction or npc> <cell>`), no calling
reference.

| Command | What the page gives | What it leaves open | What the code does |
|---|---|---|---|
| `ref.SetOwnershipTemp [newOwner] [cell]` | alternate of SetOwnership and SetCellOwnership; session persistent, not savebaked; owner is an NPC or faction base form, default the player (whose base form becomes the owner); a cell may be given; answers a success bool | what a reference as owner does beyond the player example; what failure is | owner given a reference: its base form; omitted: the player's base form; with a cell (form type 0x39) given, that cell's extra data list gets the owner, else the calling reference's; the game's own owner setter is used and the form is NOT marked changed (that mark is what makes the game's own `SetOwnership` savebaked); 1 when an owner was set, 0 with neither cell nor calling reference |
