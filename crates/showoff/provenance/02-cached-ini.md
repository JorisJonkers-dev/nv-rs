# Group 4: cached INI reads and writes

Written 2026-11-22. Source: the plugin's GECK Wiki pages, read in the saved copies of 2026-11-21
(`GetINIFloatOrDefault_Cached`, `GetINIFloatOrCreate_Cached`, `GetINIStringOrDefault_Cached`,
`SetINIValue_Cached`, `SaveCachedIniFile`, `ClearFileCacheShowOff`; last edited 11 February 2026;
the pages state no licence: facts only, no text copied; the example of `SetINIValue_Cached` is
used for one test, as a fact about the file it makes). Opcodes: places in the plugin's public
registration list (names and order only): `SetINIValue_Cached` 0x3d26 (`SetINIValC`),
`GetINIFloatOrCreate_Cached` 0x3d27 (`GetINIFltOrCreateC`), `GetINIFloatOrDefault_Cached` 0x3d29
(`GetINIFloat_Cached`), `GetINIStringOrDefault_Cached` 0x3d2a (`GetINIString_Cached`),
`ClearFileCacheShowOff` 0x3d2c (`CloseFileSO`), `SaveCachedIniFile` 0x3d2d (`SaveCachedIni`). All
six are expression-format commands (`kNVSEParams_*` lists).

What the pages give: files under `Data\Config\`; "Section:Key" with `\` and `/` also as the
delimiter; an empty path means the calling mod's name; subfolders allowed; a file that does not
exist is made, with its folders, on writing; the first read caches the file; `Create` and `Set`
change the cache only; `SaveCachedIniFile` recreates the file entirely and answers success;
`Clear` answers 1 when the cache was found and cleared, type 0 INI (path relative to
`Data\Config`) and 1 JSON; `Set` answers -1 for an error, 1 for an updated key, 2 for an
inserted one; a comment is put above a created key; `Create` answers the key's value or the
create value (and writes it to the cache).

Left open, and decided here:

| Open | Decision |
|---|---|
| the file of an empty path | the calling mod's file name with its extension replaced by `.ini` (`MyMod.esp` gives `Data\Config\MyMod.ini`) |
| a path without `.ini` (the page writes "subDir\nameOfFile") | `.ini` is added; any case of `.ini` counts |
| a path that leaves the folder | `..`, an empty part, a drive or a leading `\` is an error: a read answers its default, `Set` answers -1, `Save` and `Clear` answer 0 |
| what an error is for `Get...` | a name without a delimiter or with an empty section or key, a bad path, or an argument of the wrong type: the default (0 or "") |
| a key that exists but is not a number | its leading number as C's `atof` reads it, 0 when it has none |
| case | section and key names and cache paths match without case; an updated key keeps the spelling it has in the file |
| the file's text | lines kept as read (comments, blank lines, other text) and written back with `\r\n`; entries are written `key = value`; a new key goes at the end of its section, a new section at the end of the file after a blank line; a comment is written as `; text` above the new key |
| a number written to the file | as the shortest text that reads back (`2` for 2.0, `2.5`) |
| `Save` for a file that is not cached | answers 0 and writes nothing |
| `Clear` with type 1 (JSON), or another type | 0: there is no JSON cache yet (`ReadFromJSONFile`, a later group, will use type 1); the cache of an INI file is dropped with type 0 |
| a missing file | the cache holds an empty file, so a later `Set` and `Save` make it; the cache is not read again until it is cleared |
