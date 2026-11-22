# Group 2, rest: IsBaseForm (ShowOff) and GetFormOverrideIndex (JohnnyGuitar)

Written 2026-11-22. Source: the plugins' GECK Wiki pages, read in the saved copies of
2026-11-21 (`IsBaseForm`, last edited 22 June 2021; `GetFormOverrideIndex`, last edited
1 June 2024). The pages state no licence: facts only, no text or example code copied. Opcodes:
places in the plugins' public registration lists (names and order only): `IsBaseForm` 0x3cd4
(`IsBasedForm`), `GetFormOverrideIndex` 0x316d.

| Command | What the page gives | What it leaves open | What the code does |
|---|---|---|---|
| `IsBaseForm form` | 1 if the variable holds a valid base form, else 0 | whether a reference to a base form counts; an empty variable | 1 for a valid form that is not a reference; 0 for a reference, for no form, and for an id no form has |
| `GetFormOverrideIndex form` | the index of the last mod overriding the form; broken before plugin 5.08 | a form with no file; a missing form | the file index of the last file in the form's own file list; -1 for a form with no file or no valid form |
