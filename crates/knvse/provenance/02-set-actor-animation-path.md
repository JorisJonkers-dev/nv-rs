<!-- SPDX-License-Identifier: GPL-3.0-only -->
# 02: `SetActorAnimationPath` (nv-rs#865, pull request 2)

Sources: the GECK Wiki pages SetActorAnimationPath and kNVSE Plugin (saved copies of 2026-11-21),
and these interface facts of the plugin at the pinned release: its opcode base `0x3920`, its third
registration `SetActorAnimationPath` (so `0x3922`), its parameter list (first person: integer;
enable: integer; animation path: string; poll condition: integer, optional; condition: any form,
optional; match base anim group id: integer, optional), and the name and version its query hands
xNVSE ("kNVSE", 40). No code of the plugin was read; no example text of a page is copied.

What the page gives:

- `actor.SetActorAnimationPath firstPerson enable path [pollCondition] [condition]` replaces the
  animation group named in the `.kf` file at `path` for that actor; without a calling reference,
  for every actor. It answers success 0 or 1.
- The condition is a lambda or user function; the override applies while it returns true. Its
  example leaves the player out with `this != Player`, so the condition runs with the actor as its
  calling reference.
- With the poll flag, the animation starts and stops while it plays as the condition changes.
- `path` may also be a folder of `.kf` files.

What the code does with that:

- Enabled: the file is loaded with the game's `ModelLoader::LoadKF` (the store keeps the manual
  reference that load takes), its group id read from the model's `TESAnimGroup`, and the model put
  on the stack for (actor or every actor, person, group), owned by the mod of the calling script.
  The condition's lambda variables are saved while the store holds it (xNVSE's rule for a lambda
  kept past its call). Answers 1.
- Disabled: the calling mod's entries for that path come off every group of that actor (or every
  actor) and person; references and saved variables are given back. Answers 1 when one came off.
- At each start of the group (the seam in `Animation::StartGroup`), the candidates are asked in
  order (the actor's own stack from the top, then every actor's) and the first whose condition
  holds answers. The condition is called through xNVSE's function call with the actor as `this`;
  its result is read as `GetBool` reads it.
- The sequence started is made from the model as `Animation::AddAnimation` (`00490500`) makes one:
  a `BSAnimGroupSequence` of the model's group and sequence (`004ee9f0`), added to the animation's
  controller manager with the name `AddAnimation` passes. A sequence belongs to one manager, so
  one is made per animation and model and kept, with a manual reference on the model. When the
  animation's manager is made again (`0048ffd0`) or the animation is torn down (`0048fb50`), the
  kept sequences of that animation are forgotten and the references given back (the manager owns
  the sequences). A kept sequence whose manager is no longer the animation's is never started.

Left open by the pages, decided here:

- Which of several candidates answers when an earlier one's condition does not hold: the next one
  on the stacks, so a condition that fails lets the mod below answer.
- Whether overrides are kept in a save or dropped on a load: neither page says. The store is not
  saved and not cleared on a load; it lasts the session, as a plugin's own memory would.
- `match base anim group id`: no public page describes it. It is read and not used.

Not built in this step (both built in step 03, `03-folders-and-polling.md`):

- A folder path.
- Polling: the poll flag is kept, but the condition is asked only at each start of the group.
- A sequence the manager refuses is not deleted. The game's `AddAnimation` reports it and
  removes the group's entry; here the group's own sequence plays, and the refusal is kept, so
  the same manager is not handed a new block at every start.
