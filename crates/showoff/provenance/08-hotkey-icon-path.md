# Group 8: the hotkey icon path

Written 2026-11-25. Sources: the plugin's GECK Wiki pages for `GetItemHotkeyIconPath` and
`SetItemHotkeyIconPath`, read in the saved copies of 2026-11-21. The pages state no licence, so
only facts are used and no text or example code is copied.

From the plugin's public repository at f45d069 (release 1.84), as for the earlier groups, two
facts:
- the opcode, `0x3d4e`;
- the declaration line's parameter list: one form, then one optional integer; no reference
  needed.

No function body was read.

What the page gives: the command answers the item's hotkey icon override, the one
`SetItemHotkeyIconPath` sets. With no override it answers the item's regular path, the one
`GetTexturePath` answers. With `bOnlyReturnOverride` 1 it answers only the override, or an empty
string.

What the code does: `SetItemHotkeyIconPath` is not built, and no script of the owner's profile
calls it, so no override can exist. The command therefore answers the texture path as xNVSE's
`GetTexturePath` reads it: the item's `TESTexture` part (`RTTI` `0x01183218`, the base `TESIcon`
derives from), and the text of its path string at `+4`. With `bOnlyReturnOverride` 1 it answers
an empty string. A reference answers for its base form (`TryGetREFRParent`, as the other
base-form commands do).

Left open: the hotkey menu's own use of an override. That is `SetItemHotkeyIconPath`'s part, and
it is not built.
