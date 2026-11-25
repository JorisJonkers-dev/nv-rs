# Group 4: game-state reads

Written 2026-11-24. Sources: the plugin's GECK Wiki pages for `GetPipBoyMode`,
`GetThresholdedActorValue` and `GetActorValueAlt`, read in the saved copies of 2026-11-21. The
pages state no licence: facts only, no text or example code copied.

From the plugin's public repository at 6215c70 (the progress pin), as for the earlier groups: the
opcodes (the places of the names in the registration list of `JohnnyCommands.cpp` from 0x3100:
0x3168, 0x3174, 0x31a7), and of each declaration line (`fn_utility.h`, `fn_av.h`) its name, short
name, needs-a-reference flag, parameter-list name and whether it is a condition:
`GetPipBoyMode` (no reference, no parameters, condition), `GetActorValueAlt` / `GetAVAlt`
(reference, `kParams_OneInt`), `GetThresholdedActorValue` / `GetThresholdAV` (reference,
`kParams_OneActorValue`, condition). No function body was read.

The places of the values are the engine's own: the Pip-Boy state word at `InterfaceManager`
+0x4BC (`InterfaceManager::UpdatePipboy`, `0070ee80`, and `FOPipboyManager`'s reads of 3 as open,
engine-fallout-interface; jip's `game.rs` names the field), `ActorValueOwner::
GetClampedActorFloatValue` (`0066ef50`, engine-fallout-forms `actorvalue.rs`: the owner's value
held within the actor value's limits) on the actor's owner at +0xA4, and the `GetActorValue`
condition function (`0059c4f0`, engine-fallout-script `tesconditionfunctions.rs`).

How the owner's scripts call them (their source text): `GetPipBoyMode == 0` in conditions of
Enhanced Movement's camera and sprint scripts (9 calls); `Player.GetThresholdAV Agility`,
`Endurance`, `Strength`, and `rHolder.GetThresholdAV (GetWeaponSkill ...)` with an expression.

| Command | What the page gives | What it leaves open | What the code does |
|---|---|---|---|
| `GetPipBoyMode` | the Pip-Boy's state: 0 closed, 1 or 2 opening, 3 open, 4 or 5 closing | before the interface exists | the state word as it is; 0 without an interface manager; the same as a condition |
| `actor.GetThresholdedActorValue av` | the actor value capped by its limits, as the game's formulas use it (example: Barter on the player) | a reference that is not an actor | the game's clamped value of the actor's owner; 0 for no actor or a code past 76; the same as a condition, the actor value its first parameter |
| `actor.GetActorValueAlt code` | the current, modified value of the actor value with that number (example: 16); marked deprecated (xNVSE takes numbers in `GetActorValue`) | a number out of range; the page types the result as an integer | the game's `GetActorValue` condition with the number, so the value is the one `GetActorValue` answers (not rounded); 0 for no actor or a number outside 0 to 76 |

Not built from the same declaration file: the other `...ActorValueAlt` commands (set, mod, force,
damage, restore, base): no script of the profile calls them.
