<!-- SPDX-License-Identifier: GPL-3.0-only -->
# 01: `GetQuestCompletedAlt` and `Sv_PadStart`

Sources: the GECK Wiki pages GetQuestCompletedAlt and Sv_PadStart (Internet Archive copies).
The opcodes come from the owner's compiled scripts (`NOTICE`). No code of the plugin was read.

What the pages give:

- `(IsCompleted:int) GetQuestCompletedAlt QuestID:ref`: 1 when the quest was completed
  successfully, that is completed and not failed. The page files it under the quest functions
  and the condition functions.
- `(string) Sv_PadStart srcString:string targetLength:int padString:string`: pads the string
  from its start with the pad string, repeated as often as needed, until the result has the
  target length. The pad is cut where the length is reached.

What the code does with that:

- `GetQuestCompletedAlt` reads the quest's flag byte (`TESQuest +0x3C`). The result is 1 when
  the completed bit is set and the failed bit is not. The completed bit is `0x02`, the one the
  game's own `GetQuestCompleted` reads (`0059e400`). The failed bit is `0x40`, the one
  `SetFailed` sets. No quest gives 0. It is also a condition function, with the same answer.
- `Sv_PadStart` answers a new string variable owned by the calling script's mod, as xNVSE's
  string commands do. The result is the pad repeated and cut to the missing length, followed by
  the source.

Left open by the pages, decided here:

- `Sv_PadStart` returns the source string unchanged in these cases:
  - the source is already at or past the target length;
  - the target length is 0 or less;
  - the pad string is empty.
- A target length past the longest string xNVSE keeps (0x3FFF bytes) is cut to that length, so a script
  cannot make the engine build a string of gigabytes.
