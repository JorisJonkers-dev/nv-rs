# The address space: what is locked, what is measured, what the fork can unlock

Status: 2026-11-16, measured on the local loop (`nv-rs-dev bench`, offscreen, Linux, play profile) at `fork` 42b195d3.
Numbers only; no game data. Upstream's current state was read from the local clone (read only).

## 1. The model

The game code is the 32-bit `FalloutNV.exe`, translated to Rust and run on a 64-bit host inside one guest address space
(`engine-core/src/mem.rs`). Every guest pointer is a `u32`, so **the guest can never hold more than 4 GiB, whatever the
host has.** Only moving data out of the guest (host memory behind a handle) lifts that, which is the typed migration,
upstream's work ([ADR-0012](../adr/0012-modern-64-bit-engine.md), [ADR-0014](../adr/0014-typed-migration-in-two-stages.md)).

The exe's own large-address-aware bit does not matter here: the port always gives the guest the 4 GiB window of an LAA
process on 64-bit Windows, and `GetSystemInfo` answers 0xFFFEFFFF. A patched or unpatched exe behaves the same.

### What each part of the 4 GiB is for (from `mem.rs`)

| Range | Bytes | Use | Mapped by |
| --- | --- | --- | --- |
| `0x00400000..0x01409000` | 16 MB | exe image | the loader |
| `0x01410000..0x10000000` | 240 MB | `VirtualAlloc` reservations (scrap heaps' 8 MiB buffers; `NV_RESERVE_GAP`) | on demand |
| `0x10000000..0x20000000` | 256 MB | stage 2 of the typed migration: slab handles, buffer tokens (128 MB never mapped), 16-byte stubs | on demand |
| `0x20000000..0x7E000000` | 1.47 GiB | heap, lower range | heap |
| `0x7E000000..0x80000000` | 32 MB | platform addresses (COM code, helper stacks, built-in extenders) | platform |
| `0x80000000..0xF0000000` | 1.75 GiB | heap, upper range (used only when the lower cannot serve) | heap |
| `0xF0000000..0xFFFF0000` | 255 MB | freed-block check's words (`freed_word`), never mapped | nothing |
| last 64 KB | 64 KB | poison for moved fields, never mapped | nothing |

The heap is therefore 3,456,106,496 bytes (3.22 GiB; `heap_cap` in `NV_MEMLOG`), the largest single block 1.75 GiB.
Reservations are `mmap(MAP_NORESERVE)`, so the 4 GiB costs address space only (the host's `vm.overcommit_memory` does not
matter).

## 2. Measured

`NV_MEMLOG=1` on the offscreen scenarios (`boot`, `newgame`, `probe`; 56 recorded runs, the newest at 2026-11-16):

| Number | Value |
| --- | --- |
| Guest heap in use, main menu | about 87 MB (`heap_used`); `heap_top` 632,521,120, i.e. 96 MB above `HEAP_BASE` |
| Guest heap peak, first room after new game | 201 MB (`heap_peak`), highest address reached 775,473,672 (0x2E3A...) |
| Heap capacity used at that peak | 5.8% of 3.46 GB; the upper range (`0x80000000..`) is never touched |
| Host RSS | 590 MB at the menu, peak 1.16 GB in the first room |
| Host RSS not in the guest heap | about 0.85 to 1.0 GB: exe image mapping (77 MB), the GPU layer's kept CPU-side copies (85 MB at 4,348 levels, `gpu_kept_bytes`), wgpu/Vulkan driver memory, helper threads, textures and buffers already held outside the guest |
| Guest reservation | one 4,097 MiB mapping, of which 106 MiB resident at the menu |
| Process virtual size (`VmSize`) | 7.1 GB at the menu (`vm_size=7,097,483,264`): the 4 GiB guest, a 960 MiB anonymous mapping (not attributed) and the rest; irrelevant on a 64-bit host |
| `rss_anon` | 482 MB of the 613 MB resident at the menu (the rest, 131 MB, is mapped files). Both keys added to `NV_MEMLOG` by this change (appended keys, Linux) so that this split is read without `/proc` by hand |

Not measured: a heavily modded or Tale of Two Wastelands load. Nothing on this machine runs one yet, so **the point at
which the 3.46 GB heap binds is unknown**; vanilla routes use 6% of it. `heap_peak`, `heap_peak_addr` and
`heap_largest_free` in `NV_MEMLOG` are the numbers to read on such a run: pressure shows as `heap_peak_addr` passing
0x7E000000 (the upper range in use), then `heap_largest_free` falling. A stop on exhaustion prints the live blocks by size
(`Mem::heap_exhausted`).

## 3. What is locked, and what overlaps upstream

| Limit | Locked by | Upstream |
| --- | --- | --- |
| Guest pointers are 32 bits: 4 GiB total | the translated exe | Lifted only by the typed migration (README "Native 64-bit code: Started"; ten typed families on). Upstream's work and in flight; the fork imports it, does not duplicate it. |
| Heap 3.46 GB of those 4 GiB | `HEAP_BASE`, `HEAP_LOW_END`, `HEAP_HIGH_BASE`, `HEAP_END` constants | Upstream's stopgap (branch `claude/heap-4gb`, described in docs/HANDOFF.md). Unchanged since. |
| Textures, vertex and index buffers, sound samples | now host memory outside the guest | Done upstream, the main host-side relocation so far |
| Slab handles and buffer tokens for moved objects | 256 MB reserved in `0x10000000..0x20000000` | Upstream (stage 2 of the typed migration), still growing family by family |
| Renderer's CPU-side copies | `d3d9-wgpu` (`gpu_kept_bytes`, 85 MB) | Host side already; it is 64-bit and not bounded by the guest |

## 4. Options, ranked by gain over risk

1. **Measure first (done here).** `NV_MEMLOG` now also reports `vm_size` and `rss_anon`, so a long modded run shows, in one
   place, guest heap against anonymous host memory against virtual size. Gain: it settles whether the limit binds. Risk:
   none (read only, appended keys, off by default).
2. **Move more guest data to the host (upstream's typed migration).** The only change that removes the limit rather than
   spending the same 4 GiB. Every family moved takes its blocks out of the guest heap. Highest gain, not the fork's to
   duplicate; the useful fork contribution is the measuring and the benchmark harness (scenarios that reach heavy cells with
   many plugins, once a modded instance exists).
3. **Widen the heap into the freed-word region (`HEAP_END` 0xF0000000 to about 0xFFF00000).** +255 MB (+7%) of heap, and
   the only guest range that is spare. Cost: the freed-block check's words (`freed_word(addr) = 0xF0000000 | addr >> 4`) and
   stage 2's freed-slab words live there by design and must stay unmapped, so the check would have to be dropped or
   re-encoded in a smaller window (it cannot name a block above 0xFFF00000 >> 4 and still fit); `HEAP_LARGEST_BLOCK` and
   the `GetSystemInfo` assertion move with it. Gain only when the heap is nearly full, which no measured run is. **Not
   implemented**: small gain, touches the diagnostics that upstream's typed work relies on, and cannot be verified without
   a route that fills 3.2 GB. Revisit if option 1 shows a modded run past about 2.8 GB.
4. **Reclaim the 128 MB of never-mapped handle and token space** in `0x10000000..0x20000000`. The typed migration needs
   that space as it moves more families. Not safe to take while upstream grows into it.
5. **Smaller reservation gap use.** `NV_RESERVE_GAP` already keeps the scrap heaps' 8 MiB reservations out of the heap
   (99% never committed). Nothing left to gain.
6. **Lowering the window (a vanilla 2 GB preset).** Not an unlock; a test of how mods behave in a plain 32-bit process.
   `NV_GUEST_WINDOW=2GiB` (also `1GiB`, `1536`, `1536MiB`; at least 256 MiB; unset is the whole heap) lowers the guest heap
   to that size: the two ranges end earlier (`mem/heap.rs`, `window_ends`; the lower range is 1504 MiB, so 1 GiB cuts it
   short with no upper range and 2 GiB leaves 544 MiB above `HEAP_HIGH_BASE`). Nothing moves: addresses of the platform
   area, the slab and token ranges and the freed-block words stay as they are, and `heap_cap` in `NV_MEMLOG` reports the
   lowered size. Off by default, one process-wide read of the variable, no cost on the allocation path. An allocation the
   lowered heap cannot serve ends the run as the exhausted heap does (`heap exhausted allocating ...`), which names the
   sizes most live and most free; with the stop report of nv-rs#227 the record carries the class. Results below.

## 5. What was left for upstream

The unlock proper: more typed families, the removal of the guest heap as a requirement, and any change to the guest's
4 GiB window shape (`mem.rs` layout), which the typed migration's slab and token ranges depend on.

## 6. Reading a run

```
NV_MEMLOG=1 nv-rs-dev bench run newgame
grep NV_MEMLOG ~/.local/state/nv-rs/bench/newgame-*/viewer.log | tail -1
```

Keys: [MEMLOG.md](MEMLOG.md). Guest share of the resident set is `heap_used` plus the exe image; the host share is
`rss_anon` minus the guest's committed pages.

## 7. Window shrink results

Runs of `NV_GUEST_WINDOW` on a scenario (nv-rs#267) go in this table, one row each: the scenario, the window, the heap peak
(`heap_peak` of `NV_MEMLOG`), how the run ended (verdict, and the stop's reason text when it stopped) and the run's commit. The
runs need the owner's game data, so they are run on a machine that has it, under the trial lock, not in CI.

| Scenario | Window | Heap peak | Ended | Commit |
| --- | --- | --- | --- | --- |
| goodsprings (new game to the first exterior, the walk) | 2 GiB (`heap_cap` 2147483648) | 206.9 MB (`heap_peak` 206909040) | PASS, no stop lines | bac7a385 |
| goodsprings | 1 GiB (`heap_cap` 1073741824) | 206.3 MB (`heap_peak` 206261056) | PASS, no stop lines | bac7a385 |

Reading: the vanilla new game to Goodsprings uses about 200 MB of the guest heap, a fifth of even the 1 GiB window, so no family
depends on the large window in this scenario. The scenario that fills the heap (a modded, merged instance, #265) is the one
that tells; run it with `NV_GUEST_WINDOW=2GiB` and `1GiB` and add its rows here. A run that does not fit stops with the
`heap exhausted` text, which names the sizes most live and most free.
