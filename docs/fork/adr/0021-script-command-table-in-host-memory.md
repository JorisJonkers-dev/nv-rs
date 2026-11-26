# ADR-0016 amendment (Proposed): the script command table and the built-ins' block in host memory

> A fork decision record kept under `docs/fork/adr/` because `docs/adr/` is upstream-owned
> (ADR-0017). It amends sections 2 and 4 of
> [ADR-0016](../../adr/0016-script-extenders-built-in.md); the accepted record stands until this
> amendment is accepted.

**Status:** Proposed (2026-11-26). Not accepted. Deletion of the guest memory waits on it
(issue #916, track D, user stories 53 and 54).
**Date:** 2026-11-26
**Amends:** [ADR-0016](../../adr/0016-script-extenders-built-in.md) §2 ("The command table in guest
memory, and the immediates") and §4 ("Where built-ins' guest data and code addresses live").

## Context

ADR-0016 §2 keeps one script command table in guest memory as xNVSE builds it: a copy of the exe's
0x280 records of 0x28 bytes from `0x01190910` (opcodes `0x1000..0x127f`), xNVSE's padding command up
to `0x1400`, xNVSE's 654 commands from `0x1400`, and the built-in plugins' commands further up.
Section 4 puts the table, its strings and parameter lists in `mem::BUILTIN_DATA`
(`0x7F000000..0x80000000`) and the handlers, evals and routines in `mem::EXTENDER_CODE`
(`0x7E800000..0x7F000000`). The engine's `ScriptTables { start, end, max_opcode }` holds the table's
guest address and bound, and the translations read it where xNVSE patched 19 immediates in 13 exe
functions. That is how a built-in command works today without a DLL.

The native-port programme (issue #916, track D) removes the guest memory, the translations and the
local decomp backend once no family needs them. User story 53 requires opcode bases and co-save ids
to stay unchanged throughout; user story 54 requires deletion only when the ledger shows no caller
by address left. The table and the reserved block are such callers today: their records hold guest
code addresses and the descriptor holds a guest data address. Deleting the guest memory before they
move would break every mod that calls a built-in command and every co-save a built-in plugin carries.

The numbers and paths below are checked against `crates/nvse/src/table.rs` (`TABLE_AT`,
`TABLE_ROOM`, `DATA_AT`, `Record`), `crates/nvse/src/names.rs`, `crates/jip/src/table.rs`, the
built-in plugins' `identity.rs`, `crates/engine-core/src/extend.rs` (`ScriptTables`, `EXECUTE_BASE`,
`EVAL_BASE`, `ROUTINE_BASE`) and `crates/engine-core/src/mem.rs`.

## Decision (proposed)

### 1. How opcode bases and co-save ids stay stable

**Opcode bases are numbers, not addresses, and do not change.** A compiled script calls a command
by its opcode; the host table is indexed by that same opcode with the same arithmetic
`record(opcode) = start + (opcode - 0x1000) * 0x28`, where `start` is the first record of a host
vector instead of a guest address. The bounds stay as they are today:

| Owner | Opcode block | Table bound after install |
| --- | --- | --- |
| Exe | `0x1000..0x127f` (0x280 records of 0x28 bytes, from `0x01190910`) | `max_opcode = 0x1280` |
| xNVSE | padding `0x1280..0x13ff`, then 654 commands from `0x1400` | `0x1400 + 654` |
| JIP LN NVSE | former lutana block `0x2200..0x233a` (299 commands), JIP's own `0x2600..0x2942` (819 commands; 16 empty slots in each block) | `0x2943` |
| Built-in plugins | Tweaks `0x2132`, MCM `0x21c0`, JohnnyGuitar `0x3100`, Tale of Two Wastelands `0x3800`, ShowOff `0x3c93` | unchanged by the move |

An empty slot stays empty and a command not built yet keeps its name with no handler, so a call
fails at the same opcode and names the command in the log as it does today. `IsPluginInstalled` and
`GetPluginVersion` still answer from the same plugin identities and bases.

**Co-save ids stay because they belong to the file format, not to the table's storage.** Each
built-in plugin's co-save section header is its opcode base: `0x1400` for xNVSE's own section,
`0x2200` for JIP, `0x3100` for JohnnyGuitar, `0x3c93` for ShowOff, `0x2132` for Tweaks. The chunk
type tags and their versions stay as written: xNVSE's `MODS`, `ARVS`, `ARVR`, `ARVE`, `STVS`,
`STVR` and `STVE`, JIP's `VSPJ`, `VAPJ`, `MRPJ`, `DEPJ`, `RLPJ`, `FGPJ`, `UAPJ` and `PNPJ`, and
JohnnyGuitar's `NVXS`. String and array variable ids are already host-side numbers keyed by the ids
scripts hold in their locals; they stay `u32` and keep their values. None of these is a pointer into
the reserved block, so moving where the records live leaves the co-save bytes unchanged and a
co-save written before the move still loads after it.

### 2. The commands and data that move

- **The built-in script command table.** Every record the extenders write into `BUILTIN_DATA`: the
  0x280 vanilla records copied from `0x01190910`, xNVSE's padding records for `0x1280..0x13ff`, the
  654 xNVSE commands from `0x1400`, JIP's records in both of its blocks (including the empty
  slots), and the records of the built-in plugins (Tweaks, MCM, JohnnyGuitar, Tale of Two Wastelands
  and ShowOff). Each record's fields move with it: long name, short name, opcode, help text,
  needs-a-reference word, parameter count, parameters pointer, execute word, parse word, eval word
  and flags. Return types are already host-side, by opcode, and do not move.
- **The strings and parameter lists the records point at**, allocated above the records from
  `DATA_AT`: command long and short names, help texts, `ParamInfo` lists (name pointer, type id,
  optional byte), the `string_var` and `array_var` keyword aliases, built-in statistic names, JIP's
  parameter lists and the expression scratch buffer.
- **The code words in `EXTENDER_CODE`**: the per-opcode execute handlers at `EXECUTE_BASE`
  (`0x7E800000`), the condition evals at `EVAL_BASE` (`0x7EC00000`) and the named routines at
  `ROUTINE_BASE` (`0x7EFF0000`). Those routines are xNVSE's default execute and parse, the built-in
  parse routines, the padding handler, JIP's identity, override and game-patch routines, and the
  extra miscellaneous-statistic natives, as well as each plugin command's handler. They become host
  function references, not addresses to call through guest memory.
- **The other fixed data a built-in places in the block**: the fake input device vtables and the
  UI, script-runner and game-patch scratch data a plugin maps there, with the plugin that owns them.
- **What does not move**: the return types, the string and array variable state, the co-save format
  and its ids, and the exe's own script and console command tables. The console and the vanilla
  interpreter keep their tables until their own families move; this amendment covers the table
  ADR-0016 builds and the block the built-ins reserve.

### 3. How the move is made

- The table becomes a host vector of records indexed by opcode, with the 0x28 field layout held as a
  Rust struct rather than as guest bytes. Record reads and writes become host operations, and the
  strings and parameter lists become host data; nothing allocates from `DATA_AT` any more.
- `ScriptTables` stops supplying a guest address: `max_opcode` and the opcode arithmetic stay, while
  `start` and `end` become positions in the host vector. The reads at the 19 immediates then resolve
  records from that vector. With nothing installed they still resolve to the exe's records, so
  vanilla behaviour is unchanged.
- Name lookup, `GetFunctionDef`, the `GetFunction` and `HelpFunction` seams and the `help` listing
  read the host table by opcode and by name; no lookup depends on a guest pointer.
- Handler, eval and parse words become host function references registered through the existing
  wrap and seam tables. Commands still run on the main game thread, and an event raised on another
  thread is still queued for the tick.
- The reserved ranges are no longer mapped: `mem::EXTENDER_CODE` and `mem::BUILTIN_DATA` leave the
  guest memory layout, with the `profile::kind_of` names for them, and no heap range takes their
  place.
- The gate is the table and co-save tests: opcode bases and bounds, co-save section ids, chunk tags
  and versions, and co-save round trips compared byte for byte. A command at a moved opcode must
  answer its name and handler from the host table, and a command not built yet must still fail at
  its opcode.

### 4. Acceptance and ordering

Until this amendment is accepted, ADR-0016 §2 and §4 remain in force and every test that pins the
table's and the block's guest addresses stays. The amendment is accepted when no family or extender
reads the guest table or the reserved block and the gates above pass. Only then may the guest
memory, the translations and the backend be deleted (issue #916, user story 54): no plugin DLL is
loaded after that, and built-in extenders still answer every opcode they own.

## Consequences

- Mods' compiled scripts keep calling the same opcodes, and their co-saves keep loading, with
  nothing installed; a mod that needs a plugin nv-rs does not have still fails at that call.
- The built-in extenders no longer reserve a guest range, so nothing in the game's memory layout
  depends on `EXTENDER_CODE` or `BUILTIN_DATA` and the guest reservation can go.
- The descriptor reads, the wrap chain and the seams stay as the interface to the extenders; the
  storage behind them changes, so the changes to engine crates stay small.
- Until acceptance, this record adds no work to ADR-0016's build order; it states the last step
  that lets track D delete the guest memory.
