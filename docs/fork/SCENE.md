# Scene capture

Fork-only. Tracking issue: nv-rs#479 (design, slices, open questions). A scene run starts the engine, sends the
player to named places the game's own way, with no main menu and no typed console text, and (in later slices) freezes
the world, waits for a real "ready" condition and writes one frame as a picture.

This page describes what is built. The design and the order of the slices are on the issue.

## Switches

| Variable | Meaning |
| --- | --- |
| `NV_SCENE` | the place file; setting it turns the scene run on. A file that cannot be read or parsed stops the start (exit 1) with one plain line |
| `NV_SCENE_PLACES` | comma-separated place names, visited in that order; default all places of the file |
| `NV_SCENE_OUT` | the folder for pictures and the run's `scene.json` (used from the picture slice on) |
| `NV_SCENE_SETTLE_FRAMES` | frames rendered after ready before the picture; default 30 |
| `NV_SCENE_TIMEOUT_S` | seconds a place may take to be reached; default 120. A place not reached in time ends the run with status 70 and one line `scene: place NAME was not reached within N s` |

Exit status 70 (`fork_scene_run::EXIT_SCENE_FAILED`) is a scene condition that failed; 0 is every place reached.

## The place file

Version 1, a small subset of TOML read by `fork-scene` (no dependency):

```toml
version = 1
[[place]]
name = "lake-mead"       # letters, digits, - and _ (it names the picture file)
world = "WastelandNV"    # an editor id this build knows, or the world space's form id in hex
x = 53799.8
y = 38375.3
z = 2917.4               # standing height
heading = 60             # degrees, 0 north, 90 east
pitch = 0                # degrees, positive looks down
# path = [[x, y, z], ...]   # optional, for a bench
```

## How the engine goes to a place

The runner (`fork_scene_run::Runner`) runs inside the game's frame, on the game's main thread, at the entry of `Main::OnIdle` (`0086e650`): `fork_scene_run::install` registers a function there that runs the runner and then the translation. (Calling the game's functions from the host's thread between frames does not work: the load waits for loader threads while the host holds the kernel lock, and the call spins.) The runner waits for the main menu, then calls the game's own
`PlayerCharacter::PositionPlayerExterior` (`0093cce0`, the function fast travel and `moveto` end in: it cancels pending
exterior loads, loads or creates the cell and places the player on its land) with the place's world, position and
heading, once the screen has been the menu for 120 frames (the game's first frames still start its threads). The place is reached when the screen is the play screen and the player's position lies in the place's grid
cell. Later places are sent from the play screen the same way.

Limits: only `WastelandNV` is known by editor id (others by form id). The frozen world (hour, weather, creatures, interface hidden) is the next slice.

## The picture

With `NV_SCENE_OUT=<dir>` each place ends as `<dir>/<name>.png`. The drawing layer keeps its frame readback on
(`NV_SHOT=<dir>` with `NV_SHOT_LIST=0`, set by the runner when the variables are not set) and the runner asks for one
frame with `d3d9_wgpu::shot::ask`; the next present's frame is written as a BMP, the runner converts it to PNG
(`fork-scene`, no dependency beyond `flate2`) and removes the BMP.

A place is ready when the screen is the play screen, the player is in the place's cell and its height has stayed within
0.25 units for `NV_SCENE_SETTLE_FRAMES` frames in a row (collision and land present: the player has stopped falling).
Message boxes the new game's items and quests open (`MessageMenu`, top menu `0x3e9`) are closed with
`MessageMenu::ClearMessageQueueHead` (`007aa530`) as they appear. Not frozen yet (next slice): the weather, the hour,
creatures, the HUD and its messages: pictures taken now differ in cloud and dust, and show the HUD.

One command (offscreen, no window session; the game folder is the first argument):

```
NV_D3D9=wgpu NV_D3D9_OFFSCREEN=1 NV_SOUND=out NV_SOUND_BACKEND=null NV_RESOLUTION=1280x720 \
NV_SCENE=places.toml NV_SCENE_PLACES=lake-mead,goodsprings-valley NV_SCENE_OUT=out-dir \
NV_SCENE_SETTLE_FRAMES=200 nv-viewer /path/to/game
```

All places of the file are taken in one start, in order.

## Measured

Offscreen (`NV_D3D9_OFFSCREEN=1`, no window session), `play` profile, base game, one place (Goodsprings): the menu is up
at main frame 120 (about 9 s after the process starts), the place is reached 2 frames and about 2.7 s later, the
whole run ends with status 0 after 11 s wall time. A run without `NV_SCENE` reaches frame 200 in 9 s.

Five places (the calibrated views of 2026-11-21) in one start, 200 settle frames each: 40 s wall time for the whole run
(start to the menu 9 s, then about 6 s a place), exit status 0, five PNGs. The old driver hung at the fourth view of a
session; this path did not (one run). Timing rows not valid: machine shared.
