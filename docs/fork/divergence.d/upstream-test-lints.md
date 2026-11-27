### Lints on upstream's tests
- Files: `crates/engine/tests/typed_ai_middle_low_process.rs`
- Upstream approach: its checks do not run clippy on test targets, so a comparison that is always true (`& 0xff != 0x100`, slaterain/nv-rs#1089) stays
- Fork approach: `#[allow(clippy::bad_bit_mask)]` on the test function; the test body is as upstream wrote it
- Decision: keep; drop when upstream fixes the comparison or lints its tests
- Reason: the fork's pipeline runs `clippy --all-targets -D warnings`
- Evidence: `cargo clippy -p engine --all-targets -- -D warnings` fails without it (`bad_bit_mask`)
