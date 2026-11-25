<!-- SPDX-License-Identifier: GPL-3.0-only -->
# 01: the override store and its seam (nv-rs#865, pull request 1)

Pages read (saved copies of 2026-11-21): SetActorAnimationPath, SetWeaponAnimationPath, kNVSE
Plugin. No plugin code was read.

What the pages give:

- `actor.SetActorAnimationPath firstPerson enable path [poll condition]` replaces, for that actor,
  the animation group named in the `.kf` file at `path` (or in each file of a folder). Without a
  calling reference it applies to every actor; the page's example adds a condition to leave the
  player out.
- The override stacks: when two mods override the same group on the same actor and one disables
  its override, the other one's comes back.
- `SetWeaponAnimationPath` does the same for a weapon (a later step).

What the code does with that:

- The store is keyed by (an actor's form id or every actor, first or third person, group). Each key
  holds a stack; the entry on top answers. Enabling the same mod's same path again moves it to the
  top; disabling removes that mod's entry for that path only, so the one below answers again.
- An actor's own stack comes before the every-actor stack. The pages do not say which wins; this
  is the reading that lets a mod give one actor a different animation than everyone else.
- First person is the player's first-person animation (`PlayerCharacter::GetAnimation(1)`); every
  other animation is third person.
- Each entry holds the sequence that replaces the group. Loading it from the path, the conditions
  and the poll flag come with the command (the next step).

Left open by the pages, decided here:

- Whether overrides are kept in a save: not on the pages. The store is not saved; scripts set
  overrides again in their own blocks.
- Which of two entries wins when both are on one stack: the newest.

The seam: `Animation::StartGroup` (004948c0) asks it after its test of group 0xff and before it
asks the sequence map. With no handler, or no override for that actor, group and person, the
translation goes on exactly as before (`start_group_without_an_override_is_the_exes_path`). While
the store is empty the handler answers after one atomic load.
