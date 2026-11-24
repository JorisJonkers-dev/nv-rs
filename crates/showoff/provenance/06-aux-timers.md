# Group 6: the auxiliary timers

Written 2026-11-24. Sources: the plugin's GECK Wiki pages for `AuxTimerStart`, `AuxTimerStop`,
`AuxTimerPaused`, `AuxTimerTimeElapsed`, `AuxTimerTimeToCountdown`, `AuxTimerTimeLeft` (last
edited 5 January 2026), `SetOnAuxTimerStartHandler`, `SetOnAuxTimerStopHandler` (9 October 2025)
and `SetOnAuxTimerUpdateHandler` (14 December 2024), read in the saved copies of 2026-11-21. The
pages state no licence: facts only, no text or example code copied.

From the plugin's public repository at f45d069 (release 1.84), as for the earlier groups: the
opcodes (the comments of the registration list in `ShowOffNVSE.cpp`: 0x3d3e to 0x3d45 and
0x3d4c), and of each command's declaration line its name, alias, needs-a-reference flag (false
for all nine) and parameter list (names and types, from `nvse/ParamInfos.h`). No function body
was read. A name search in `ShowOffNVSE.cpp` also printed six lines of the plugin's message
handler that name the timers; nothing of them is used here.

How the owner's scripts call them (their source text, one mod, 42 calls): always temporary shared
names (`*_...`), no owner argument, flags 14, 526 or 1038 (game mode, seconds, auto-remove on
load, and ignore turbo or ignore the time multiplier in game mode), `AuxTimerTimeLeft` compared
with -1 and 0, and stop handlers set from a lambda with a name and no owner.

| Command | What the page gives | What it leaves open | What the code does |
|---|---|---|---|
| `owner.AuxTimerStart name [time] [flags] [owner]` | makes and starts the timer, or restarts it (a paused one runs again); default owner PlayerRef; flags table (11 bits); default flags 1+2+4; runs out: stop event, then erased unless it auto-restarts (then it restarts and fires the start event); owner gone: erased, no event; answers a success bool | the meaning of the default time -1; whether a restart takes the new flags; whether a start fires the start event | a time of 0 or more is the countdown; -1 (omitted) restarts an existing timer with its own countdown and makes none (0); a restart takes the given flags with the paused bit cleared; every start and restart fires the start event |
| `AuxTimerStop name [fireEvent] [owner]`, alias `AuxTimerErase` | erases the timer; the stop event only with fireEvent; 0 when there was none | | 1 when a timer was erased |
| `AuxTimerPaused name [setPaused] [owner]` | 1 paused, 0 not, -1 none; then sets it when setPaused is given | which values set | 0 and 1 set; any other value (the default -1) only reads |
| `AuxTimerTimeElapsed name [unused] [owner]` | time since it started, paused time not counted; -1 none | | countdown minus time left |
| `AuxTimerTimeToCountdown name [owner]` | the countdown it was told; -1 none | | |
| `AuxTimerTimeLeft name [owner]` | time left; -1 none | | |
| `SetOnAuxTimerStartHandler`, `SetOnAuxTimerStopHandler`, `SetOnAuxTimerUpdateHandler` (aliases `SetOnTimer...Handler`) `setOrRemove script name [owner]` | adds or removes a function called with the name and owner (the update also with the time counted: 1 for a frame-based timer, else the seconds); the update only with flag 256, every frame the timer runs; handlers removed at a load and at the main menu; default owner PlayerRef | whether adding twice calls twice; what is answered | added once (a second add of the same function, event and timer does nothing); the command answers nothing (result 0); a lambda's variables are kept while it is a handler |

Rules the pages imply, and the choices made where they do not say:
- The owner is the form argument as given, else the calling reference, else `PlayerRef` (form id
  0x14); a timer is found by its owner's form id. The handler gets the owner as a form.
- Names follow the auxiliary variables' rule the pages refer to ("private-permanent",
  "temporary-public"): a leading `*` is temporary; a `_` after it makes the name shared, otherwise
  the timer belongs to the mod of the calling script (the last file of the script form's files).
  Names compare without case; the events pass the name as it was given first.
- Counting, every frame (xNVSE's `MainGameLoop`): a paused timer does not count; a timer counts in
  menu mode only with flag 1, in game mode only with flag 2, and not while the game is paused (the
  start menu or the console open) with flag 128. A frame-based timer counts 1. A seconds-based
  one counts the game's step (`TimeGlobal`'s `fDeltaUnmodified`) when it ignores the time
  multiplier in the current mode (flag 64 in menu mode, 1024 in game mode); else the step times
  the global multiplier (`fDelta`) with flag 512 (turbo slows the world through V.A.T.S.'s
  multiplier, which xNVSE's delayed calls use too); else that times V.A.T.S.'s multiplier.
- The events of a frame fire after every timer counted, in the timers' order (update, then stop,
  then the restart's start), so a handler may start or stop timers.
- At a load (`PreLoadGame`) and the main menu, every handler goes and every timer except the
  temporary ones without flag 8. A new game drops the permanent timers.

Not built yet: permanent timers are not written into the co-save (the plugin's own record of them
is not public). They last until the next load, new game or main menu. The owner's mods use
temporary timers only.
