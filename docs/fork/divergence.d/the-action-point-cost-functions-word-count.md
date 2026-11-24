### The action-point cost function's word count
- Files: `crates/engine-fallout-forms/src/units/fallout_shared/actionpoints.rs`
- Upstream approach: `fn_0066dca0(i32, bool)` is registered with two words, while the game's cdecl function is called with four (`0093ab30`, lStewieAl's Tweaks' run-and-gun in `crates/stewie/src/run_and_gun.rs`), so `engine/tests/jip_call_arity.rs` (`stewie_passes_each_game_function_the_words_it_takes`) fails on upstream's own `main` (replay of 2c7100c7)
- Fork approach: the entry is `fn_0066dca0(i32, bool, f32, u32)`; the two extra words are not read
- Decision: drop when upstream's entry takes four words, or its callers pass two where the game's function takes two
- Reason: keep the word-count guard green in the fork's Windows integration job without weakening the guard
- Evidence: Windows job `aggregator: engine lib, doctests and par_render, host, play-patch, viewer` of the sync pull request, `stewie: wrong word counts: run_and_gun.rs:47`
