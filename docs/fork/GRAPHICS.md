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
cell layers take effect while playing in the next slice; until then the start and the overlay use the global layer.

## Presets

| Preset | Ambient occlusion | Sun shadows | Light balance (fill light outdoors, indoors) |
| --- | --- | --- | --- |
| `original` | off | off | 1, 1 |
| `modern` | on, medium, strength 0.7 | on, medium (2048) | 0.7, 1, with light from the sky (see Light balance) |
| `performance` | on, low, strength 0.6 | off | 1, 1 |

A preset is a settings file (`crates/fork-gfx/presets/`); a value set in the profile's file wins over it.

## The overlay in the game

F10 opens and closes it (Escape closes it too), while the engine draws the game with Vulkan. It lists the preset and
every module's settings, each with where its value comes from (`original`, `preset modern`, `global`, `interior`, ...)
and, for a setting that takes effect only at the next start, "next start" once it was changed.

- The keyboard works it: Tab and Shift+Tab move between the controls, Space and Enter switch or open one, the arrows
  move a slider or a list. While it is open, keys pressed go to the overlay and not to the game; keys let go still
  reach the game, so nothing you held when you opened it stays held. The mouse is not used yet.
- "Changes go to" chooses the layer a change is written to: everywhere (the default); once the layers by place are live,
  also outdoors in the current world space, every interior, or the current cell.
- A change applies in the next frame where the setting is live, and is written to the profile's `nv-rs-graphics.toml`
  at once (keys and comments the engine does not know are kept). A scripted run that reads no file applies the change
  and keeps nothing.
- The overlay is drawn after the frame is captured: it is never in an F12 report, a screenshot the engine takes or a
  scene picture. Closed, it costs nothing; open, about 0.15 ms of the processor's time and 0.03 ms of the card's at
  2560x1440. Its text is the frame-rate counter's font, twice as large from 1440 lines up.

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

### Light balance (`light`)

The game lifts every lit surface by a flat fill light: the weather's ambient colour outdoors, the cell's indoors. It
stands in for the light the sky and the ground bounce into shade. With the port's own shade (ambient occlusion, sun
shadows) and the light from the sky (the sun light comes from the disc, so ground at midday faces it squarely) the whole
fill on top washes the picture out: under `modern` with the whole fill, sunlit ground at the standard day views was 1.7
times as bright as under `original` on the plain game and 1.45 times on a 312-mod profile (nv-rs#768). The light balance
hands the lit shaders a share of that fill (`crates/engine-fallout-env/src/units/fallout_shared_sky/sun.rs`,
`AmbientShare`); the sky's own colours, the fog and water are unchanged. Far objects take the same share.

| Setting | Values | Original | Applies |
| --- | --- | --- | --- |
| `ambient_exterior` | 0 (no fill: sun, moon and lamps only) to 1 | 1 | live |
| `ambient_interior` | 0 (no fill: the cell's directional light and lamps only) to 1 | 1 | live |
| `ambient_night` | the share of the fill at night, 0 to 1 | 1 | live |
| `sun` | the share of the sun's direct light by day, 0.3 to 1 | 1 | live |
| `moon` | the share of the moon's direct light at night, 0.3 to 1 | 1 | live |
| `moon_max_height` | how high the moon climbs, 15 to 90 degrees; the drawn moon and its light move together; 90 is the game's path | 90 | live |
| `moon_specular` | how strongly glossy materials catch the moon, 0 to 8; 1 is the game's highlight; needs `sky_dome` and the light from the sun and moon discs | 1 | live |
| `sky_dome` | how much of the fill follows the sky, 0 (the game's flat light) to 1; needs sun shadows | 0 | live |
| `shade_fill` | how much more sky light reaches surfaces the sun or moon does not, 0 to 2; needs `sky_dome` | 0 | live |

With `sky_dome`, faces toward the sky take the sky's colour and faces toward the ground a warm bounce, in shadow too, and
ambient occlusion outdoors dims only this sky light, not the direct light. At night the moon's direct light follows the
game's phase, fade and height, so a new moon gives no direct light and no moon shadow; a dim, cool starlight by surface
direction is the fill.

`modern` takes `ambient_exterior` 0.7, `ambient_interior` 1, `ambient_night` 1, `sun` 0.65, `moon` 0.75,
`moon_max_height` 45, `moon_specular` 2, `sky_dome` 1 and `shade_fill` 0.2. Over 30 views of a 312-mod profile at 2560x1440, against
`original`: sunlit light 0.83 and shade 0.91 over the 22 day exteriors; moonlit light 0.82 and shade 0.96 over the 7 night
views. 45 degrees is the lowest moon height tested that keeps the detail check below at the night Strip skyline.

The sky light costs the card under 0.1 ms a frame at 2560x1440 on a high-end AMD card (`modern` with and without
`sky_dome` and `shade_fill`, 11 standard views, two runs: +0.03 and +0.08 ms), less than the difference between runs.

The moon highlight scales the one place in each of the game's specular shaders where the highlight meets the light's
colour (`wgsl::specular_reads`; 284 of the game's 2716 lit pixel shaders have a highlight, each with one such place). It
uses the materials' own gloss, follows the moon's direction, and is made from the moon's light, so a new moon gives none.
Surfaces whose shaders have no specular term gain nothing. At 4 ground starts to look wet, at 8 it does. Its cost is
within the difference between runs (-0.01 and +0.04 ms of card time at 2560x1440 on a high-end AMD card).

Known limits: glow is not separated from the occlusion; surfaces turned away from the true sun or moon can be much darker
than under `original` (a rock wall at Hoover Dam by day has 0.26 of its shade light); a single night view can differ
between runs of one build.

The picture case: draw the standard day views (`docs/fork/scenes/mojave-far.toml`) with `--variant original --variant
modern --variant-env modern:NV_GFX_PRESET=modern` in one run, then

```
nv-rs-scene light OUT/original/pictures OUT/modern/pictures --within 5
```

prints, per view, the sunlit, shade and ground light of `modern` against `original` (linear light of the lower 45% of
the picture; sunlit its brightest 40%, shade its darkest 20%) and exits 1 when the mean sunlit ratio is more than 5% from 1.

With `--max-lost-detail POINTS` it also exits 1 when a view gains more than that many points of dark blocks that show no
detail in the lower 55% of the picture: darkness alone passes, lost shapes and texture do not. The limit for `modern` is 5.

### Textures (`textures`)

`anisotropy` (nv-rs#829, `d3d9_wgpu::state::forced_anisotropy`): the game asks for plain linear filtering on most
surfaces, so ground, roads and walls blur at grazing angles. A level forces anisotropic filtering, at that level or the
game's own when it asks for more, on every sampler that filters a mipmapped texture linearly as it shrinks it; point
sampling, render targets read without mipmaps and depth stay as the game set them. It is what DXVK's
`d3d9.samplerAnisotropy` offers.

| Setting | Values | Original | Applies |
| --- | --- | --- | --- |
| `anisotropy` | `game` (the game's sampler states), `x2`, `x4`, `x8`, `x16` | `game` | live |

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
