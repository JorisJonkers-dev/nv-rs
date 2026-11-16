# The stack reorganisation: what the fork owns

The stack was divided again on 2026-11-16. The plan is the Toolbox's
[`docs/REORG.md`](https://github.com/JorisJonkers-dev/rauls-trusty-toolbox/blob/main/docs/REORG.md), and its decisions
are the Toolbox's ADRs 0007 to 0011 (`docs/adr/` in rauls-trusty-toolbox). This page only says where the fork stands
in it.

## What this fork owns

- **The engine:** the native port, its window, sound and input layers on Linux, and its Windows and macOS builds.
- **Performance:** main-thread work, the renderer's draw path, the shader and pipeline caches, threading and memory.
- **The extenders:** the built-in script extenders and their command sets, including the commands a large modded list
  needs that upstream does not build.
- **The Engine extension API:** the extension host and its tiers.
- **Render injection:** loose shader overrides, name-keyed packs and effects on the wgpu renderer.
- **The in-game bench harness:** `nv-rs-dev bench`, which measures frame time, hitches and memory in the game under
  the bench lock (`scripts/fork/bench/` today, a fork-owned Rust crate later).
- **The engine Contracts:** `nv-rs-release-manifest`, `nv-rs-capabilities`, the run summary, the bench summary and
  `run-options`. nv-rs defines and versions them; the Toolbox keeps a snapshot of each version a Known-good set names.

## What it does not own

The Toolbox (`raul`) fetches, verifies, installs, checks and launches nv-rs, holds the run settings and the GUI, and
reads the engine Contracts (ADR-0007). nvmod-rs keeps mod management and the content Doctor, including the extender
catalogue. nv-rs's own CLI works alone: it never fetches, finds or starts another Component.

## Tickets

Every open ticket in this repository carries a `size:` label, acceptance criteria, its dependencies and the number it
moves. Work for a milestone is a sub-issue of that milestone's issue in rauls-trusty-toolbox. Tickets labelled
`parked` are outside every milestone until the condition written at the top of the ticket is met.
