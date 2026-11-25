<!-- SPDX-License-Identifier: GPL-3.0-only -->
# Engine features

The coverage catalogue's area `features` ([CATALOGUE.md](CATALOGUE.md)) lists the engine the way players and modders
know it: LOD drawing, water, the Pip-Boy map, V.A.T.S., combat AI, save games, the radio. It also lists the move to a
modern engine: 64-bit readiness, multi-threading, and the rendering backend on wgpu and Vulkan.

## Two bars

The picture (`docs/fork/coverage/tally.svg`, made by tally 0.7.0 or later, which reads the evidence keys `works` and
`share_column`) gives the area two bars.

1. **Features.** A feature counts as implemented only when it is **100% native**: every one of its functions runs as
   native typed code (`// Typed from <addr>`, docs/ENGINE_CRATE.md "Typed code"), in a family that is on by default.
   The grey-blue segment shows the features that are not native yet but **work via the game's code**: the
   address-level translation of the exe's own functions. Every engine feature works today.
2. **Native share.** The average, over the engine features, of each feature's native share (native functions of all
   its functions; each feature counts once, whatever its size). The grey-blue part is the share that runs through the
   game's code.

## Where the engine features come from

Two hand-kept files in `.fork/catalogue/features/`:

- `features.tsv`: `id`, `group`, `title`. The groups are `rendering`, `world`, `navigation`, `actors`, `combat`,
  `gameplay`, `ui`, `audio`, `scripting`, `data`, `physics` and `platform`; a feature's item is
  `features.<group>.<id>`.
- `units.tsv`: `unit`, `feature`. Every compile unit of the ledger (`scripts/ledger --tsv`, column `unit`, from
  `research/engine-map/engine_map.tsv`) that belongs to a feature, a unit to at most one feature.

`task catalogue` (`scripts/fork/progress catalogue`) reads both with the ledger. It writes one reference list per
group (`ref/features_<group>.tsv`) and, in `target/catalogue/evidence.tsv`, one row per feature: `implemented` or
`translated`, and the share `native/translated/total` in functions. It fails when a feature has no function, when a
unit is not in the ledger, and when a list is malformed.

- A function counts when its ledger status is `translated` and it is not a compiler static initialiser. Static
  initialisers, the Havok SDK, unplaced runs and the C runtime are left out. Functions in no feature's unit are left
  unmapped.
- A function is native when the ledger finds its typed version and no family that is off by default (`past_gate:
  false`) is declared in its marker's folder or a folder above it.

Moving a unit to another feature, or adding a feature, is an edit of these two files. The reference lists then change
with them, and `progress catalogue --check` in CI says when they were not regenerated.

## The other groups, kept by hand

- `features.bit64` (64-bit readiness): the host program, no exe code run, families in host memory (stage 2), the 4 GB
  limit, typed extender access, the plugin image base, and traces and pixel gates against the exe (ADR-0012, ADR-0014,
  ADR-0015).
- `features.threading`: the area:threading issues, one item each.
- `features.backend`: the area:vulkan issues and what the wgpu backend has proven.
- `features.looks`: what the fork's own graphics settings change in the game's drawing beyond the graphics plan's rows
  (the near water's fade).
- `features.direct3d`: two roll-ups, read from the same code as `d3d9.methods` and `d3d9.states` (every method carried,
  every render state mapped). They are not duplicated as items.

A hand-kept row is implemented only when its proof has landed: a merged change with a test or a measurement, never a
plan. For example, real threads as the default stay missing until their proof run (#245) is in.

## Tags

A test tags a feature only when it exercises the feature, asserts its result, and would fail without it
(`.fork/catalogue/covers.d/engine_features.tsv`). tally refuses a tag on a missing item, so an engine feature can be
tagged only once it is fully native.
