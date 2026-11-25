# Group 9: the enable parent and the equipped items

Written 2026-11-25. Sources: the plugin's GECK Wiki pages for `SetEnableParent`,
`GetEquippedWeapon` and `GetEquippedItems`, read in the saved copies of 2026-11-21. The pages state no licence, so only
facts are used and no text or example code is copied.

From the plugin's public repository at f45d069 (release 1.84), as for the earlier groups, these
facts: the opcodes `0x3d30`, `0x3d31` and `0x3cd3` (`.fork/catalogue/ref/showoff.tsv`), the alias
`SetParentRef`, and of each declaration line the parameter list (one optional form; none; one optional
integer) and the needs-a-reference flag (true for all three), from the profile's command scan of 2026-11-22. No function
body was read.

How the owner's scripts call them (their source text): Goodies (and its TTW version) gives two
dogs a named disable-parent marker and later removes the parent of another reference (3 calls
each); Simple Melee Attack Cancel and Parry asks the player's and an attacker's weapon (3 calls); JBT Improved and Stash Organizer ask the
player's equipped items without flags (2 calls).

| Command | What the page gives | What it leaves open | What the code does |
|---|---|---|---|
| `ref.SetEnableParent [newParent]` | the calling reference's enable parent becomes `newParent`; none or null removes it | the child list of the parents, a parent the game does not keep, a loop, the reference's enabled state at the time of the call | the game's own re-parenting `0056aa10` (leaves the old parent's child list, keeps only a persistent parent), then, as the game's own reference copy `0055b9c0` does, `0041dcd0` on the new parent's extra data (joins its child list); a parent that is the reference or has it in its own chain is refused; a form that is not a reference changes nothing; the enabled state is not changed now (the parent's next enable or disable reaches it) |
| `actor.GetEquippedWeapon` | the base form of the actor's current weapon; with none, the fists weapon | another kind of reference | `Actor::GetCurrentWeapon` (`008a1710`, engine-fallout-ai `actor_p3.rs`), else the game's fists weapon (`011ca278`, read the same way by `Actor::RestoreActionPoints`); 0 for a reference that is not an actor |
| `actor.GetEquippedItems [flags]` | an array of the actor's equipped base forms, each once; no flags: every playable item; flags: the 20 equip slots (head 1 to body add-on 3), then no unplayable items (bit 20), no quest items (bit 21), no slotless items (bit 22) | the order, a reference that is not an actor, how a slotless item is found | each chosen slot asked as xNVSE's `GetEquippedObject` does (`find_equipped`: the first worn stack using the slot), in slot order from the head, each item once; playable as xNVSE's `IsPlayable` answers for armor and weapons, quest item the form flag `0x400`; a slotless item is never found, so bit 22 changes nothing; an empty array for a reference that is not an actor |
