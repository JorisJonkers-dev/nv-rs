# Shader and pipeline caching

Three layers, all keyed by content, none by file name, mod, object, texture, path or load order.

| Layer | What | Where | Key | Code |
| --- | --- | --- | --- | --- |
| (a) converted modules | Direct3D 9 bytecode linked as the device links it, converted to WGSL | `<root>/modules/<stamp>/xx/<key>.wgsl` | 128-bit hash of converter stamp, bytecode bytes, link | `d3d9-wgpu/src/shader_cache.rs` |
| (b) driver pipeline cache | wgpu's `PipelineCache` blob (Vulkan: `VkPipelineCache` data, validated by wgpu's header: vendor, device, driver, pipeline-cache UUID) | `<root>/pipelines/wgpu_pipeline_cache_*-<fingerprint>.bin` | adapter key plus the crate's source fingerprint | `d3d9-wgpu/src/cache.rs` |
| (c) pipeline list | the pipeline state combinations seen in play (shader pair by bytecode hash, vertex declaration, render state, target formats), shipped list plus this machine's recordings | `<root>/pipelines/pipelines.txt` | bytecode hashes | `d3d9-wgpu/src/prepare.rs` |

`<root>` is `$XDG_CACHE_HOME/nv-rs/shaders`, else `~/.cache/nv-rs/shaders` (Windows: `%LOCALAPPDATA%\nv-rs\shaders`).
Before this work the Linux run kept layers (b) and (c) nowhere (only the Windows launcher set `NV_WGPU_CACHE`); they now
default to `<root>/pipelines`. `NV_WGPU_CACHE` still names another folder for (b) and (c).

## Switches

- `NV_SHADER_CACHE=off` (also `0`, `none`, `false`) turns the default store off (an explicit `NV_WGPU_CACHE` stays);
  `NV_SHADER_CACHE=/path` moves the root.
- `NV_SHADER_CACHE_MAX_MB=N` caps layer (a) (default 1024). Over the cap, the least recently used entries (a hit
  refreshes the file's modified time) are removed down to nine tenths of it when the cache is opened.

## Why a mod does not invalidate anything

The game creates every shader through `CreateVertexShader` / `CreatePixelShader` with the bytecode in hand, whatever file
it was read from (a shader package the base game ships, a loose replacement a mod ships in `Data\Shaders`, a package a
mod adds, shaders a render injector or script extender compiles and hands to the device at run time). The cache sees
the bytecode only. So:

- a mod that replaces a shader changes that one hash: one new entry, every other entry still hits;
- two mods that ship the same shader share one entry;
- adding, removing or reordering mods leaves entries of shaders no longer used to age out by the cap;
- textures, meshes, objects and paths never enter a key.

Layer (b) is per GPU and driver (wgpu refuses a blob of another adapter, backend or driver, and the file name carries
the adapter), so a driver update is one cold start, not a crash. Layer (c) is per bytecode hash and per machine's play.

- `NV_PIPELINE_CACHE_MAX_MB=N` (default 256) caps the pipeline folder: the driver cache files of other adapters go
  oldest first until the `wgpu_pipeline_cache_*.bin` files fit (this run's file is never removed), and the recorded
  part of `pipelines.txt` is held to an eighth of it (at least 1 MB), oldest recorded lines dropped first; the shipped
  lines always stay.

## Why there is no SPIR-V layer

The vendored `wgpu-hal` Vulkan backend always makes shader modules as intermediate modules
(`Workarounds::SEPARATE_ENTRY_POINTS`, set for every device): the module holds naga's validated IR and the SPIR-V is
written per pipeline stage, with that pipeline layout's binding map. So the "modules" thread time at the start is
wgpu-core parsing and validating WGSL, and the SPIR-V is written inside pipeline-library creation. SPIR-V cached per
shader would not match the per-layout words, and passing it as a raw (passthrough) module would skip the binding map:
not safe. It was tried (capture at module creation: no SPIR-V is written there) and dropped.

## Stamp, validation, safety

- The stamp of (a) is a fingerprint of `crates/d3d9-wgpu/src/sm/` (`build.rs`), so a converter change opens a new folder
  and removes the old ones; a change elsewhere in the crate keeps layer (a). Layer (b) keeps its wider fingerprint
  (converter, layouts, vendored `wgpu-hal`, `Cargo.lock`).
- An entry is magic, version, its key, length and a hash of the text. A missing, short, altered or foreign entry is a
  miss; a damaged one is removed, counted and logged at warn. A folder that cannot be made, or a failed write, is one
  warn line and no cache for the run, never a failed start.
- Writes go to a temporary of the writer's own and are renamed, so concurrent processes see the old entry or the new
  one; two processes writing one key write the same bytes. Stale temporaries are removed at open.
- Counters (hits, misses, stored, damaged, evicted, bytes, I/O time) are logged at the end of the run:
  `NV_D3D9 wgpu: shader cache <root>: N hits, M misses (P% hit), ...`.

## Precompiling a load order

`nv-viewer DATA --precompile-shaders [--shader-root PATH]...` (or `d3d9-precompile [--shader-cache DIR] [--pipelines DIR]
ROOT...`) fills layers (a) and (b) for a load order without starting the game. Roots are Data folders (or shader package
files), lowest priority first; the Data folder comes first, each `--shader-root` after it. Packages are found as the game
finds them (`Shaders/*.sdp`, any case); a package of the same name in a later root replaces the earlier one, as a mod's
loose file does. The shaders of the surviving packages are deduplicated by content, created on an offscreen device,
and made on every core with their pipeline libraries (a precompile makes the unlisted shaders' libraries too, which
the game's warm start leaves to the driver's cache); the driver cache and list are written. It is incremental by
construction: a hash the store and the driver cache hold costs a lookup. It ends with the counts and the cache summary.
Shaders that mods add at run time through render injectors reach the device the same way and are cached on their first
run; the pass covers what the game loader finds in packages. Launching with a warm-up is the Toolbox's job
(`raul run`, the Toolbox's ADR-0007): it is a thin caller of the viewer flag with the load order's Data roots; until
then `nvmod run --warm` (nvmod-rs) does the same.

Measured on the vanilla install (16 packages, 15,791 shaders, 1,809 distinct bytecodes), AMD RDNA 3 (RADV), 16 cores:

| Run | Time | Store hits / misses |
| --- | --- | --- |
| empty caches | 2.51 s | 0 / 1809 |
| again | 0.64 s | 1809 / 0 |
| a mod root replacing package 002 with 20 shaders, 3 of them changed | 0.64 s | 1797 / 3 (3 stored) |
| the same mod root again | 0.63 s | 1800 / 0 |

## Measured

`nv-rs-dev bench run newgame --nested headless --resolution 1920x1080`, AMD RDNA 3 (RADV) (RADV), 16 cores, the vanilla
game, 765 shaders; the first row's run was marked noisy by the bench (another viewer overlapped), the others were not. Start numbers are the viewer log's `the start:` line.

| Run | Start (wall, 16 cores) | Threads: converting / modules / libraries | First picture | Play 1% low |
| --- | --- | --- | --- | --- |
| Linux before: no layer (b), (c) or (a) kept | 88 ms | 109 / 2582 / 784 ms | 784 ms | 81.5 fps |
| all three on, first run (cold) | 78 ms | 444 / 1887 / 340 ms | 631 ms | 74.0 fps |
| all three on, second run (warm) | 41 ms | 35 / 1928 / 43 ms | 594 ms | 78.5 fps |

What this shows. The pipeline cache and list (layers (b), (c)) were not kept at all on Linux; keeping them takes the
pipeline libraries from 340 to 43 ms of thread time and the start from 78 to 41 ms. The converted-module store (a)
saves about 60 ms of thread time on 765 shaders (35 ms against 92 to 109 ms of converting), under 5 ms of wall time:
converting WGSL is not the cost. The cost is `modules`, about 1.9 s of thread time: wgpu parses and validates the WGSL
and writes SPIR-V for every shader, and none of that is kept yet. The whole shader stage is 40 to 80 ms of a first
picture at about 600 ms, and the bench showed no present over 20 ms in play with or without the caches. A load order with
thousands of mod shaders scales the stage linearly, which is where the store and a later SPIR-V layer pay.
