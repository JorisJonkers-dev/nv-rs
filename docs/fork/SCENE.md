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

Limits of this slice: only `WastelandNV` is known by editor id (others by form id); the picture, the frozen world and
the ready condition (land and collision present, the player standing) come in the next slices.

## Measured

Offscreen (`NV_D3D9_OFFSCREEN=1`, no window session), `play` profile, base game, one place (Goodsprings): the menu is up
at main frame 120 (about 9 s after the process starts), the place is reached 2 frames and about 2.7 s later, the
whole run ends with status 0 after 11 s wall time. A run without `NV_SCENE` reaches frame 200 in 9 s.
