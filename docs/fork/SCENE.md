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

## Tale of Two Wastelands

A load order is a variant like any other: `--game` is the base installation, the overlay is the TTW data folder
(`--overlay-skip meshes,textures,lodsettings` leaves its loose LOD out, so TTW's own archived LOD is the control),
`--plugins` is the load order (`FalloutNV.esm,...,TaleOfTwoWastelands.esm,YUPTTW.esm`, as `plugins.txt` holds it).
Worlds are found by editor id (`WastelandNV`, the Capital Wasteland's `Wasteland`).

Two defects on the way: a start with the 18 TTW plugins in the fixes preset stopped with `JohnnyGuitarNVSE: registered
twice` (nv-rs#482: the editor ids crate and the plugin crate both registered the plugin; one record now carries both
halves); and the second place of a start timed out on the Menu screen (the new game's message boxes, closed by the
runner now). Five Mojave places on a clean TTW folder in one start: 97 s. `scenes/ttw-capital.toml` has three Capital
Wasteland views by landmark. Two runs of the same five places gave identical pictures in three places and differed by at most 0.62%
of pixels (brush clumps, mean 0.16) in two; the runs fall in two groups of identical pictures, cause not traced.

## Places by reference

A place may stand at a reference (`ref`, an editor id or a form id in hex) plus an offset (`dx`, `dy`, `dz`, world axes)
and face another (`look_at`; `heading` and `pitch` win when given), so a landmark view needs no hand-found coordinates
and follows a load order that moves things. Map markers are persistent references with editor ids
(`WashMonMapMarker`, `TenpennyTowerMapMarker`, `VHeliosONEMapMarkerREF`, `BlackMountainMapMarkerREF`, ...); the
editor ids of any plugin come from the fixes preset's editor id store. `world` is needed on a place by reference for the first place of a start
(its cell is not loaded yet, so the reference has no world); the log says where each place resolved to. The default
files: `scenes/mojave-far.toml` (five Mojave views) and `scenes/ttw-capital.toml` (three Capital views). The offsets
were found by looking: a reference's own spot is often among rocks or camps, so a landmark view stands thousands of
units away; one that ends against a hillside needs another offset.

## A generated LOD for a load order as a variant

A variant can name more than one overlay, joined with `+`: later ones are laid over the first (their files win, a folder
both have is merged). For a Tale of Two Wastelands load order the first is the TTW data folder (left without its own loose
LOD: `--overlay-skip meshes,textures,lodsettings`, which applies to the first overlay only) and the second is the folder
lodwright wrote (`meshes/`, `textures/`, `lodsettings/` loose; for a packed result the folder holding the archive and its
stub plugin). `--plugins` is always given for such a load order: the 18 plugins in order, then the stub plugin after them
when the LOD is packed (a stub that names a master registers the archive; no other setting).

One command, TTW's own LOD against a generated one, the same place file, per-view difference printed:

```
nv-rs-scene run --viewer nv-viewer --game "$STEAM/Fallout New Vegas" --places docs/fork/scenes/ttw-capital.toml --out out \
  --resolution 1920x1080 --userdata "$TEMPLATE" \
  --plugins "FalloutNV.esm,DeadMoney.esm,HonestHearts.esm,OldWorldBlues.esm,LonesomeRoad.esm,GunRunnersArsenal.esm,Fallout3.esm,Anchorage.esm,ThePitt.esm,BrokenSteel.esm,PointLookout.esm,Zeta.esm,CaravanPack.esm,ClassicPack.esm,MercenaryPack.esm,TribalPack.esm,TaleOfTwoWastelands.esm,YUPTTW.esm" \
  --overlay-skip meshes,textures,lodsettings \
  --variant "ttw-own=$TTW_DATA" \
  --variant "ttw-generated=$TTW_DATA+$GENERATED_LOD_OUT"
```

For a packed generated LOD add the stub at the end of `--plugins` (`...,YUPTTW.esm,Lodwright LOD.esp`); the stub and its
archive sit at the top of `$GENERATED_LOD_OUT`. Checked with the plain game's generated folder laid over the TTW folder
(a single Goodsprings place, two variants, 25 s); a complete TTW generated folder was not available to run.

## The two TTW groups, the fixed capture frame, the idle camera, #474

- Two runs of one TTW build fall in two or three groups of identical pictures; the groups are not the thread mode (real
  and stepped both run stepped under `NV_REPEATABLE`), not the directory order of the staged folder, not the pipeline
  cache, and not the frame a picture is taken at: `NV_SCENE_CAPTURE_FRAMES=N` takes it exactly N frames after the place
  was sent (a place ready sooner waits, later fails), and three runs with it still gave two groups (one of three
  differed in Goodsprings and two other places by up to 2.8% of pixels). The ready frame itself differs by one between
  groups, so the simulation differs a little: the candidates left are a host-side hash map whose iteration order is
  random per process (the standard library's) in a loading or physics path, or an order that depends on the
  address a buffer was given. Pinning it needs that site found (a trace of the first frame where two runs' game state
  differ, e.g. the player's height each frame); it was not.
- The idle camera: the runner sets `fVanityModeAutoDelay` to 1,000,000 seconds through the game's own settings collection (looked
  up by name, once, as soon as the collection exists; logged). A picture taken 4000 frames after the send was first
  person with and without it: the delay counts game time, which the frozen world does not advance, so the setting is
  a safeguard, not something the pictures showed.
- nv-rs#474: `scenes/soak-setpos-474.toml` has a place with `move = "setpos"`, which sets the coordinates and angles
  on the player one by one as the console's `player.setpos` and `setangle` do (`Script::SetPosBase`, `SetAngleBase`),
  without asking for a cell. As built it does not reproduce #474 yet: the player did not move to the target (the soak
  ends with `place lake-mead-setpos was not reached within 40 s (screen Play, player at -72152 643 8268, wanted 53811 38382
  3215)`), so the call path is not the console's: the console goes through `move_command` and its task queue
  (`TASK_SET_POSITION`), which this does not.
