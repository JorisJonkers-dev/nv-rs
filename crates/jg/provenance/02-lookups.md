# Group 2: EditorIDToFormID, GetEditorID

Written 2026-11-22. Source: the plugin's GECK Wiki pages, read in the saved copies of 2026-11-21
(geckwiki.com/index.php/EditorIDToFormID, last edited 24 June 2022; /GetEditorID). Licence: the
pages state none; facts only, no text or example code copied. Opcodes: places in the plugin's
public registration list (names and order only): `GetEditorID` 0x3114 (short name `GetEdID`),
`EditorIDToFormID` 0x3128. Parameter kinds and the optional form come from the declaration lines.

| Command | What the page gives | What it leaves open | What the code does |
|---|---|---|---|
| `EditorIDToFormID "id"` | "the Form ID associated with a specified Editor ID string (case-sensitive)"; answers a form | an empty name; a name with another case | the game's own by-name lookup (the editor-id store answers it when its row is on), then the found form's name must equal the text exactly; else 0 |
| `GetEditorID form` | the editor id as a string; for a reference, that reference's own; empty string when none was set | a call without a form (the form is optional in the parameter list) | the form's `+0x130` name function; without a form, the calling reference's; empty when neither |

Not built from the same list: `GetFormOverrideIndex` (1 call), and ShowOff's `IsBaseForm`.
