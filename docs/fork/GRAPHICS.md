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

Measured with `nv-rs-scene bench` (exclusive game lock, 120 warm-up frames, five passes of 600 frames, the median),
`play-opt` build, 2560x1440. The cost of a level is its frame time minus `original`'s at the same places.

(Filled in by the pull request that measures them.)
