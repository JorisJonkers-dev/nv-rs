# Pointer-key scan

A read-only, re-runnable regex scan for code whose result can depend on the width or layout of a guest
pointer (a 32-bit `u32` in a `Ptr<T>` wrapper today). A wider pointer or a different layout changes
anything computed from a pointer's value: hash buckets, orderings and the iteration orders that follow.
It walks `crates/**/*.rs` (skipping `target`) and recognises four kinds of pattern:

- `hash` - `m_uiHashSize` / `m_ppkHashTable` reads, a key reduced by `%` against a size or count,
  a `>> 2` bucket shift, or a virtual bucket slot (`SLOT_BUCKET`, `KeyToHashIndex`).
- `order` - `sort`, `sort_by`, `sort_by_key`, `cmp` or a spaced `<` / `>` on pointer-like values
  (`Ptr`, `ptr`, `addr`, `*_addr`, `.addr()`).
- `map-key` - `BTreeMap`, `BTreeSet`, `HashMap`, `HashSet` keyed by `Ptr`, `u32`, `usize` or `isize`.
- `cast-mix` - a key-like value cast with `as usize` / `as u32` then mixed with `%`, `>>`, `^` or
  `wrapping_mul`.

**Reach** is the number of distinct *other* `.rs` files whose text contains the enclosing function's
name as a word: a cheap call-graph proxy, not a call graph. A name that is a word in more than 40
`.rs` files (`GENERIC_FILE_THRESHOLD`) is **generic**: its reach reads `generic` and it ranks after
every row with a numeric reach. Rows are then ranked by kind and path. Test code is not reported:
files named `*_tests.rs`, `tests.rs` or `test_*.rs` (including `test_support.rs` and `test_game.rs`),
anything under `tests/`, modules declared by `#[cfg(test)] mod name;` in a sibling file, and code
after `#[cfg(test)]` are skipped as findings, but still count towards the word index and reach. The
table shows the top 150 rows by reach; the counts above cover every finding. Findings are candidates
for triage: a regex hit is not on its own proof of pointer dependence, and a miss is not proof of independence. Known limits: an address used as an index with `>> 2` is found only next to a key-like name, a module declared with `#[path]` is not followed, a cfg(test) struct field whose comma is on a later line hides the lines up to it, and raw strings are not tokenised.

Re-run from the repository root with
`python3 scripts/fork/pointer_key_scan.py --markdown docs/fork/POINTER-KEYS.md`; check the committed copy
without writing anything with
`python3 scripts/fork/pointer_key_scan.py --check docs/fork/POINTER-KEYS.md`; verify the matchers on
synthetic snippets with `python3 scripts/fork/pointer_key_scan.py --self-test`.

Total findings: 1245 (hash: 423, order: 365, map-key: 450, cast-mix: 7).

| kind | file:line | enclosing fn | matched line (<= 120 chars) | reach |
| --- | --- | --- | --- | ---: |
| order | crates/engine-core/src/mem/touch.rs:183 | forget | if o.end > addr { | 39 |
| hash | crates/engine-fallout-ai/src/units/fallout_ai/combatthreatmap.rs:252 | set_at | let bucket = e.vcall(this.addr(), 4, &args![key]).u32().wrapping_mul(4); | 30 |
| order | crates/engine-havok/src/units/havok_sdk/hkstreamstatisticscollector.rs:241 | new_node | while index < e.mem.i32(parent.addr() + 0x34) { | 25 |
| hash | crates/d3d9-stream/src/cmd.rs:85 | to_words | if b.as_ptr() as usize % 4 == 0 && b.len() % 4 == 0 { | 18 |
| hash | crates/engine-bs/src/units/bsmain/nibsbonelodcontroller.rs:643 | first_position | while bucket < e.get(map, NiTPointerMap::m_uiHashSize) { | 16 |
| hash | crates/engine-bs/src/units/bsmain/nibsbonelodcontroller.rs:644 | first_position | let table = e.get(map, NiTPointerMap::m_ppkHashTable); | 16 |
| hash | crates/engine-gamebryo/src/units/nimain/nistream.rs:1093 | ni_t_map_base_scalar_deleting_destructor | let table = e.get(this, NiTPointerMap::m_ppkHashTable); | 14 |
| map-key | crates/nvse/src/invref.rs:425 | inventory_items | let mut items: HashMap<u32, (i32, u32)> = HashMap::new(); | 9 |
| hash | crates/uio/src/build.rs:155 | map_insert | let slot = buckets.wrapping_add(bucket.wrapping_mul(4)); | 8 |
| map-key | crates/engine-core/src/profile.rs:1387 | other_threads | let mut group_of: HashMap<u32, String> = HashMap::new(); | 6 |
| order | crates/engine-bs/src/units/bsshader/bsshaderpplightingproperty_passes_2x.rs:137 | below_one | 1.0 > x87_m32(e.mem.u32(addr)) | 6 |
| hash | crates/engine-fallout-ai/src/units/fallout_ai/run_008443f0.rs:86 | construct_map_base | e.set(this, NiTMap::m_uiHashSize, buckets); | 4 |
| hash | crates/engine-fallout-ai/src/units/fallout_ai/run_008443f0.rs:88 | construct_map_base | let bytes = e.get(this, NiTMap::m_uiHashSize).wrapping_shl(2); | 4 |
| hash | crates/engine-fallout-ai/src/units/fallout_ai/run_008443f0.rs:90 | construct_map_base | e.set(this, NiTMap::m_ppkHashTable, table); | 4 |
| hash | crates/engine-fallout-ai/src/units/fallout_ai/run_008443f0.rs:91 | construct_map_base | let bytes = e.get(this, NiTMap::m_uiHashSize).wrapping_shl(2); | 4 |
| hash | crates/engine-fallout-ai/src/units/fallout_ai/run_008443f0.rs:92 | construct_map_base | let table = e.get(this, NiTMap::m_ppkHashTable); | 4 |
| hash | crates/engine-fallout-cell/src/units/fallout_shared/tesobjectcell_p4.rs:178 | construct_map_base | e.set(this, NiTPointerMap::m_uiHashSize, hash_size); | 4 |
| hash | crates/engine-fallout-cell/src/units/fallout_shared/tesobjectcell_p4.rs:182 | construct_map_base | e.set(this, NiTPointerMap::m_ppkHashTable, table); | 4 |
| hash | crates/engine-fallout-cell/src/units/fallout_shared/tesobjectcell_p4.rs:183 | construct_map_base | let size = e.get(this, NiTPointerMap::m_uiHashSize).wrapping_mul(4); | 4 |
| hash | crates/engine-fallout-cell/src/units/fallout_shared/tesworldspace.rs:3874 | map_base_construct | e.set(this, NiTPointerMap::m_uiHashSize, buckets); | 4 |
| hash | crates/engine-fallout-cell/src/units/fallout_shared/tesworldspace.rs:3877 | map_base_construct | e.set(this, NiTPointerMap::m_ppkHashTable, table); | 4 |
| hash | crates/engine-fallout-cell/src/units/fallout_shared/tesworldspace.rs:3878 | map_base_construct | let table = e.get(this, NiTPointerMap::m_ppkHashTable); | 4 |
| hash | crates/engine-fallout-forms/src/units/fallout_shared/bgssceneinfo.rs:360 | construct_map_base | e.set(this, NiTMap::m_uiHashSize, buckets); | 4 |
| hash | crates/engine-fallout-forms/src/units/fallout_shared/bgssceneinfo.rs:363 | construct_map_base | e.set(this, NiTMap::m_ppkHashTable, table); | 4 |
| hash | crates/engine-fallout-forms/src/units/fallout_shared/bgssceneinfo.rs:364 | construct_map_base | let table = e.get(this, NiTMap::m_ppkHashTable); | 4 |
| hash | crates/engine-fallout-forms/src/units/fallout_shared/bgssceneinfo.rs:365 | construct_map_base | let size = e.get(this, NiTMap::m_uiHashSize) << 2; | 4 |
| hash | crates/engine-fallout-forms/src/units/fallout_shared/tesfile.rs:2796 | construct_map_base | e.set(this, NiTPointerMap::m_uiHashSize, hash_size); | 4 |
| hash | crates/engine-fallout-forms/src/units/fallout_shared/tesfile.rs:2800 | construct_map_base | e.set(this, NiTPointerMap::m_ppkHashTable, buckets); | 4 |
| hash | crates/engine-fallout-forms/src/units/fallout_shared/tesfile.rs:2801 | construct_map_base | let buckets = e.get(this, NiTPointerMap::m_ppkHashTable); | 4 |
| hash | crates/engine-fallout-forms/src/units/fallout_shared/tesfile.rs:2802 | construct_map_base | let bytes = e.get(this, NiTPointerMap::m_uiHashSize).wrapping_shl(2); | 4 |
| hash | crates/engine-fallout-pathfinding/src/units/fallout_shared_pathfinding/navmeshobstaclemanager.rs:4732 | construct_map_base | e.set(this, NiTMap::m_uiHashSize, buckets); | 4 |
| hash | crates/engine-fallout-pathfinding/src/units/fallout_shared_pathfinding/navmeshobstaclemanager.rs:4736 | construct_map_base | e.set(this, NiTMap::m_ppkHashTable, table); | 4 |
| hash | crates/engine-fallout-pathfinding/src/units/fallout_shared_pathfinding/navmeshobstaclemanager.rs:4737 | construct_map_base | let bytes = e.get(this, NiTMap::m_uiHashSize) << 2; | 4 |
| hash | crates/engine-gamebryo/src/units/nimain/nistream.rs:1113 | ni_t_pointer_map_scalar_deleting_destructor | let table = e.get(this, NiTPointerMap::m_ppkHashTable); | 4 |
| order | crates/engine-typed/src/scripts/value.rs:150 | referenced_object | if e.mem.u32(this.addr() + REF_OBJECT_COUNT) < index { | 4 |
| hash | crates/engine-gamebryo/src/units/nianimation/nicontrollermanager.rs:1137 | ni_t_fixed_string_map_scalar_deleting_destructor | let table = e.get(this, NiTPointerMap::m_ppkHashTable); | 3 |
| hash | crates/engine-typed/src/lockfree/iface.rs:651 | lookup_word | let bucket = e.vcall(map.addr(), BUCKET_OF_SLOT, &args![key]).u32(); | 3 |
| map-key | crates/engine-core/src/profile.rs:641 | import_names | static NAMES: OnceLock<Mutex<HashMap<u32, String>>> = OnceLock::new(); | 3 |
| map-key | crates/jip/src/commands/jip_fn_minimap.rs:939 | world_markers | let mut out: BTreeMap<u32, HashMap<u32, Vec<MarkerInfo>>> = BTreeMap::new(); | 3 |
| hash | crates/engine-fallout-cell/src/units/fallout_shared/tesworldspace.rs:3937 | map_base_destroy | let table = e.get(this, NiTPointerMap::m_ppkHashTable); | 2 |
| hash | crates/engine-gamebryo/src/units/nixenonrenderer/run_00e6e8a0.rs:87 | destroy_map | let table = e.get(this, NiTPointerMap::m_ppkHashTable); | 2 |
| hash | crates/engine-typed/src/dispatch/saveload.rs:426 | queue_hash | key_low % e.mem.u32(this.addr() + MAP_SIZE) | 2 |
| hash | crates/engine-typed/src/lockfree/iface.rs:671 | insert_word | let bucket = e.vcall(map.addr(), BUCKET_OF_SLOT, &args![key]).u32(); | 2 |
| hash | crates/engine-typed/src/lockfree/iface.rs:686 | remove_string_map | let bucket = e.vcall(map.addr(), BUCKET_OF_SLOT, &args![key]).u32(); | 2 |
| hash | crates/engine-typed/src/shader/batch.rs:1434 | clear_renderer | let mut item = if e.get(this, BSBatchRenderer::RenderPassMap_m_uiHashSize) != 0 { | 2 |
| hash | crates/engine-typed/src/shader/batch.rs:1453 | clear_renderer | if index < e.get(this, BSBatchRenderer::RenderPassMap_m_uiHashSize) { | 2 |
| hash | crates/jip/src/game_patches/gl_lights.rs:325 | set_editor_id | let bucket = e.vcall(map, 4, &args![text]).u32(); | 2 |
| map-key | crates/engine-platform/src/d3d9device.rs:3992 | merge_drawing | let lights: std::collections::BTreeSet<u32> = main | 2 |
| map-key | crates/engine-platform/src/d3d9device.rs:4046 | merge_drawing | let samplers: std::collections::BTreeSet<u32> = main | 2 |
| map-key | crates/engine-platform/src/d3d9resource.rs:2305 | merge_drawing | let streams: std::collections::BTreeSet<u32> = main | 2 |
| map-key | crates/esm/src/cell_offsets.rs:176 | world_offsets | let mut ids: HashMap<u32, usize> = HashMap::new(); | 2 |
| map-key | crates/mod-archive/src/lib.rs:1151 | extract_with | let by_index: BTreeMap<usize, String> = chosen.iter().map(\|(k, &i)\| (i, k.clone())).collect(); | 2 |
| order | crates/engine-fallout-interface/src/units/fallout_interface/recipemenu.rs:3119 | fn_00728da0 | while e.mem.u32(cursor.addr()) != 0 && index < 0xc { | 2 |
| order | crates/engine-speedtree/src/units/speedtree/run_00b1e750.rs:22 | check_iterator | if this == 0 \|\| e.mem.u32(this + FIRST) > pointer \|\| pointer > e.mem.u32(this + LAST) { | 2 |
| hash | crates/engine-fallout-ai/src/units/fallout_ai/run_008443f0.rs:78 | destroy_map_base | let table = e.get(this, NiTMap::m_ppkHashTable); | 1 |
| hash | crates/engine-fallout-facegen/src/units/fallout_shared_facegen/bsfacegenmanager.rs:27 | key_to_hash_index | hash % e.mem.u32(this.addr() + 4) | 1 |
| hash | crates/engine-fallout-forms/src/units/fallout_shared/bgssceneinfo.rs:376 | destroy_map_base | let table = e.get(this, NiTMap::m_ppkHashTable); | 1 |
| hash | crates/engine-fallout-unplaced-a/src/units/unplaced/run_00486ab0.rs:31 | key_to_hash_index | hash % e.mem.u32(this.addr() + 4) | 1 |
| hash | crates/engine-gamebryo/src/units/nianimation/nibonelodcontroller.rs:270 | clone_lookup | let bucket = e.vcall(map, 4, &args![key]).u32(); | 1 |
| hash | crates/engine-typed/src/shader/batch.rs:1410 | map_item_from | let count = e.get(this, BSBatchRenderer::RenderPassMap_m_uiHashSize); | 1 |
| hash | crates/engine-typed/src/shader/batch.rs:1411 | map_item_from | let buckets = e.get(this, BSBatchRenderer::RenderPassMap_m_ppkHashTable); | 1 |
| hash | crates/jip/src/cosave.rs:678 | map_entry | let count = e.get(map, NiTPointerMap::m_uiHashSize); | 1 |
| hash | crates/jip/src/cosave.rs:682 | map_entry | let buckets = e.get(map, NiTPointerMap::m_ppkHashTable); | 1 |
| hash | crates/jip/src/cosave.rs:683 | map_entry | let mut entry = e.mem.u32(buckets + 4 * (id % count)); | 1 |
| hash | crates/jip/src/game_patches/gh2.rs:654 | update_water_textures | let mut entry = e.mem.u32(e.mem.u32(map + 8) + (key % buckets) * 4); | 1 |
| map-key | crates/nvinspect/src/vcall_words.rs:489 | table_words | let mut memo: HashMap<u32, Option<u32>> = HashMap::new(); | 1 |
| map-key | crates/nvinspect/src/vcall_words.rs:2583 | check_groups | let slots: BTreeSet<u32> = sites.iter().map(\|s\| s.slot).collect(); | 1 |
| map-key | crates/plugin-host/src/lib.rs:210 | code_reads | let mut by: std::collections::BTreeMap<usize, usize> = Default::default(); | 1 |
| order | crates/engine-bs/src/units/bsmenu/debugtext.rs:702 | fn_00a0df80 | if e.mem.u32(node.addr() + 4) < 2 { | 1 |
| order | crates/engine-core/src/extend.rs:1106 | check_table | i == 0 \|\| table[i - 1].addr < e.addr, | 1 |
| order | crates/engine-fallout-facegen/src/units/fallout_shared_facegen/bsfacegenkeyframe.rs:589 | positive_setting | let value = if x87_load32(e.mem.u32(address)) > zero { | 1 |
| order | crates/engine-fallout-interface/src/units/fallout_interface/specialbookmenu.rs:690 | change_value | < e.mem.i32(this.addr() + AVAILABLE_POINTS) | 1 |
| order | crates/engine-havok/src/units/havok_sdk/hkstreamstatisticscollector.rs:462 | new_command | let address = if e.mem.i32(array + 4) > 0 { | 1 |
| order | crates/engine-typed/src/facegen/keyframe.rs:237 | positive_setting | if e.mem.f32(address) > 0.0 { | 1 |
| cast-mix | crates/engine-fallout-forms/src/units/fallout_shared/tessaveloadgame_p2.rs:1760 | ni_t_map_base_unsigned_char_bs_simple_list_load_form_header_p_key_to_hash_index | key as u32 % hash_size | 0 |
| cast-mix | crates/engine-fallout-interface/src/units/fallout_interface/computersmenu.rs:769 | computers_menu_do_click | tile = word(e, this, (id as u32).wrapping_mul(4).wrapping_add(0x28)); | 0 |
| cast-mix | crates/engine-fallout-interface/src/units/fallout_interface/sleepwaitmenu.rs:18 | sleep_wait_menu_attach_tile_by_id | .wrapping_add((id as u32).wrapping_mul(4)); | 0 |
| cast-mix | crates/engine-fallout-pathfinding/src/units/fallout_shared_pathfinding/navmesh.rs:535 | ni_t_map_base_key_to_hash_index | key as u32 % hash_size | 0 |
| cast-mix | crates/engine-fallout-pathfinding/src/units/fallout_shared_pathfinding/navmesh_util.rs:3239 | ni_t_map_fast_vertex_key_to_hash_index | (hash as u32) % e.mem.u32(this.addr() + 4) | 0 |
| cast-mix | crates/engine-fallout-unplaced-a/src/units/unplaced/run_0044dec0.rs:151 | fn_0044e050 | .vcall(this.addr(), 0x24, &args![key as u32, (key >> 32) as u32]) | 0 |
| cast-mix | crates/engine-fallout-unplaced-a/src/units/unplaced/run_0044dec0.rs:154 | fn_0044e050 | if e.vcall(this.addr(), 0x14, &args![key as u32, (key >> 32) as u32]) | 0 |
| hash | crates/engine-bs/src/units/bsaudio/bsaudiomanager.rs:800 | map_first_item | let buckets = e.get(map, NiTPointerMap::m_uiHashSize); | 0 |
| hash | crates/engine-bs/src/units/bsaudio/bsaudiomanager.rs:801 | map_first_item | let table = e.get(map, NiTPointerMap::m_ppkHashTable); | 0 |
| hash | crates/engine-bs/src/units/bsaudio/bsaudiomanager.rs:4171 | map_constructor | e.set(this, NiTPointerMap::m_uiHashSize, buckets); | 0 |
| hash | crates/engine-bs/src/units/bsaudio/bsaudiomanager.rs:4175 | map_constructor | e.set(this, NiTPointerMap::m_ppkHashTable, table); | 0 |
| hash | crates/engine-bs/src/units/bsaudio/bsaudiomanager.rs:4213 | fn_00ae7cc0 | let table = e.get(this, NiTPointerMap::m_ppkHashTable); | 0 |
| hash | crates/engine-bs/src/units/bsaudio/bsaudiomanager.rs:4232 | fn_00ae7cc0 | let table = e.get(this, NiTPointerMap::m_ppkHashTable); | 0 |
| hash | crates/engine-bs/src/units/bsaudio/bsaudiomanager.rs:4249 | fn_00ae7e60 | let table = e.get(this, NiTPointerMap::m_ppkHashTable); | 0 |
| hash | crates/engine-bs/src/units/bsaudio/bsaudiomanager.rs:4286 | fn_00ae7ee0 | let mut bucket = e.vcall(this.addr(), MAP_SLOT_HASH, &[key]).u32(); | 0 |
| hash | crates/engine-bs/src/units/bsaudio/bsaudiomanager.rs:4287 | fn_00ae7ee0 | let size = e.get(this, NiTPointerMap::m_uiHashSize); | 0 |
| hash | crates/engine-bs/src/units/bsaudio/bsaudiomanager.rs:4288 | fn_00ae7ee0 | let table = e.get(this, NiTPointerMap::m_ppkHashTable); | 0 |
| hash | crates/engine-bs/src/units/bsaudio/run_00ae91f0.rs:32 | fn_00ae91f0 | while bucket < e.get(this, NiTPointerMap::m_uiHashSize) { | 0 |
| hash | crates/engine-bs/src/units/bsaudio/run_00ae91f0.rs:35 | fn_00ae91f0 | .get(this, NiTPointerMap::m_ppkHashTable) | 0 |
| hash | crates/engine-bs/src/units/bsaudio/run_00ae91f0.rs:43 | fn_00ae91f0 | .get(this, NiTPointerMap::m_ppkHashTable) | 0 |
| hash | crates/engine-bs/src/units/bscore/bsblockmemheap.rs:16 | bs_map_base_block_mem_page_set_at | let bucket = e.vcall(base, 0x4, &args![key]).u32(); | 0 |
| hash | crates/engine-bs/src/units/bscore/run_00aa45a0.rs:40 | (module) | 0x04 m_uiHashSize: u32, | 0 |
| hash | crates/engine-bs/src/units/bscore/run_00aa45a0.rs:42 | (module) | 0x08 m_ppkHashTable: u32, | 0 |
| hash | crates/engine-bs/src/units/bscore/run_00aa45a0.rs:147 | fn_00aa47e0 | e.set(this, ScrapHeapMap::m_uiHashSize, hash_size); | 0 |
| hash | crates/engine-bs/src/units/bscore/run_00aa45a0.rs:150 | fn_00aa47e0 | let bytes = e.get(this, ScrapHeapMap::m_uiHashSize).wrapping_shl(2); | 0 |
| hash | crates/engine-bs/src/units/bscore/run_00aa45a0.rs:154 | fn_00aa47e0 | e.set(this, ScrapHeapMap::m_ppkHashTable, table); | 0 |
| hash | crates/engine-bs/src/units/bscore/run_00aa45a0.rs:155 | fn_00aa47e0 | let bytes = e.get(this, ScrapHeapMap::m_uiHashSize).wrapping_shl(2); | 0 |
| hash | crates/engine-bs/src/units/bscore/run_00aa45a0.rs:156 | fn_00aa47e0 | let table = e.get(this, ScrapHeapMap::m_ppkHashTable); | 0 |
| hash | crates/engine-bs/src/units/bsmain/iomanager.rs:814 | fn_00c3fb50 | let bucket = e.vcall(this.addr(), 0x24, &args![low, high]).u32(); | 0 |
| hash | crates/engine-bs/src/units/bsmain/iomanager.rs:1525 | fn_00c41ac0 | let bucket_slot = buckets.wrapping_add(bucket.wrapping_mul(4)); | 0 |
| hash | crates/engine-bs/src/units/bsmain/iomanager.rs:1607 | fn_00c42380 | let slot = buckets.wrapping_add(bucket.wrapping_mul(4)); | 0 |
| hash | crates/engine-bs/src/units/bsmain/nibsbonelodcontroller.rs:659 | fn_00a54060 | e.set(this, NiTPointerMap::m_uiHashSize, hash_size); | 0 |
| hash | crates/engine-bs/src/units/bsmain/nibsbonelodcontroller.rs:664 | fn_00a54060 | let size = e.get(this, NiTPointerMap::m_uiHashSize).wrapping_mul(4); | 0 |
| hash | crates/engine-bs/src/units/bsmain/nibsbonelodcontroller.rs:665 | fn_00a54060 | e.set(this, NiTPointerMap::m_ppkHashTable, table); | 0 |
| hash | crates/engine-bs/src/units/bsmain/nibsbonelodcontroller.rs:679 | fn_00a540a0 | let table = e.get(this, NiTPointerMap::m_ppkHashTable); | 0 |
| hash | crates/engine-bs/src/units/bsmain/run_00c3f160.rs:20 | (module) | const SLOT_BUCKET: u32 = 0x24; | 0 |
| hash | crates/engine-bs/src/units/bsmain/run_00c3f160.rs:254 | fn_00c3f4c0 | .vcall(object, SLOT_BUCKET, &args![key_low, key_high]) | 0 |
| hash | crates/engine-bs/src/units/bsmain/run_00c3f160.rs:269 | fn_00c3f650 | key_low % e.mem.u32(this.addr().wrapping_add(8)) | 0 |
| hash | crates/engine-bs/src/units/bsmovie/movieplayer_xenon.rs:532 | bst_case_insensitive_string_map_key_to_hash_index | hash % e.mem.u32(this.addr() + 4) | 0 |
| hash | crates/engine-bs/src/units/bsmovie/movieplayer_xenon.rs:948 | ni_t_string_template_map_key_to_hash_index | hash % e.mem.u32(this.addr() + 4) | 0 |
| hash | crates/engine-bs/src/units/bsshader/bsbatchrenderer.rs:17 | (module) | 0x10 RenderPassMap_m_uiHashSize: u32, | 0 |
| hash | crates/engine-bs/src/units/bsshader/bsbatchrenderer.rs:19 | (module) | 0x14 RenderPassMap_m_ppkHashTable: u32, | 0 |
| hash | crates/engine-bs/src/units/bsshader/bsbatchrenderer.rs:146 | fn_00b9a5b0 | e.set(this, BSBatchRenderer::RenderPassMap_m_uiHashSize, 0x25); | 0 |
| hash | crates/engine-bs/src/units/bsshader/bsbatchrenderer.rs:149 | fn_00b9a5b0 | let hash_size = e.get(this, BSBatchRenderer::RenderPassMap_m_uiHashSize); | 0 |
| hash | crates/engine-bs/src/units/bsshader/bsbatchrenderer.rs:150 | fn_00b9a5b0 | e.set(this, BSBatchRenderer::RenderPassMap_m_ppkHashTable, buckets); | 0 |
| hash | crates/engine-bs/src/units/bsshader/bsbatchrenderer.rs:325 | fn_00b99bc0 | let count = e.get(this, BSBatchRenderer::RenderPassMap_m_uiHashSize); | 0 |
| hash | crates/engine-bs/src/units/bsshader/bsbatchrenderer.rs:327 | fn_00b99bc0 | let buckets = e.get(this, BSBatchRenderer::RenderPassMap_m_ppkHashTable); | 0 |
| hash | crates/engine-bs/src/units/bsshader/bsbatchrenderer.rs:341 | fn_00b99bc0 | let count = e.get(this, BSBatchRenderer::RenderPassMap_m_uiHashSize); | 0 |
| hash | crates/engine-bs/src/units/bsshader/bsbatchrenderer.rs:343 | fn_00b99bc0 | let buckets = e.get(this, BSBatchRenderer::RenderPassMap_m_ppkHashTable); | 0 |
| hash | crates/engine-bs/src/units/bsshader/bsshaderaccumulator.rs:218 | bs_shader_accumulator_bs_shader_accumulator | e.set(map, NiTPointerMap::m_uiHashSize, 0x25); | 0 |
| hash | crates/engine-bs/src/units/bsshader/bsshaderaccumulator.rs:221 | bs_shader_accumulator_bs_shader_accumulator | let hash_size = e.get(map, NiTPointerMap::m_uiHashSize); | 0 |
| hash | crates/engine-bs/src/units/bsshader/bsshaderaccumulator.rs:222 | bs_shader_accumulator_bs_shader_accumulator | e.set(map, NiTPointerMap::m_ppkHashTable, buckets); | 0 |
| hash | crates/engine-bs/src/units/bsshader/bsshaderaccumulator.rs:886 | fn_00b63770 | &[renderer, pass_slot, bucket, first, 0], | 0 |
| hash | crates/engine-bs/src/units/bsshader/bsshaderaccumulator.rs:892 | fn_00b63770 | &[renderer, pass_slot, bucket, first, second], | 0 |
| hash | crates/engine-bs/src/units/bsshader/run_00b57e70.rs:124 | fn_00b57f20 | let size = e.get(this, NiTPointerMap::m_uiHashSize); | 0 |
| hash | crates/engine-bs/src/units/bsshader/run_00b57e70.rs:126 | fn_00b57f20 | let table = e.get(this, NiTPointerMap::m_ppkHashTable); | 0 |
| hash | crates/engine-bs/src/units/bsshader/run_00b57e70.rs:184 | fn_00b580a0 | let table = e.get(this, NiTPointerMap::m_ppkHashTable); | 0 |
| hash | crates/engine-bs/src/units/bsshader/run_00b57e70.rs:200 | fn_00b582b0 | let bucket = e.vcall(map, SLOT_KEY_TO_HASH_INDEX, &args![key]).u32(); | 0 |
| hash | crates/engine-bs/src/units/bsshader/run_00b57e70.rs:201 | fn_00b582b0 | let table = e.get(this, NiTPointerMap::m_ppkHashTable); | 0 |
| hash | crates/engine-bs/src/units/bsshader/run_00b57e70.rs:230 | fn_00b582b0 | let table = e.get(this, NiTPointerMap::m_ppkHashTable); | 0 |
| hash | crates/engine-bs/src/units/bsshader/run_00b57e70.rs:231 | fn_00b582b0 | let slot = table.wrapping_add(bucket.wrapping_mul(4)); | 0 |
| hash | crates/engine-bs/src/units/bsshader/run_00b57e70.rs:234 | fn_00b582b0 | let table = e.get(this, NiTPointerMap::m_ppkHashTable); | 0 |
| hash | crates/engine-bs/src/units/bsshader/run_00b595b0.rs:572 | fn_00b596a0 | let low_cell = half.wrapping_sub(low % size).wrapping_add(position as i32) as u32; | 0 |
| hash | crates/engine-bs/src/units/bsshader/run_00b60a50.rs:359 | fn_00b61010 | let slot = buckets.wrapping_add(index.wrapping_mul(4)); | 0 |
| hash | crates/engine-core/src/types.rs:97 | (module) | 0x04 m_uiHashSize: u32, | 0 |
| hash | crates/engine-core/src/types.rs:99 | (module) | 0x08 m_ppkHashTable: u32, | 0 |
| hash | crates/engine-fallout-ai/src/units/fallout_ai/combatmanager.rs:669 | ni_t_map_base_actor_int64_set_at | let bucket = e.vcall(me, 4, &args![key]).u32(); | 0 |
| hash | crates/engine-fallout-ai/src/units/fallout_ai/combatmanager.rs:670 | ni_t_map_base_actor_int64_set_at | let table = e.get(this, NiTPointerMap::m_ppkHashTable); | 0 |
| hash | crates/engine-fallout-ai/src/units/fallout_ai/combatmanager.rs:683 | ni_t_map_base_actor_int64_set_at | let table = e.get(this, NiTPointerMap::m_ppkHashTable); | 0 |
| hash | crates/engine-fallout-ai/src/units/fallout_ai/combatmanager.rs:684 | ni_t_map_base_actor_int64_set_at | let slot = table.wrapping_add(bucket.wrapping_mul(4)); | 0 |
| hash | crates/engine-fallout-ai/src/units/fallout_ai/combatmanager.rs:687 | ni_t_map_base_actor_int64_set_at | let table = e.get(this, NiTPointerMap::m_ppkHashTable); | 0 |
| hash | crates/engine-fallout-ai/src/units/fallout_ai/combatmanager.rs:700 | fn_00993a00 | let bucket = e.vcall(me, 4, &args![key]).u32(); | 0 |
| hash | crates/engine-fallout-ai/src/units/fallout_ai/combatmanager.rs:701 | fn_00993a00 | let table = e.get(this, NiTPointerMap::m_ppkHashTable); | 0 |
| hash | crates/engine-fallout-ai/src/units/fallout_ai/combatmanager.rs:742 | ni_t_map_base_actor_int64_get_next | let mut bucket = e.vcall(this.addr(), 4, &args![item_key]).u32(); | 0 |
| hash | crates/engine-fallout-ai/src/units/fallout_ai/combatmanager.rs:744 | ni_t_map_base_actor_int64_get_next | while bucket < e.get(this, NiTPointerMap::m_uiHashSize) { | 0 |
| hash | crates/engine-fallout-ai/src/units/fallout_ai/combatmanager.rs:745 | ni_t_map_base_actor_int64_get_next | let table = e.get(this, NiTPointerMap::m_ppkHashTable); | 0 |
