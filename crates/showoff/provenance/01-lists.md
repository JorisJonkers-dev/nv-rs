# Group 3: ListAddList, ListAddArray

Written 2026-11-22. Source: the plugin's GECK Wiki pages, read in the saved copies of 2026-11-21
(geckwiki.com/index.php/ListAddList and /ListAddArray; the pages state no licence: facts only, no
text copied; the ListAddArray page has an example, which is not copied: the test builds its own
array). Opcodes: places in the plugin's public registration list (names and order only):
`ListAddList` 0x3cad (short name `AddFormListToFormList`), `ListAddArray` 0x3d1c (short name
`AddArrayToFormList`). Parameters from the pages' syntax lines.

| Command | What the page gives | What it leaves open | What the code does |
|---|---|---|---|
| `ListAddList dest src [index]` | forms of `src` appended in their order to `dest` at `index` (the end when not given); 1 for success, 0 for an error; same changes as several `ListAddForm`s, so not savebaked | what an error is; whether the order is kept at an index; a negative index; an empty source; a list added to itself; both arguments are "baseform" | an error is: either argument is not a form list (type byte), the read fails, or an add finds no such place. Each form goes in as `ListAddForm` adds one (duplicates allowed, the same check of the index against the forms scripts added); the first at `index`, each next right after the one before, so the order of the source is kept; a negative index is the end; an empty source answers 1; the source is read before the first add, so a list added to itself doubles; an add that fails stops the run, answers 0, and the forms before it stay (several `ListAddForm`s would do the same); an index equal to the list's count has no form to go before and fails as `ListAddForm`'s does (the end is -2 or no index) |
| `ListAddArray list arr [index]` | the forms of an array appended in key order; 1 or 0; non-forms and invalid forms are skipped; same changes as `ListAddForm`s | the same as above; an array that does not exist; a form id no form has ("invalid") | elements in the container's own order (a map is ordered by its key); numbers, strings and arrays are skipped; an id that finds no form is skipped; an array that does not exist, a first argument that is not a form list, or an index that is not a number answers 0 and adds nothing; an array with no forms in it answers 1 |

Parameter kinds: both `ListAddList` lists are `AnyForm` (the page calls both "baseform"; the list
check is made by the command so a wrong argument answers 0, as the page says, instead of failing
the script's compile); `ListAddArray` is an expression-format command (form, array, optional
number).
