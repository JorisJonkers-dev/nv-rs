<!-- SPDX-License-Identifier: GPL-3.0-only -->
# 03: `SetActorAnimationPath` folders and polled conditions (nv-rs#865, pull request 3)

Sources: the same GECK Wiki pages as step 02 (SetActorAnimationPath, kNVSE Plugin; saved copies
of 2026-11-21). No code of the plugin was read. No example text of a page is copied.

What the page gives:

- "Path may also be a folder containing KF files."
- With the poll flag set, the animation starts and stops while it plays, as the condition
  becomes true or false.

What the code does with that:

- A path that does not end in `.kf` is a folder. A trailing `\` does not matter. Its `.kf` files
  are listed as `ModelLoader::BuildKFFileList` lists a folder: with the game's file finder
  (`FileFinder::BuildFileList`, `00afe420`), pattern `Data\Meshes\<folder>\*.kf`, loose files
  and the meshes archives. Each is named `<folder>\<file>`. Unlike `BuildKFFileList`, the folder
  is not passed to the idle manager, because a mod's override folder is not an idle folder. The
  list is freed the way the game frees its own: each name freed, the head taken off, the list
  deleted.
- Each file that loads and names a group is put on its group's stack. The entry is kept under
  the folder's path, so disabling the folder takes them all off, and enabling it again replaces
  them all. The condition and the poll flag go with each entry, and the condition's variables are
  saved once for each entry.
- Polling: a group whose start had a polled candidate (poll flag set and a condition given) is
  watched as a pair (animation, group). Every frame (xNVSE's `MainGameLoop` message) the
  candidates are asked again for each watched group that still plays in one of the animation's
  8 slots. The group is started again (`Animation::StartGroup`, with the slot word its own start
  was given: -1 for the slot of its type, or a slot) in two cases:
  - the answer is an override sequence that is not the one playing;
  - no override answers, but one of the overrides' sequences is playing.

  The seam then gives the new answer, or the game's own path plays its own sequence. A watch ends
  when its group no longer plays in any slot or no longer has a polled candidate, when the
  animation's manager is made again or the animation is torn down, on a load, and on the way to
  the main menu.

Left open by the pages, decided here:

- Two files in one folder that name the same group. The first in name order is used, and the
  other's load is given back. The page says nothing about choosing between them.
- Files in folders below the folder are not used. The game's own folder listing does not go
  down either.
- A condition with the poll flag but no condition function polls nothing.
- `match base anim group id`: no public page describes it. It is still read and not used.
