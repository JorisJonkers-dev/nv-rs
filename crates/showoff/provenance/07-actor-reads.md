# Group 7: actor and item reads

Written 2026-11-25. Sources: the plugin's GECK Wiki pages for `IsLimbCrippled`,
`GetIngestibleConsumeSound` (plugin version 1.30), `IsAiming` and `IsBlockingOrAiming` (1.65),
read in the saved copies of 2026-11-21. The pages state no licence: facts only, no text or example
code copied.

From the plugin's public repository at f45d069 (release 1.84), as for the earlier groups: the
opcodes (the comments of the registration list in `ShowOffNVSE.cpp`: 0x3cc7, 0x3cfb, 0x3d4a,
0x3d4b), and of each command's declaration line (`functions/SO_fn_*.h`) its name,
needs-a-reference flag, parameter list and whether it is a condition: `IsLimbCrippled`
(reference, two optional integers, condition), `GetIngestibleConsumeSound` (no reference, one
optional form, answers a form), `IsAiming` and `IsBlockingOrAiming` (reference, no parameters,
condition). No function body was read.

The engine's own facts used: the limb condition actor values 0x19 (perception: the head) to 0x1F
(the brain) in the page's limb order, read through the actor's `ActorValueOwner` (+0xA4, virtual
+0xC, as the game's `GetActorValue` condition does); `Actor::GetIronSights` (`008bbc10`) and
`Actor::GetBlocked` (`00894d60`: the animation action is 7, block), engine-fallout-ai;
`AlchemyItem`'s `data.pConsumptionSound` at +0xC8 (engine-fallout-magic `alchemyitem.rs`) and the
form type 0x2F.

| Command | What the page gives | What it leaves open | What the code does |
|---|---|---|---|
| `actor.IsLimbCrippled [limb] [threshold]` | 1 when the limb's condition actor value is at or below 0, or at or below the threshold; limb -1 or none: any limb; the table of limbs 0 head to 6 brain | another reference; a limb outside the table | the condition value at or below the threshold (default 0); "any" includes the brain, as the table does; 0 for another reference or a limb outside -1 to 6 |
| `actor.IsAiming` | 1 while the actor aims or zooms in | another reference | the game's iron sights answer (its byte); 0 for another reference |
| `actor.IsBlockingOrAiming` | aiming, or the animation action is block | another reference | iron sights, or the game's `GetBlocked`; 0 for another reference |
| `[ref.]GetIngestibleConsumeSound [ingestible]` | the consumption sound of the ingestible; a reference stands for its base form; script only | another form; an ingestible without a sound | the sound's form; no form for another form, no sound, or neither a form nor a reference |

Not built from the profile's list yet: `GetIsPlayerOverencumbered` and `IsNight`. Their pages leave
open how the perk entry points enter the weight (`GetIsPlayerOverencumbered`) and which of the
sunrise and sunset begin and end hours bound the night (`IsNight`). Both wait for a reading of the
engine's own uses.
