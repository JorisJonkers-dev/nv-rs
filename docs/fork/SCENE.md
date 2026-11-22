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
world = "WastelandNV"    # an editor id of any world space in the load order, or its form id in hex (six digits or more)
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
`MessageMenu::ClearMessageQueueHead` (`007aa530`) as they appear. 

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

## The frozen world

Unless `NV_SCENE_FREEZE=0`, the runner holds the world every frame (idempotent writes, so a load that changes
something is corrected the next frame): the calendar's hour global at `NV_SCENE_HOUR` (default 12) and its time-scale
global at 0; the weather forced as an override with `Sky::ForceWeather` (`0063d0e0`) to `NV_SCENE_WEATHER` (default
form `000FFC88`, the clear weather the calibrated views used); all AI processing off (the byte `ToggleAI` flips, `011f1220`);
the interface hidden (the byte `ToggleMenus` flips, `0118c6f0`, and its change call `00703810`). First person comes from
the game's own placement. Not done yet: the idle ("vanity") camera setting (the capture settles well inside its 9 s),
creatures already standing in the cell keep their idle animation, and the water and cloud animation are not pinned.

## Actors

By default the 3D of every actor but the player in the cells around the player (three cells each way) is hidden
(`NiAVObject::SetAppCulled`, `00450f90`, on the reference's 3D, slot `0x1d0`), again every 15 frames and right
before the picture. `NV_SCENE_ACTORS=1` keeps them. The log says how many were hidden.

## Determinism

`NV_REPEATABLE` is set by the runner (unless it is set already; `NV_SCENE_REALTIME=1` keeps the real clock): the game's
clock is the engine's virtual one, one fixed step a frame, so clouds, grass and water move by frame number, not by
the wall clock. Two runs of the same build then give the same pictures: five places, two runs at the same time,
mean difference 0.000 and 0.000% of pixels changed on every picture. Between two builds that differ by a frame in
when a place is ready, the difference is at most 0.074 mean (0.27% of pixels, vegas-from-the-south). Without the
virtual clock (real clock, frozen world, actors hidden) two runs differed by a mean of 0.5 to 1.8 and 1.7% to 9.9%
of pixels, from the cloud layer's scroll and swaying grass and brush (`nv-rs-scene compare --diff-out` writes the
difference picture). The cost: the virtual clock steps the threads, a five-place run takes about 48 s instead of 40 s.

`nv-rs-scene compare [--max-mean N] [--diff-out DIR] A B` prints, per picture (a PNG pair, or two folders: same-named
PNGs), the mean absolute difference over all channel values (0 to 255), the largest channel difference and the
percent of pixels with a channel difference above 8.

## Variants and the one command

```
nv-rs-scene run --viewer BIN --game INSTALL_DIR --places places.toml --out DIR \
  [--userdata TEMPLATE_DIR] [--variant NAME[=OVERLAY_DIR]]... [--settle N] [--timeout S] [--jobs 3] [--wrap WORD]...
```

Each `--variant` is the installation's data (no overlay) or with an overlay folder over its `Data` (loose
`meshes`, `textures`, `lodsettings`, and any `.esp` or `.esm` at the overlay's top, which is put in the plugin list).
The tool stages a game folder of symbolic links per variant (`fork_scene::stage`: nothing is copied or written in the
installation, a folder both sides have is merged, the overlay's file wins), starts one engine per variant (up to
`--jobs` at once, `--wrap` words start the command, for a lock or a label), then prints, per variant, the exit
status and the seconds it took, and the first variant's pictures against each other variant's. Two variants (the
game's own LOD and a generated LOD folder), five places: 51 s in all, both engines at once. An overlay mounted by the
engine's file layer, with no staged folder at all, is not built: the staging takes under a second.


## Soak and timing

`nv-rs-scene soak --viewer BIN --game DIR --places FILE --out DIR [--frames N] [--timeout S] [--realtime] [--userdata TPL] [--wrap WORD]...`
starts the engine once, visits every place of the file and runs `--frames` main frames at each after it is ready
(`NV_SCENE_SOAK_FRAMES`), then prints the average, 99th-percentile and largest frame time of each place and one verdict
line: places soaked, the engine's exit status, stops (`Game's main stopped`), error lines and warnings of the log. It
exits 1 when the engine did not end with status 0, when a stop or an error line was logged, or when a place was
missed; a run that fails ends with `ended at place NAME` and the path of the run log. A menu that stays up for 600
frames in a row fails the soak naming the top menu. `scenes/soak-far-moves.toml` is the place file: the far moves of
nv-rs#455 (Goodsprings, the NCR farms, Boulder Beach, Cottonwood Overlook, the Hoover Dam road, Black Mountain, El
Dorado Dry Lake, Novac) and two returns, ten places in one start: the old driver hung at the fourth view of a
session.

Result (default clock, shared machine, `play` profile, base game, 600 frames a place): 10 of 10 places, exit
status 0, 0 stops, 0 error lines, 141 s. The one warning is the engine's own depth stand-in line. `nv-rs#474` (a far
`player.setpos` typed at the console leaves the land unloaded) is not reproduced by this path: the runner moves the
player with the game's own placement, which loads the cells; the console `setpos` itself is not called.

Timing: add `--realtime` (the real clock; the default virtual clock steps the threads) and start under the exclusive
lock (the wrapper `flock -x LOCKFILE`), nothing else running. Ten places, 1000 frames each, offscreen 1280x720,
`play` profile: 5.7 to 10.3 ms average a frame (97 to 175 frames a second), 99th percentile 7.5 to 13.5 ms, largest
frame 9.0 to 17.7 ms, exit status 0, 120 s. A timing run under the shared lock is not valid; the same places under it
gave 9.9 to 17.9 ms.
