# Renderer stage 1 (`NV_DRAW_ITEMS`) with the detached draw

Fork-only note on upstream's renderer stage 1 (slaterain/nv-rs#1108, commit `605e759ac25a5f7d64c93b373bd5859e653aed0b`):
the world's still lit objects are recorded once with their inputs and drawn again as one item while the inputs stay the
same. Off by default (`sDrawItems` / `NV_DRAW_ITEMS`); off, nothing of it runs and the stream is the one of the build
before it, apart from the 16 byte header (the command list's hash).

## Collisions and how they combine

| Where | Upstream | Fork | Combined |
|---|---|---|---|
| `engine-typed/src/draw/drivers.rs` | `keep::world_draw` brackets the world pass's two halves; `scene_draw` runs inside `outside_world_draw` | the half draws from a posted packet (`capture::serve`), the passes' own blocks replace the join in `scene_draw` (nv-rs#992) | both: the keep brackets sit inside the packet guard; `scene_draw` loses the join as on the fork and keeps the bracket. A draw is kept only while no value packet is open (`keep::draw`), so with the drawing thread's packet the draws are plain |
| `engine-typed/src/render/stage.rs` | `bind` is a leaf the record makes again (`bind_now`) | the stage's fields are read through the draw's own blocks (`capture::own_get`) | `bind_now` keeps the fork's own-block reads; the leaf wrapper is upstream's |
| `engine-core/src/mem.rs` | the store log and the read audit | `HeapStats` (the heap's counters) | both, side by side (different jobs, no shared code) |
| `engine-platform` `skip_repeats_in` | a check run keeps states set again to their value | repeats are skipped unless the cut is open and not the drawing side | one condition: kept when asked, or a check run |
| `d3d9-wgpu/tests/frame.rs` | the kept record tests | the managed level and upload tests | both sets |

## What was checked

On `docs/fork/scenes/bench-quick.toml` (goodsprings and docs-house, stepped threads so that a stream repeats), the
branch below this one against this one:

- off (default): byte-identical after the 16 byte header; control (baseline twice) byte-identical.
- `NV_DRAW_ITEMS=1`: 0 stops, 0 error lines; 797,514 of the first million draws replayed as items, 12,735 built, 0 plain,
  0 items without a record.
- `NV_DRAW_ITEMS=1` with the stepped drawing thread (`NV_THREADS=stepped NV_DRAW_THREAD=1 NV_DRAW_THREAD_CHECK=1`):
  byte-identical to the same run with items off: the packet is open, so every draw is plain; the two coexist.
- `NV_DRAW_ITEMS=1` with `NV_DRAW_DETACH=1` (no thread): runs, 0 stops; records are built (the stream holds `RecordNext`).
- Speed, 1,100 frames each, stepped threads (noise 5 to 10 %): off 19.4 / 14.4 ms and 17.9 / 12.1 ms, items on
  19.6 / 13.0 ms and 19.2 / 12.6 ms (goodsprings / docs-house). No measurable gain in stepped mode.

## Known problem, why it stays off

`NV_DRAW_ITEMS=check` (each draw as the original draws it, beside the item it would have been) reports differences on
this fork: in 900 frames 799,424 draws checked, 100,906 differed. The first: a `SetSamplerState`-like device call
(slot 0x178) with another first argument in the kept record than in the set-up; the replay's digests (both
`record_compare --items` and `replay_digests` with `NV_DIGESTS_ITEMS=1`) differ at the same draw (a pixel shader
constant). It does not go away with `NV_PRESET=vanilla` or `NV_TYPED=lockfree:off`. Upstream states 0 differences for
its own tree, so a fork change that the record does not know as an input (the fork's sampler and draw state code) is the
suspect. The picture check that decides the switch has therefore not passed here; the switch stays off, as upstream's
does, and the check is the first thing to run before anything turns it on.
