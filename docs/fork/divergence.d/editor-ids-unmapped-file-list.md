### Editor ids: an unmapped file list
- Files: `crates/engine-core/src/editor_ids.rs`, `crates/engine-core/src/editor_ids_tests.rs`
- Upstream approach: since slaterain/nv-rs#1120 a name clash reads a form's file list at +0x10 (the fork's own fix of nv-rs#494 made the same change first, so that part is retired and upstream's is taken); the walk follows the list's nodes and each file record without asking whether they are mapped
- Fork approach: the walk stops at a node, or a file record's name, that is not mapped guest memory, and answers with the file found so far
- Decision: keep; drop when upstream guards the walk
- Reason: a form from a plugin with a damaged list would stop the game with a read through an unmapped address
- Evidence: the fork's test of the unmapped case in `editor_ids_tests.rs`
