# Graphics settings

Fork-only. Design: [ADR-0019](adr/0019-graphics-modules.md); issues nv-rs#591 (graphics modules) and nv-rs#592 (this
slice). A fresh install draws the original picture exactly: every setting below starts at the value that draws the
original, and the `Original` preset is all of them.

## Where the settings are

One file per profile, `nv-rs-graphics.toml`, beside the profile's `Fallout.ini` (`My Games/FalloutNV` in the run's
Documents folder). A launcher's play run reads it; a scripted run reads it only when `NV_GFX_FILE` names it. raul's
Settings page edits the same file.

```toml
version = 1
preset = "modern"                  # original (the default), modern, performance

[global.ao]                        # every place
quality = "high"

[worldspace.WastelandNV.shadows]   # outdoors in one world space (editor id, or form id in hex)
quality = "low"

[interior.ao]                      # every interior cell
strength = 0.5

[cell.GSDocMitchellHouse.ao]       # one cell (editor id, or form id in hex)
enabled = false
```

A value is taken, weakest first, from: the original, the preset, the file's `global`, `worldspace`, `interior` and
`cell` tables, then `NV_GFX`. Keys the engine does not know, comments and lines it cannot read are kept when the file is
written; what it cannot use is named once in the log (`nv-rs: graphics settings ...`). The world space, interior and
cell layers take effect with the in-game overlay (next slice); until then the start reads the global layer.

## Presets

| Preset | Ambient occlusion | Sun shadows |
| --- | --- | --- |
| `original` | off | off |
| `modern` | on, medium, strength 0.7 | on, medium (2048) |
| `performance` | on, low, strength 0.6 | off |

A preset is a settings file (`crates/fork-gfx/presets/`); a value set in the profile's file wins over it.

## The modules

### Ambient occlusion (`ao`)

A soft shade in corners, creases and where things meet the ground; the port's GTAO step at half resolution
(`crates/d3d9-wgpu/src/scene_light.rs`). Not in the original game; Vulkan only.

| Setting | Values | Original | Applies |
| --- | --- | --- | --- |
| `enabled` | true, false | false | at the next start |
| `quality` | `low` (1 slice of 4 steps a side), `medium` (2 x 4), `high` (3 x 6) | `medium` | live |
| `strength` | 0.1 to 1.0 | 0.7 | live |

### Sun shadows (`shadows`)

Objects cast shadows from the game's sun and moon light in four cascades out to about 140 m
(`crates/d3d9-wgpu/src/sun_shadow.rs`). Not in the original game; Vulkan only.

| Setting | Values | Original | Applies |
| --- | --- | --- | --- |
| `enabled` | true, false | false | at the next start |
| `quality` | `low` (maps of 1024 pixels), `medium` (2048), `high` (4096) | `medium` | at the next start |
| `characters` | true, false: people, creatures and you in third person cast them too | true | at the next start |
| `light_from_sky` | true, false: the light comes from the sun disc and the moon | true | at the next start |

The older keys (`[nv-rs Display] bAmbientOcclusion`, `fAmbientOcclusionStrength`, `bSunShadows`,
`bSunShadowsCharacters`, `bSunLightFromSky` and their `NV_*` variables) still work; a value set in the graphics settings
wins over them.

## Scripted runs

- `NV_GFX_PRESET=modern`: a preset for the run (wins over the file's).
- `NV_GFX=ao.enabled=true,ao.quality=high,shadows.enabled=true`: values for the run, over everything; a layer may lead
  (`cell.GSDocMitchellHouse.ao.enabled=false`).
- `NV_GFX_FILE=PATH`: a settings file for the run.

A misspelt preset or a malformed `NV_GFX` stops the start with one line. With the scene tool, one invocation draws the
same places with several settings: `--variant NAME` with `--variant-env NAME:NV_GFX_PRESET=modern` (the variant's own
environment, over `--env`):

```
nv-rs-scene run --places docs/fork/scenes/mojave-far.toml --out OUT --variant original \
  --variant modern --variant-env modern:NV_GFX_PRESET=modern --game-lock "$GAME" --label gfx --who gfx ...
```

## Cost

Measured 2026-11-23 with `nv-rs-scene bench --suite quick` (exclusive game lock, 120 warm-up frames, three passes of 600
frames, the median), `play-opt` build, 2560x1440, offscreen, one machine (an 8-core desktop processor and a high-end desktop
graphics card, CPU governor `powersave`, the card's clocks on automatic). Two measures:

- **Frame time** (`avg_ms`): what the player feels. At these places the frame is bound by the processor (the game's own
  draws), and no level of either module changed it by more than the run-to-run noise (about 0.5 ms), on the plain game and
  on a 312-mod profile, with one exception: shadows `high` at Goodsprings on the plain game, 8.6 ms against 5.5 ms.
- **The card's time** (`NV_WGPU_GPU_TIMES=1`, mean over the timed frames of the five places): the effect's own step, and
  the total. The card runs at a low clock while it is mostly idle, so these totals move by up to 1.5 ms between runs of
  similar work; the step times are the steadier number.

| Level | Frame time, plain game (mean of 5 places) | Frame time, 312-mod profile | Card: the step itself | Card: total, plain / modded (original 0.98 / 1.12) |
| --- | --- | --- | --- | --- |
| `original` | 6.15 ms | 9.49 ms | none | 0.98 / 1.12 |
| `ao` low | 6.10 | 9.27 | 0.20 to 0.34 | 2.44 / 1.58 |
| `ao` medium | 6.09 | 9.27 | 0.33 to 0.35 | 2.39 / 2.89 |
| `ao` high | 6.36 | 9.25 | 0.23 to 0.28 | 1.24 / 2.10 |
| `shadows` low | 6.17 | 9.33 | maps 0.08 to 0.17 | 1.40 / 2.70 |
| `shadows` medium | 6.18 | 9.39 | maps 0.04 to 0.55 | 1.14 / 2.81 |
| `shadows` high | 6.81 | 9.23 | maps 0.36 to 1.00 | 2.71 / 2.00 |
| `modern` | 6.19 | 9.25 | 0.40 + 0.24 | 1.34 / 2.45 |
| `performance` | 6.13 | 9.10 | 0.13 to 0.14 | 1.09 / 1.25 |

For the `Modern` target (8.3 ms a frame at 1440p with a 312-mod profile): the effects of `modern` fit in about 1 ms of the
card's time, and the card is not what limits the frame. On the modded profile the frame itself is already 9.5 ms on
average (12.8 ms on the Tops casino floor, 11.4 ms at Novac) with every effect off. The processor's share of the frame is
what the target needs next.
