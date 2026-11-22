# ADR-0019: Graphics modules, a settings file per profile with layers by place, and an in-game overlay

> A fork decision record kept under docs/fork/adr/ because docs/adr/ is upstream-owned; it takes the next free number after ADR-0018.

**Status:** Proposed (design for the maintainer's decision; the decisions it builds on are the maintainer's, 2026-11-22, on nv-rs#591 and nv-rs#592)
**Date:** 2026-11-22
**Issues:** nv-rs#591 (graphics modules), nv-rs#592 (this first slice); raul's Settings page: Toolbox #556

## Context

The engine draws the original game's picture with the game's own renderer translated to Rust (ADR-0007): the HDR
step, bloom, depth of field, fog and the weathers' image-space data all run as the original does. Two effects were
added by the port and are off by default: ambient occlusion (GTAO at half resolution, `crates/d3d9-wgpu/src/scene_light.rs`)
and cascaded sun shadows (`crates/d3d9-wgpu/src/sun_shadow.rs`). Both are switched once at start, from
`[nv-rs Display]` in the run's `Fallout.ini` or from `NV_AO` and `NV_SUN_SHADOWS` (`host::fixes::Choice`), and their
quality is fixed by constants. Nothing can be changed while playing; the only overlay the port draws is the F11
frame-rate counter (`crates/d3d9-wgpu/src/frame_rate.rs`), drawn at the present with the port's bitmap font.

The maintainer decided (2026-11-22):

- a fresh install draws the original picture exactly; presets `Original`, `Modern`, `Performance` (later `Traced`) are one choice away;
- one settings file per profile, typed entries (toggle, slider, choice) from a schema each module declares, layers by
  place from weakest to strongest: global, world space, interior, cell; the most specific place wins; unknown keys are
  kept when the file is written;
- raul's Settings page edits the same file from the same schema, which the engine describes in a form another program reads;
- an overlay inside the game opens with one key, applies a change in the next frame and saves it to the profile's file;
- `Modern` aims at 1440p and 120 frames a second (8.3 ms a frame) on the maintainer's target class of current high-end desktop graphics card with a
  312-mod load order; every module states its measured cost;
- compatible open-source code (GPL-3.0, LGPL-2.1) may be adapted with credit and notices kept; nothing proprietary.

## Decision

### 1. A graphics module

A graphics module is a named group of settings and the part of the frame it owns. It is Rust code inside the engine (no
plugin DLLs, no scripting), registered in one list. Its declaration is plain data (`fork_gfx::ModuleSpec`):

| Field | What |
| --- | --- |
| `id` | its key in the settings file (`ao`, `shadows`, `bloom`, `dof`, `fog`) |
| `label`, `help` | what the player reads in the overlay and in raul |
| `settings` | typed entries: a toggle, a slider (range and step) or a choice (named options); each with the value that draws the original picture as its default |
| `owns` | what it switches or adds: a switch of existing code (`NV_AO`), a value of the game's own data it scales (a fog distance, an image-space field), or a pass with its insertion point (after the world, inside the HDR chain, where AO and the sun shadows hook in today; or at the present) |
| `cost` | where its measured cost is written (`docs/fork/GRAPHICS.md`), filled from `nv-rs-scene bench` |

The trait (`fork_gfx::GraphicsModule`) has two methods: `spec()` and `apply(&ModuleSettings)`. `apply` is called once
when the module is registered and again only when one of its values changes; the values are used from the next frame on.
The drawing side (the passes a module records) stays in `d3d9-wgpu`, which the settings crate does not depend on; it
becomes a second trait there when the pass list of nv-rs#289 is built. Phase 1 needs none: its modules switch and tune
code that exists.

### 2. The settings file

One file per profile: `nv-rs-graphics.toml` in the profile's settings folder, beside its `Fallout.ini` (`My Games/FalloutNV`
in the run's Documents folder; raul gives each profile its own). A small subset of TOML, so another program reads it with
any TOML reader:

```toml
version = 1
preset = "modern"                  # original (the default), modern, performance

[global.ao]                        # every place
quality = "medium"

[worldspace.WastelandNV.shadows]   # outdoors in one world space (editor id, or form id in hex)
quality = "low"

[interior.ao]                      # every interior cell
strength = 0.5

[cell.GSDocMitchellHouse.ao]       # one cell, inside or outside (editor id, or form id in hex)
quality = "off"
```

- **Layers.** A value is found, weakest first, in: the setting's original value, the preset (itself a settings file,
  resolved at the same place), the file's `global`, `worldspace.<name>`, `interior` and `cell.<name>` tables, and last the
  overrides of a scripted run. Within one file the most specific place wins; the profile's file always wins over the preset
  (a value the player set is never undone by the preset's place tables). A world space layer applies outdoors only; the
  interior layer to every interior cell; a cell layer to that one cell. Names are compared without case, as the game
  compares editor ids; a form id is written in hex (six to eight digits, `0x` optional), as in the scene tool's place files.
- **Unknown keys are kept.** The crate reads the file into pieces that keep their text byte for byte; writing changes only
  the value of the key that was set (its line, spaces and comment stay), adds a line in the right table or adds a table at
  the end. Keys of other programs, keys of a newer engine, comments, and lines that are not TOML all survive a write. What
  cannot be used (an unknown module or key, a value of the wrong type, a number outside its range, which is clamped) is
  named once in the log and left to the weaker layers.
- **Versions.** `version = 1`. A newer version is read for what this engine knows; the rest is kept.
- **No dependency.** The reader and writer are this crate's own (as `fork-scene`'s place files are): the TOML crates in the
  lock file come in only as a build-time dependency of a macro crate, and a format-preserving writer for this small subset
  is about 1,000 lines with its tests.

### 3. Presets are data

A preset is a settings file with an id and a label: `original` (always first, and empty: every setting at its original
value), then the built-in `modern` and `performance` files shipped in the crate as data, later the player's own preset
files and imported looks. Choosing a preset writes one line, `preset = "..."`. A value the player changes on top is written
to the layer the overlay is showing; "back to the preset" removes it.

### 4. Original is identical

- Every setting's default is the value that draws the original picture, and the empty file (or none) resolves to it.
- A module whose values are all original installs nothing, reads nothing and records nothing in the frame, exactly as
  today's `NV_AO=0` and `NV_SUN_SHADOWS=0` do (`ModuleSettings::is_original`).
- The overlay, closed, draws nothing; open, it is drawn like the F11 counter, into the window's frame after the frame's
  capture is taken, so it is never in the back buffer, an `NV_SHOT` capture, or a scene picture.
- Proof in every pull request that adds a module or changes one: the standard views of `nv-rs-scene` with `Original`
  against the last good set, mean 0.000 (`nv-rs-scene compare`), each view opened.

### 5. Scripted selection

Pictures and benches choose a preset and single values without the overlay: `NV_GFX_PRESET=modern` and
`NV_GFX=ao.quality=high,shadows.quality=low` (a layer may lead: `cell.GSDocMitchellHouse.ao.quality=off`), passed with the
scene tool's `--env`, and a place file's `gfx = "..."` for one place. A scripted run reads no profile file unless
`NV_GFX_FILE` names one, so a test's picture does not depend on the machine's profile. The current `NV_AO`,
`NV_SUN_SHADOWS` and `[nv-rs Display]` keys keep working and map onto the same module values.

### 6. The overlay

- One key opens and closes it: F10 (the game's default controls do not use it; F11 stays the frame-rate counter's), only while a wgpu
  device draws the game's window. While it is open its keys (arrows, Enter, Page Up and Down, Escape, F10) go to it and not
  to the game; closed, every key reaches the game as before.
- It lists the presets and the modules with their settings, generated from the schemas; it shows which layer a value comes
  from and which layer an edit goes to (global by default; the current world space, interior or cell on request).
- A change is applied in the next frame and written to the profile's file at once (written to a new file beside it, then
  renamed over it). The engine also reads the file again when another program (raul) changes it, checked about once a second.
- Drawn with the port's own bitmap font and quad list at the present, as the F11 counter is. No interface crate is added:
  an immediate-mode toolkit (egui) would need an egui-wgpu release for the tree's wgpu 30.0.1 and its own input path, for a
  list of a few dozen lines. If the overlay later needs more (mouse, text input, pictures), the choice is made again then.

### 7. raul and the preset importer

- raul's Settings page (Toolbox #556) depends on `fork-gfx` through its engine submodule (a pure crate, no engine closure)
  and uses the same reader and writer, so both programs keep the same keys and comments. It renders from the schema, which
  the engine prints as JSON (`fork_gfx::dump::schema_json`; `nv-viewer --gfx-schema` and a `fork-api` call in slice 4): the
  file's name and version, the layers, the presets, and per module its settings with type, range, step, options and
  original value. raul hard-codes no setting.
- A preset importer (later slice) reads a look the player downloaded (an ENB, NVR or ReShade preset) and writes a preset
  file of this format, with what it could not map listed; nothing of the look is shipped.

### 8. Cost

Each module's cost is measured with `nv-rs-scene bench` at 1440p, per quality level, on the plain game and on the full
modded profile, and written in `docs/fork/GRAPHICS.md` and the pull request that exposes it. The budget for `Modern`
inside 8.3 ms is the graphics atlas's estimate until then: ambient occlusion about 0.8 ms, sun shadows on medium about
1.5 ms of GPU time plus their caster draws (the largest risk; shadows fall back to low on the Strip).

### 9. The first modules (nv-rs#592), proposed levels

| Module | Settings | Original | What a level changes |
| --- | --- | --- | --- |
| `ao` | quality off / low / medium / high; strength 0.1 to 1.0 | off; 0.7 | slices and steps of the GTAO search (`SLICES`, `STEPS`: 2 x 4 today is medium), radius, half or full resolution at high |
| `shadows` | quality off / low / medium / high / ultra; characters cast (toggle) | off; on | map size (`SIZE` 2048 today), cascades (`CASCADES` 4) and reach (`FAR`) |
| `bloom` | strength 0 to 2 (x the game's) | 1 | the image-space bloom values the weather gives, scaled; the game's own step |
| `dof` | depth of field: off / original | original | whether the game's own depth of field runs (dialogue, V.A.T.S., aiming) |
| `fog` | distance 0.5 to 4 (x the weather's); fog on or off | 1; on | the weather's fog distances, scaled |

The levels and ranges are settled in the slice that wires each module, after its pictures and bench numbers.

## Consequences

- One new fork-owned crate, `fork-gfx` (pure: schema, file, layers, presets, resolution, JSON description), with no
  dependency; component `gfx` in `.fork/components.toml`. The workspace member is listed in
  `.fork/allowed-divergence.d/graphics-settings`.
- Wiring the modules changes upstream-owned files (`host::fixes`, `host/src/lib.rs`, `d3d9-wgpu`'s AO and shadow steps
  become switchable at run time); each such change is recorded in `docs/fork/DIVERGENCE.md` in its pull request.
- The existing `[nv-rs Display]` AO and shadow keys stay for old profiles; the settings file is where new settings go.
- The real Direct3D 9 fallback has no modules and no overlay.
- Every later module (tone curve, grading, anti-aliasing, the shader path, upscaling, traced effects) uses the same schema,
  file, layers and overlay, and carries a picture case per level and a bench number.
