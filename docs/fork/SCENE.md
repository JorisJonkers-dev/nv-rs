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
# path = [[x, y, z], ...]   # optional, for a bench (see "Bench": the player walks it in a soak)
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

## The free camera: a view from above or from somewhere else

A place may take its picture from the game's free camera (what the console's `tfc` turns on) instead of the player's
eyes. The player stands where the place says, as before, and the cells around it are loaded, so they are the near land;
the camera hangs elsewhere, and everything it sees beyond the loaded cells is LOD. Judging where near land meets far land
needs this: a player on the ground sees too little of it. When the player has stood still, the runner calls the game's own
`ToggleFlyCam` (`00961e30`, with no console text) and writes the camera's heading, pitch and position into the player's
fly-camera fields (`+0x7e0`, `+0x7e4`, `+0x7e8`), again on each of the 30 frames before the picture; the game's own
`UpdateUFOCamera` places the camera from them every frame, the player is not simulated while the camera is on (it
cannot fall and nothing reacts to it) and the terrain follows the camera. The next place turns the camera off.

```toml
[[place]]
name = "goodsprings-high-east"
world = "WastelandNV"
x = -70200
y = 2560
z = 8356          # where the player stands, as always
heading = 90      # aims the camera when there is no `look` or `look_at`
pitch = 8
camera_dz = 6000  # the camera 6000 units above the player (camera_dx and camera_dy move it sideways), or
# camera = [x, y, z]      # a world position
# look = [x, y, z]        # look at a point (or `look_at = "RefEditorId"`; `heading` and `pitch` win when given)
```

A place by reference works the same (the offsets are from where the player stands). The pictures on the horizon: a
height of 6000 to 12000 over the standing point looking a few degrees down shows the near land as the loaded patch and
the LOD around it. `scenes/camera-seam.toml` (plain game) and `scenes/camera-seam-ttw.toml` (the Capital Wasteland,
`world = "Wasteland"`, on the TTW folder, the command in the Tale of Two Wastelands section) are examples; opened and
looked at: Goodsprings 6000 and 12000 units up, Tenpenny Tower from 5000 and the Washington Monument from 7000. The
run is 15 s for the two plain places and 69 s for two Capital places (17 s for one more, `--only`).

## Variants and the one command

```
nv-rs-scene run --viewer BIN --game INSTALL_DIR --places places.toml --out DIR \
  [--userdata TEMPLATE_DIR] [--variant NAME[=OVERLAY_DIR]]... [--settle N] [--timeout S] [--jobs 3] [--wrap WORD]...
```

Relative paths (`--places`, `--out`, ...) are taken from the current directory. `--game-lock FILE`
is the shorthand for the wrap words `flock -s FILE` (the shared game lock), `--label NAME [--who NAME]` names the capped scope (the Mission Center
label; `--who` defaults to `engine`); `soak` takes it too, `bench` takes `--lock` (exclusive) and `--label`.

### What caps a run's memory

Every engine start of `run`, `soak` and `bench` is a `gamerun` scope by default, with or without `--label`: a transient
systemd scope in `nv-games.slice` with `MemoryMax` and `MemoryHigh` 10 GiB (the value read from a running scope:
`systemctl --user show nvgame-<label>-*.scope -p MemoryMax` gives 10737418240), `MemorySwapMax` 0 and an out-of-memory score
that has the kernel kill the run before the desktop. The slice itself has no limit (`MemoryMax=infinity`); the limit is on
each scope, so three starts at once may use 30 GiB in all. The tool prints `memory cap: 10G per engine start (...)` first.
`--label NAME` names the scope (`NAME-<variant>` for a run; the default is `scene-<kind>-...-<pid>`), `--who NAME` says
who started it, `--mem 6G` changes the limit. `--no-cap` starts the engine with no cap and says so at the start; the
`--wrap` words that already hold `gamerun` keep their own cap. The tool refuses to start when `~/.local/bin/gamerun` is
missing and `--no-cap` is not given. The old harness's memory guard (host, VRAM and GTT checks every 100 ms, halt file)
is not ported: the scope limit is the cap, and the VRAM and GTT checks are not there.

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
runner now). Five Mojave places on a clean TTW folder in one start: 97 s. `scenes/ttw-capital.toml` has four Capital
Wasteland views by landmark (the fourth, `rivet-city-from-the-river-bank`, stands on the river's east bank and looks across the
water at the carrier with the free camera; a place 12000 units north-east of the marker sat in a loading screen and was not
reached in 150 s: the engine side of that is not looked into). Two runs of the same five places gave identical pictures in three places and differed by at most 0.62%
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

- Cheap fix (nv-rs#484): as a place is reached the runner sets the tree update's time accumulator (`011d5dd4`, the sway's clock
  `BSTreeManager::Update` adds each frame's time to) to 0, so the sway's phase at the picture no longer depends on how many
  loading-screen frames the move took. Six runs of `scenes/ttw-capital.toml` (two batches of three, three engines at once),
  all pictures identical (mean 0.000, 0 changed pixels, every pair), where one run in six differed before. One batch of six
  is not proof of a rare event (about a third of the time six identical runs happen by chance); the pin follows from the
  cause, and the loading loop itself is not pinned. The explanation below is the state before the fix.
- Two runs of one TTW build mostly give identical pictures and sometimes differ by brush clumps (at most 0.8% of pixels,
  mean 0.17; six runs: five identical, one different). `NV_SCENE_TRACE=1` logs the player's position (the bits of
  the floats) every frame; diffing the log of a differing run with an identical pair finds the first frame where
  the game state differs. It is the end of the second place's move: the game's own loading screen runs its frames
  inside the one placement call (about 14,600 of them, 48 s, in the Capital Wasteland), and the player appears at the new
  place one main frame later in the odd run (frame 15,492 against 15,493, same positions before and after). Everything
  after is shifted by one frame, which is the brush clumps' phase. So the number of loading-screen frames varies by one
  between runs although the clock, the threads (stepped) and the order of the files are fixed: something the loading loop
  polls completes in real time (a host thread's decode or upload, the drawing thread's frame, the loading movie). Not
  excluded by the trace: that a hash map with a per-process random order sits in that path. Pinning it takes finding the
  loop's wait (the exit test of the loading screen) and making every host job it polls finish before the frame
  ends under `NV_REPEATABLE`; that was not done. What works now: `nv-rs-scene compare --noise A2 A B` takes a second
  run `A2` of the first variant and leaves out the pixels where `A` and `A2` differ by more than the threshold (the
  noise), so what it prints for `B` is what the variants differ by; it also prints how many percent were left out.
  Checked on the six TTW runs: with one differing run as the baseline pair, an identical third run differs by mean 0.004
  (largest channel difference 8) and 0 changed pixels.
- The idle camera: the runner sets `fVanityModeAutoDelay` to 1,000,000 seconds through the game's own settings collection (looked
  up by name, once, as soon as the collection exists; logged). A picture taken 4000 frames after the send was first
  person with and without it: the delay counts game time, which the frozen world does not advance, so the setting is
  a safeguard, not something the pictures showed.
- nv-rs#474: `scenes/soak-setpos-474.toml` has a place with `move = "setpos"`, which sets the three coordinates and
  the two angles on the player one by one with the console's own functions (`Script::SetPosBase`, `SetAngleBase`; the
  console calls them directly when the setting `011c3ea4` is not above 1, which is the case here: `move_command`
  queues a task only above 1). The soak ends at that place: `place lake-mead-setpos was not reached within N s (screen
  Play, player at -72152 643 8291, wanted 53811 38382 3215)`. With `NV_SCENE_TRACE=1` the player is at Goodsprings'
  place (-70200 2560) until the move, then 2,000 units away from it, never near the target: a far `setpos` does not
  take the player to the new cells, which is what #474 reports (the land of the target never loads). So the soak
  fails while the defect is there and is the test for its fix. The task queue path (setting above 1) is not
  exercised.

## The owner's list against what exists

| asked for | command | missing |
| --- | --- | --- |
| engine benchmark in Rust | `nv-rs-scene bench --suite quick` (real clock, exclusive game lock, warm-up and median of passes, `summary.json`, suite table with the change against the previous suite and a baseline, `--save-baseline`), `bench-compare A.json B.json`, a walking place (`path`) | not ported (no need on this path): the nested window session, the input driver and the memory guard (the offscreen run has no window and no input; `gamerun` caps memory), the matrix of refs and hosts (`nv-rs-dev matrix`), the replay bench; the interior scenarios (Doc's house); the Contract v1 file (nv-rs#231) is not published yet, so the summary follows the old harness's keys plus `schema_version`; one machine's numbers only |
| spawning in specific locations | `NV_SCENE` place file: world, position, heading, pitch, or a reference with an offset | interiors (no cell by name), places by a worldspace that is not in the load order |
| loading into places | the runner's placement through the game's own exterior loader; one start for any number of places | no menu or intro is shown; the start is 6 s of data load (12 s on TTW) |
| no main menu needed | done: the runner sends the player from the menu in frame 121 | the start screens (intro videos) are not skipped, only passed |
| stability tests | `nv-rs-scene soak` (places, N frames each, counts stops, errors, warnings, exits non-zero) | no long-run memory growth check; #455 and #474 cases are place files (#474 fails until fixed) |
| pictures of areas, a frozen frame | `nv-rs-scene run` (variants, same place file, per-view difference) | cloud/brush phase differs by one frame between rare runs (nv-rs#484); `compare --noise` is the workaround |
| all in Rust, no shell/Python | `fork-scene` and `fork-scene-run` crates, the `nv-rs-scene` tool, `nv-rs-dev scene <build\|run\|soak\|bench\|compare\|bench-compare>` | `scripts/fork/bench/` Python still runs the old window-driven scenarios (it is not removed: nv-rs#328, #348 stay open for what is not ported) |

## Bench

`nv-rs-scene bench --lock GAME.LOCK --viewer BIN --game DIR --places FILE --out DIR [--frames N] [--warmup N] [--passes N]`
is a soak on the real clock that takes the lock exclusively (`flock -x`, waiting for shared runs to end) and writes
`OUT/soak/bench.json`. At each place the engine runs `--warmup` frames that are not timed (default 120: texture uploads
and pipeline compiles), then `--passes` timed passes (default 5) of `--frames` frames each (default 600). Per place the
file holds the frames, `avg_ms` (the median of the passes' averages: one pass slowed by outside load does not move it),
`p99_ms` and `max_ms` over all timed frames, `low1_fps`, `passes` and `spread_pct` (slowest pass against fastest, over the
median), and for the run the CPU model, the CPU frequency governor and the load average at the end (`machine`).
`nv-rs-scene bench-compare A.json B.json` prints each place's average, 1% low and worst frame in both and the change
of the average in percent (positive: B is slower). A `soak` without `--exclusive` prints "timing rows not valid, machine
shared".

Suite check: the quick suite twice (second run with `--baseline`), the average fps differed by 1.9, 0.3 and 2.4%.

Noise, the same build started twice, five Mojave places, 500 frames, `play` profile, 1280x720, exclusive lock, one
machine (Ryzen 7 5800X3D, governor `powersave`, load average 4 to 6 from other work): before (one 500-frame pass, no
warm-up) the average differed by 0.9, 4.8, 7.6, 5.0 and 1.1% between two runs; after (120 warm-up frames, five passes,
median) three runs differ by at most 2.6% (0.1 to 2.6% between the first two, 0.6 to 2.4% between the second and
third). The average itself fell by 0.2 to 1.3 ms at three places: the first frames after arriving were slower. One
place (the quarry) had a pass spread of 14 to 21% in every run, which the median covers. The remaining noise is the
machine: a `performance` governor and no other load would narrow it further.

### Suites, summary and baselines

`bench --suite NAME` takes a built-in place file instead of `--places`: `quick` (Goodsprings, Novac and the Strip, the old
quick suite without Doc's house: no interiors yet; `scenes/bench-quick.toml`), `mojave` (`mojave-far.toml`), `soak` (the far
moves of `soak-far-moves.toml`, the old `soak` scenario's places) and `capital` (`ttw-capital.toml`, needs the TTW arguments
of the Tale of Two Wastelands section). Besides `bench.json` the run writes, in the old harness's shape:

- `OUT/soak/summary.json`: `schema_version` 1, `name`, `scenario`, `settings` (mode, resolution, limiters, warm-up and
  passes), `run` (wall seconds, `noisy`, exit status, governor, load average, CPU), `metrics` (`frame_time_play` with fps,
  average, p50, p99, 1% and 0.1% low; one phase `PLACE.measured` per place; the metrics the engine path has no source
  for, main busy, VRAM and hitches, are `null`), `verdict` (the checks: exit status 0, no stop, no error line) and the
  `headline` of the first place. A run is `noisy` when another `nv-viewer` was running at its start or end.
- `OUT/soak/suite.md` and `STATE/<suite-name>/suite.json`, `suite.md`: the headline table (the old columns, one row `suite:place`
  per place, verdict PASS, FAIL or CAPPED: a frame limiter in the environment or a rate within 2% of a usual cap value is
  not a headline) with the fps change against the previous suite of that name and the baseline.
- `--save-baseline NAME` stores the rows in `STATE/baselines/NAME.json`, `--baseline NAME` compares with one (the old
  harness's baseline files are read too). `STATE` is `--state DIR`, default `~/.cache/nv-rs/scene-bench`.

A place with `path` and `path_speed` walks: during the timed frames the player is moved along the points with the game's own
position setters, `path_speed` units a frame (the loop again at its end), and the cells stream in as for a walking player;
`scenes/bench-walk.toml` has one. Checked: 600 frames in the Goodsprings valley, the player at -70160..-69000, 2560..3799,
height within 3 units; the frame times show the streaming (average 13.4 ms against 7 standing, 99th percentile 47 ms,
largest 104 ms; a soak, so not a valid timing).

## Where the seconds go, and the speed options

One `run` of five Mojave views (`scenes/mojave-far.toml`), plain game, 1280x720, wall time from the log's timestamps:
the process start to the game's code 6 s (data load: nothing to cut), the menu 0.4 s (120 frames), the first place
about 5 s to load its cells plus 7 s to settle (the new game's item messages open message boxes each 75 frames, each
closed and each restarting the settle), then each further place 2 to 5 s to move and 3 to 9 s to settle, 1 to 2.5 s
before the next one (it waited 120 steady frames and wrote the PNG), the PNG itself well under 0.1 s. On the TTW folder
the start is 12 s and a far move inside the Capital Wasteland (28,000 units) took 48 s in one call, the game's own cell
load with its loading screen: the one large cost that is the engine's, so put places that are close to each other next to each
other in a place file (a far move costs what loading the new cells costs).

What changed, five Mojave views, two variants (the game's own LOD and a generated one, both started at once):

| | before | after |
| --- | --- | --- |
| default (the later places wait 5 steady frames, not 120; the settle is 120 frames, not 200) | 56 s | 39 s |
| `--quick` (640x360, settle 100) | | 35 s |
| `--only helios-one-across-the-road` (one view, to retake it) | | 17 s |

Variants already run side by side (`--jobs 3`, the game lock allows three); a variant costs no more than one. A settle of 60
frames is too short: the load's blur and the mouse cursor are still in the picture (opened and looked at), 100 is sharp;
the default 120 leaves a margin, and `--settle N` sets it. Pictures taken at different settle frames differ in the
clouds, so compare only pictures made with the same settings. `--only a,b` retakes the named places of the file.
