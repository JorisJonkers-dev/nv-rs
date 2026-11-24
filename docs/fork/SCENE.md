# Scene capture

Fork-only. Tracking issue: nv-rs#479 (design, slices, open questions). A scene run starts the engine, sends the
player to named places the game's own way, with no main menu and no typed console text, and (in later slices) freezes
the world, waits for a real "ready" condition and writes one frame as a picture.

This page describes what is built. The design and the order of the slices are on the issue.

## First use

Once: `nv-rs-dev scene build` (the viewer and the `nv-rs-scene` tool; `NV_DEV_TARGET` is the build folder). `GAME` below is the
shared game lock, `~/Games/"Fallout New Vegas RS"/game.lock`; every start is a memory-capped, labelled `gamerun` scope. Run
from the repository. The game's own folders and profile are not written: each start gets its own staged folder under `--out`.

**(a) A picture pair: two variants of the game's data at one place** (here the game's own LOD and a generated one, one
start each, side by side, the difference printed):

```
nv-rs-dev scene run --places docs/fork/scenes/camera-seam.toml --only goodsprings-high-east --out /tmp/pair \
  --variant game --variant lodwright=/path/to/generated/LOD/folder --game-lock "$GAME" --label first-use
```

```
game: ok, 13 s, pictures in /tmp/pair/game/pictures
lodwright: ok, 13 s, pictures in /tmp/pair/lodwright/pictures
--- lodwright against game
goodsprings-high-east.png: mean 7.577, max 176, changed 30.595% of pixels (channel difference above 8)
```

The two PNGs are `/tmp/pair/game/pictures/goodsprings-high-east.png` and `/tmp/pair/lodwright/pictures/...`: open both. (Tried
with the Mojave's generated overlay of the 2026-11-21 trial: the generated picture lacks the far buildings and rock walls the
game's own LOD shows and has a flat water sheet where the game has the lake.) Two lenses at one place instead: put both
in the place file (`scenes/lens-test.toml`) and run with `--only lens-plain,lens-nofog`, one variant.

**(b) A soak: ten far moves, 300 frames at each, counting stops and errors**

```
nv-rs-dev scene soak --places docs/fork/scenes/soak-far-moves.toml --out /tmp/soak --frames 300 --game-lock "$GAME" --label first-soak
```

```
place, frames, frame times (ms):
  goodsprings frames 300 avg_ms 12.694 p99_ms 34.71 ...
  ... (one line a place)
soak: 10 of 10 places soaked for 300 frames, exit status 0, 0 stops, 0 error lines, 1 warnings, 103 s
```

The exit status is 0 only when every place was reached and nothing stopped. The frame times of a soak are not a measurement
(the line "timing rows not valid, machine shared" says so).

**(c) A bench against a baseline** (the real clock, the lock taken exclusively, 120 warm-up frames and the median of five
passes of 600 frames at each place; three places, 90 s):

```
nv-rs-dev scene bench --suite quick --lock "$GAME" --out /tmp/b1 --save-baseline mine --label first-bench   # once
nv-rs-dev scene bench --suite quick --lock "$GAME" --out /tmp/b2 --baseline mine --label first-bench       # after a change
```

```
| Scenario | avg fps | p50 ms | p99 ms | 1% low | ... | verdict | noisy | fps vs previous | fps vs baseline |
| quick:goodsprings | 132 | 7.29 | 10.24 | 91 | ... | PASS |  | -2.4% | -2.4% |
...
Baseline: mine
summary: /tmp/b2/soak/summary.json
```

Two runs of the same build on this machine differed by 2 to 5% in average fps (other work was running): a change smaller than
that is not a finding. A place at a usual cap value (60, 120, 144 fps) is marked CAPPED and is not a headline number. The
baselines are under `~/.cache/nv-rs/scene-bench` (`--state DIR` moves them).

**(d) The stability sweep, before every engine release:** every place of a list loads, holds 300 frames and leaves cleanly;
a place that stops, hangs or is put back is recorded with the end of the log and the sweep goes on from the next place
(the engine is started again). One command a list (`scenes/sweep-*.toml`; the Capital lists need the TTW arguments of the
Tale of Two Wastelands section):

```
nv-rs-dev scene sweep --places docs/fork/scenes/sweep-mojave.toml --out /tmp/sweep --game-lock "$GAME" --label sweep-mojave
```

The exit status is 0 only when every place passed; `OUT/sweep.md` lists each place with its frame times or its failure. The
other lists: `sweep-mojave-interiors.toml`, `sweep-capital.toml`, `sweep-capital-interiors.toml`. A silent log for `--stall` seconds
(default 120) counts as a hang: the start is killed.

## Using this from another program

The command-line tool is a thin caller of a library: `fork_scene::tool` (crate `fork-scene`, no engine code, no game data).
One public function per command, each taking a typed request and returning a typed result:

| command | function | request | result |
| --- | --- | --- | --- |
| `compare` | `tool::compare` | `CompareRequest` | `CompareResult` (rows of mean, max, changed %) |
| `run` | `tool::run` | `RunRequest` (`Start` + variants) | `RunResult` (a `VariantResult` per start, the comparisons) |
| `soak` | `tool::soak` | `SoakRequest` | `SoakResult` (typed `PlaceRow` timings, stops, errors, warnings) |
| `sweep` | `tool::sweep` | `SweepRequest` | `SweepResult` (passed and failed places, `sweep.md`) |
| `bench` | `tool::bench` | `BenchRequest` | `BenchResult` (headlines, `summary.json`, the suite table) |
| `bench-compare` | `tool::bench_compare` | `BenchCompareRequest` | `BenchCompareResult` |

Rules of the library: it never prints and never ends the process. What the tool prints while it works (the memory cap, a
start's memory, a hang) goes to a `say: &(dyn Fn(&str) + Sync)` the caller gives (`&|_| {}` to ignore it); what it
prints at the end is the result's `summary()`. A failure of a start (the engine stopped, hung, was refused) is a field of
the result; an `Err(String)` is a request that cannot be carried out. `sweep` runs in the calling process; the
command-line tool runs it in a child of its own so a killed sweep is reported (a program that needs that does the same).

**The game child start is a hook.** The library stages the game folder and decides the environment; a `Launcher` starts the
engine:

```rust
pub trait Launcher: Sync {
    fn describe(&self, concurrent: usize) -> String;                       // one line about the memory cap
    fn launch(&self, spec: &StartSpec, log: File) -> Result<Launched, String>;
}
```

`StartSpec` holds the viewer, the staged game folder (also the working directory), the whole environment, the number of
starts at once and the game lock to hold (`spec.lock`, shared for pictures, soaks and sweeps, exclusive for a bench). The
launcher puts the child in its own scope with its own label and memory limit, takes the lock, sends the child's output to
`log`, and returns the `Child` (and `scope_prefix`, the start of the systemd scope's name, if there is one, so the library
can read its memory and kill it when the engine hangs). `CappedLauncher` is the command-line tool's: `flock`, then a
`gamerun` scope with `MemoryMax` and no swap.

A worked example: a soak of a place file in the caller's own child, with the caller's own label and a 10 GiB cap.

```rust
use fork_scene::tool::{self, Launched, Launcher, SoakRequest, Start, StartSpec};
use std::{fs::File, path::PathBuf, process::Command};

struct Mine;
impl Launcher for Mine {
    fn describe(&self, _concurrent: usize) -> String {
        "memory: 10G, scope raul-scene".into()
    }
    fn launch(&self, spec: &StartSpec, log: File) -> Result<Launched, String> {
        let mut cmd = Command::new("systemd-run");
        cmd.args(["--user", "--scope", "--quiet", "--unit", "nvgame-raul-scene-1", "-p", "MemoryMax=10G", "-p", "MemorySwapMax=0", "--"])
            .arg(&spec.viewer).arg(&spec.game_dir)
            .current_dir(&spec.game_dir)
            .envs(spec.env.iter().map(|(k, v)| (k, v)))
            .stdout(log.try_clone().map_err(|e| e.to_string())?)
            .stderr(log);
        let child = cmd.spawn().map_err(|e| e.to_string())?;
        Ok(Launched { child, scope_prefix: Some("nvgame-raul-scene-".into()) })
    }
}

fn main() -> Result<(), String> {
    let mut start = Start::new(
        PathBuf::from("/opt/raul/nv-viewer"),
        PathBuf::from("/games/Fallout New Vegas"),
        PathBuf::from("places.toml"),
        PathBuf::from("/tmp/raul-soak"),
    );
    start.only = vec!["goodsprings".into()];
    let mut request = SoakRequest::new(start);
    request.frames = 300;
    let result = tool::soak(&request, &Mine, &|line| eprintln!("{line}"))?;
    for row in &result.timings {
        println!("{}: avg {:.2} ms, 1% low {:.0} fps", row.place, row.avg_ms, row.low1_fps);
    }
    if result.ok { Ok(()) } else { Err(result.summary()) }
}
```

`run`, `sweep` and `bench` are called the same way with their own request (`RunRequest::new(start)`, `SweepRequest::new(start)`,
`BenchRequest::new(start, lock_file)`); `compare` and `bench_compare` need no launcher. The tests in
`crates/fork-scene/src/tool.rs` use a launcher that starts a shell script in place of the engine.

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

## Interiors

A place with `cell` instead of `world` is an interior: the editor id of the cell. The runner finds the cell
(`TESDataHandler::GetCellByEditorID`, `00461ae0`, the lookup `coc` uses) and puts the player in it with the game's own
placement into a cell, `PlayerCharacter::PositionPlayer` (`0093c200`), the function a door and `coc` end in; from the play
screen the game's queued request for it (`RequestPositionPlayer`, the way a door's teleport goes: the player update places the
player and sets the character controller's height). The place is reached when the player's parent cell is that cell; the
frozen world, the hidden actors, the free camera (`camera_dz`), soak and bench work the same as outdoors (the grid of
loaded cells is empty: the one cell is the world).

```toml
[[place]]
name = "docs-house"
cell = "GSDocMitchellHouse"   # alone: the cell's own start marker (what `coc` uses), facing its heading
# dx = 0  dy = 0  dz = 0      # an offset from that marker, or
# x = 0   y = 0   z = 0       # a position in the cell's own coordinates, or
# ref = "SomeMarkerRef"       # a marker or door reference placed in the cell (editor id or form id in hex) plus dx, dy, dz
```

A door is the other way in: `door = "ProspectorSaloonDoor"` (a door reference placed in the world, editor id or form id in hex)
goes where the door's teleport leads, an interior cell or another world space, with the position and facing the door gives,
through the game's request for it. The door must be a form, which it is while its cell is loaded: put an exterior place near it
before it in the file (`scenes/sweep-mojave-interiors.toml` does: Goodsprings, then the Prospector Saloon's door and the gas
station's, both opened). From the menu a door place is refused with a line saying so.

A `ref` in an interior is a form only while its cell is loaded: for a cell not yet entered the lookup fails with a line saying
so, and the cell's start marker or coordinates are the way in. A `cell` that names an exterior cell is refused with a plain line.
Files: `scenes/interiors.toml` (Doc Mitchell's house, the Tops casino floor, the Lucky 38 casino floor from the marker and
from above), `scenes/ttw-interiors.toml` (Tenpenny Tower's lobby, Moriarty's saloon, Vault 101's atrium: needs the TTW
arguments); the quick suite holds Doc's house and the Tops floor, the capital suite Tenpenny's lobby. Opened: all of them;
Doc's house is a panelled hall with a carpet and two wall lamps, the Tops floor a round reception with a counter and a
neon treble clef, the Lucky 38 floor its arch with the slot machines (from the start marker and from 500 units above it),
Tenpenny's lobby a dining room with a cash register and two mugs on a counter. Soak of `interiors.toml`, 300 frames each:
4 of 4 places, 0 stops, 0 errors; 7.5 ms average in Doc's house and 15 to 19 ms in the casinos (a shared machine).

## How the engine goes to a place

The runner (`fork_scene_run::Runner`) runs inside the game's frame, on the game's main thread, at the entry of `Main::OnIdle` (`0086e650`): `fork_scene_run::install` registers a function there that runs the runner and then the translation. (Calling the game's functions from the host's thread between frames does not work: the load waits for loader threads while the host holds the kernel lock, and the call spins.) The runner waits for the main menu, then calls the game's own
`PlayerCharacter::PositionPlayerExterior` (`0093cce0`, the function fast travel and `moveto` end in: it cancels pending
exterior loads, loads or creates the cell and places the player on its land) with the place's world, position and
heading, once the screen has been the menu for 120 frames (the game's first frames still start its threads). The place is reached when the screen is the play screen and the player's position lies in the place's grid
cell. Later places are sent from the play screen the same way.

Limits: none for the world: any world space of the load order by editor id (or form id in hex). The frozen world (hour, weather, creatures, interface hidden) is the next slice.

## No main menu

With `NV_SCENE_MENU=0` (`--env NV_SCENE_MENU=0`) the game never builds its main menu in a scene run. This is not the default yet: its pictures differ slightly from the menu route's and between runs (below), so the menu route stays the default until they are identical. `main` (`0086a850`)
calls `StartMenu::Create(0, 0)` (`007053f0`) once before its message loop when the player has no world space (a fresh start);
the scene run registers an empty function at that address before the game's code starts (`host::fork_prestart`, a registration made
at `Host::start_with`, because the game runs up to its first frame inside it). With no menu there is no menu music, no menu
assets and no start screen to close; the game runs its own first frames with a player that has the defaults the data load gave
it (the default player record, the globals, the calendar), and the runner sends it to the first place with
`PositionPlayerExterior` as before (the flag `ConfirmNewGame` sets, `011d8907`, is set by the runner; `StartMenu::Close` is
only called when a menu is up). The new game's quest and item messages are not started (as before: the menu route never ran
`ConfirmNewGame` either).

Until the first send the world is held (`Main` +7, the free camera's frozen world flag: the sky, the calendar and the
animations stop), as the menu's menu mode held it, so the frames before the send change nothing; the first place is sent at
frame 121 as before. Without the setting the menu is built and the first place is sent from it, the old route.

Measured (one place, wall time from the process start, two starts each, machine shared so the numbers move by seconds):

| | plain game, menu | plain game, no menu | TTW folder, menu | TTW folder, no menu |
| --- | --- | --- | --- | --- |
| the game's code runs (frame 1) | 5.0, 6.0 s | 4.9, 6.6 s | 16.3, 14.2 s | 16.4, 13.9 s |
| the player is sent | 5.4, 6.5 s | 5.3, 7.3 s | 17.1, 14.9 s | 16.9, 14.4 s |
| the picture is written | 12.4, 14.8 s | 13.5, 19.6 s | 25.2, 21.8 s | 23.2, 20.8 s |

The 120 frames before the send take 0.4 to 0.8 s with and without the menu: the menu itself costs nothing measurable here, and the route without it is not
faster because the first place is still sent at frame 121 (so that the frames before the picture are the same). The seconds
are the data load (the game's code at 5 s, 14 to 16 s on TTW), the first place's cell load (5 to 6 s, one blocking
call of the game's own loader) and the settle (2 to 3 s). One part was cut: the drawing layer's pipeline cache and the driver's
shader cache. Each start used to get an empty cache folder, so the 767 shader modules were compiled again each time (1.4 s);
`nv-rs-scene` now runs every start with one cache folder kept between starts (`~/.cache/nv-rs/scene-cache`, the pipeline
cache is written under a temporary name and renamed, so starts at the same time share it; `NV_RS_SCENE_COLD_CACHE=1` or
`--env XDG_CACHE_HOME=DIR` changes it). Warm: the module step 1376 ms to 149 ms, the picture the same (compare: mean 0.000,
0 changed pixels, cold against warm). What was not cut: the plugin data (the ESM parse, about 3.7 s of the 5 s on the plain game
and most of the 14 s on TTW) is the game's own code at its own pace; a cache of the parsed records would have to be written as a
snapshot of the game's memory after the load, keyed by the plugin files' sizes and times, which is not done.

Pictures against the menu route: the same place taken at the same frame is the same picture within the jitter of a player
that settles 0.17 units differently (the menu route's player stands still after the placement, the player of the route without a
menu is simulated): Goodsprings from 6000 units up, plain game: mean 0.06 (0.1% of pixels over 8); a quarry view with the capture frame
fixed (`NV_SCENE_CAPTURE_FRAMES=700`): mean 1.8, 0.33% of pixels (gravel glitter, opened and looked at); TTW Washington
skyline: mean 4.0, 1.5% (two pairs gave the same numbers: a fixed difference, not noise). Where the ready frame decides the picture
(brush sway of places taken after another one, which wait for the new game's message boxes) the two routes differ, as two
places lists with a different order do: compare pictures made by the same route and the same place list.

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

## Night: `hour` and `weather`

A place may hold its own game hour and weather in the frozen world: `hour = 22.5` (from 0 up to 24) and
`weather = "<editor id or form id in hex>"`; the run's `NV_SCENE_HOUR` (12) and `NV_SCENE_WEATHER` (the clear weather
`000FFC88`) are the defaults. `scenes/night.toml` has the Strip skyline from the south and Goodsprings at 22.5. Opened:
the Strip's towers and casino windows lit with the far mountains dark behind, Goodsprings with its street lamp glowing,
the lit windows of the Prospector Saloon and a neon sign. Lights switched by the hour (street lamps, neon, lit windows)
come on in the frozen world: the game's lighting reads the hour from the calendar when it draws and updates, which the
runner holds, and no script or timer is involved. Lights that a script switches on at dusk (a quest's, a mod's) would not.

## The lens: field of view, far plane, fog

Any place (the player's eyes or the free camera) may set the lens:

```toml
fov = 20        # degrees; a narrow lens looks at one far thing as if it were near
far = 150000    # the far plane's distance in units
fog = false     # no fog: far land and objects at full contrast
```

Why a plain write does not work, and where each is set: the game derives all three every frame, so each is taken at its
source (`crates/fork-scene-run/src/view.rs`).

- `fov`: the world camera's angle is the player's `fWorldFOV` (`+0x670`; `+0x674` is the first-person weapon's), moved each
  frame toward the setting `fDefaultWorldFOV` by `0095de30`. The place's value is written after that function and, while the
  free camera is up (the player is not simulated then), by the runner each frame. The game's `fov` is the angle of a 4:3
  picture: the vertical half-angle's tangent is `0.75 * tan(fov / 2)` whatever the picture's shape, so a square picture
  (`--resolution 1024x1024`) shows a square and a 16:9 one is wider than 4:3 would be.
- `far`: in an exterior with distant terrain the scene graph's far distance (`SceneGraph::GetFarDistance`, `008786b0`) is one
  fixed number, 353840 units in this game, not the `fNoLODFarDistance` settings; the place's value replaces the answer, and the
  game's own sky and atmosphere updates then move the frustum's far plane and the fog's far distance to it.
- `fog`: the sky recomputes its fog distances from the weather every frame and the atmosphere reads them through `00923000`
  (a folded accessor other classes share); with `fog = false` the sky's own call is answered with distances of 1e8 and 2e8.

Proved by pictures, opened: `scenes/lens-test.toml`, Goodsprings from 6000 up looking east, the same view with each key
(1280x720). `fov = 20` shows the Goodsprings cross and a far tower filling the frame; `fog = false` shows the far
mountains and the lake in full colour, where the plain picture fades them into haze; `far = 15000` ends the land at a
line with a bright haze (the fog pulled in to the far plane) between it and the sky; `far = 400000` looks like the plain one (the game's own is 353840).
Earlier tries wrote `fDefaultFOV`, `fNoLODFarDistance*` and the sky's fields once from the runner: pictures identical to
the plain ones, and the environment switches `NV_SCENE_FOV`, `NV_SCENE_FAR`, `NV_SCENE_NOFOG` that did that are gone.

## A straight-down picture of a block of land (measurement)

`scenes/topdown-test.toml`: the player stands on the ground (its cells are the loaded ones), the free camera hangs 60000
units above the block's centre and looks straight down (`pitch = 89`, the place file's limit, so the camera sits 1047 units
south of the centre), `fog = false`, and a narrow lens, `fov = 23.9` for the Mojave and 20.4 for the Capital Wasteland,
so a 4 by 4 cell block (16384 units) fills a square picture (`--resolution 1024x1024`). Not a feature: a measurement.

```
nv-rs-scene run --viewer BIN --game INSTALL --places docs/fork/scenes/topdown-test.toml --only mojave-block \
  --out OUT --resolution 1024x1024 --userdata TEMPLATE --game-lock GAME.LOCK --label topdown
```

Opened: `mojave-block` (Goodsprings: road bend, houses, the cliffs west), `mojave-novac-block` (Novac: the dinosaur and the
buildings around it) and `capital-block` (Tenpenny Tower and its plain, TTW folder, the command in the Tale of Two
Wastelands section). Each is a plan view with the game's light and a square 16384-unit edge to within a few percent.
Calibration, the only measure used: two Goodsprings pictures with the camera 2048 units apart shifted the road bend by
136 pixels, that is 15400 units across 1024 pixels at the ground (the fov was then raised from 22.5 to 23.9). The ground
is not flat, so a tall hill or a roof is nearer the lens and larger.

- Loaded cells: the game loads a square of `uGridsToLoad` by `uGridsToLoad` cells around the player's cell (5 by 5 = 25 by
  default; the runner logs it at each picture, `scene: NAME: uGridsToLoad 5: cells x -20..-16 y -2..2, 25 of 25 hold a
  cell`). Beyond it only the distant terrain and the object LOD are drawn.
- Can a place raise it? No. `uGridsToLoad` is read once when the game starts (`TES` sizes its cell arrays from it), so it is
  a property of the engine start, not of a place. A run can: `nv-rs-scene run|soak|bench --grids 9` (an odd number, 3 to
  21) or `--ini KEY=VALUE` rewrites the line in the staged copies of `Fallout.ini` and `FalloutPrefs.ini`, in each that
  has the key (the game reads `FalloutPrefs.ini` over `Fallout.ini`, and display settings such as `iMaxAnisotropy` live
  there; the template and the owner's profile are not touched). Measured with 5, 7 and 9: 25, 49 and 81 cells loaded, one place 12 s, 14 s and 14-16 s from process
  start to the picture, memory of the start 1.2 to 1.4 GB, 1.3 GB and 1.3 to 1.6 GB.
- Does it matter for a 4 by 4 block? Not when the block lies inside the loaded square: the Goodsprings block with 5, 7 and 9
  differs by a mean of 0.04 of 255 (0.17% of pixels). A 12 by 12 cell view (`fov = 61`, camera 60000 up) does: with 5 the objects
  (roads, houses, rock walls) end at the loaded square's edge and the rest is distant terrain; with 9 more of them are there
  (mean difference 6.5, 34% of pixels), and `--grids 9` gives the same picture as the INI edited by hand (difference 0.000).
- Cost of a tile, wall time from the process start (Ryzen 7 5800X3D, 1024x1024, shared machine): one tile in one start 12-13 s
  (the first 6 s are the game's data load) and 20 s for the Capital Wasteland (12 s data load); each further tile in the
  same start 6 s (18 s for Goodsprings then Novac). A far move is a cell load: tiles near each other cost less.
- Limits: the camera cannot look exactly down (89 of 90 degrees), the picture is a perspective view (a block's edge is
  farther from the lens than its centre, and objects stand out of the plane: roofs are larger than their feet), the fov that
  fits a block depends on the ground's height (calibrate once per area by moving the camera a known distance), and weather
  and light are the frozen world's. Baking a tile into anything is not built and not decided.

## The stability sweep

`nv-rs-scene sweep` (`nv-rs-dev scene sweep`) takes the arguments of `soak` (300 frames, `--timeout 120` a place, `--stall 120`
unless given) and runs every place of a list. A place that does not load, stops, is put back or leaves the play screen ends that
engine start with a plain line; the sweep records the place with the engine's error lines and starts the engine again at the
places after it, so one run says which places fail, not only the first. A start whose log stays silent for `--stall` seconds is
a hang: its scope is killed and the place recorded. `OUT/sweep.md` is the report, the exit status 0 only if every place passed
and the engine left cleanly. The lists:

| list | places | what |
| --- | --- | --- |
| `sweep-mojave.toml` | 80 | 36 map markers of the Mojave Wasteland, then the other worlds of the base game and the add-ons by a placed reference (Freeside, the Strip, Lucky 38, the Fort, Boulder City, Gomorrah, the Villa and its towns, Zion, Big MT, the Lonesome Road worlds) |
| `sweep-mojave-interiors.toml` | 33 | 18 base-game interiors, 12 add-on interiors, then two doors |
| `sweep-capital.toml` | 42 | 21 map markers of the Capital Wasteland, 21 world spaces (districts and the add-ons), on the TTW folder |
| `sweep-capital-interiors.toml` | 31 | 17 Capital interiors, 10 add-on interiors, four more places for two doors, on the TTW folder |
| `sweep-addons-mojave-interiors.toml` | 76 | the named interior cells of the five Mojave add-ons (Dead Money 15, Honest Hearts 16, Old World Blues 31, Lonesome Road 14, Gun Runners' Arsenal 0), by editor id |
| `sweep-addons-capital-interiors.toml` | 120 | the same for the five Capital add-ons on the TTW folder (Anchorage 11, The Pitt 11, Broken Steel 23, Point Lookout 54, Mothership Zeta 21) |

The add-on lists are made from the plugins' cell records: every interior cell the plugin itself defines (its own form-id range),
without those whose editor id or name says test, debug, dummy, prefab, slideshow, encounter and the like, or whose name is "None".
Against what exists: the Mojave add-ons define 104 interior cells and the list has 76; the Capital add-ons 177 and the list 120.
The cells left out are the test and holding cells; "has a start marker or a door into it" is not checked, the sweep finds out
(a cell without a start marker would show as a failing place).

Add-on interior lists (viewer 52): the Mojave add-ons 76 of 76; the Capital add-ons 119 of 120, reproducibly (two full runs, the same
frame): the engine stops moving into `DLC04MedicalTent04` after 64 earlier interior moves, "free of ... which is not a live heap
block" (nv-rs#487); alone, or after the three places before it, the place loads.

Run on 2026-11-22 (viewer `nv-viewer-scene52`, one start each): `sweep-mojave` 80 of 80 places, exit status 0, 13 min;
`sweep-capital` 42 of 42, 9 min; the two interior lists 33 of 33 and 31 of 31 (4 min each, viewer 50). No place stopped, hung or
was put back. The sweeps before the last one stopped twice in the Mojave and once in the Capital list, always at the 21st to 60th
place of one start with the top menu `0x403`, the level-up menu: each far move onto a map marker's land is a location
discovered, and the experience of about twenty discoveries is a level. Both of those places pass alone. The frozen world now sets
the experience settings of finding places (`iXPRewardDiscoverMapMarker`, `iXPRewardDiscoverSecretArea`) to 0, and god mode (the
byte `tgm` flips) on. That the level-up menu was the cause is an inference: the menu id `0x403` is the level-up menu's class, the stops came at
different places in different runs, the places pass alone, and the full runs pass once discovery experience is off; no run
logged the level itself. It is a harness cause, not an engine defect: a player who gains a level sees the same menu. The setting
belongs to the frozen world (`NV_SCENE_FREEZE` not `0`), which every picture run, soak, bench and sweep has, so a far-view list
of twenty or more places is protected the same way (the log line `scene: no experience for finding places` shows it).

The sweep's own report is written as it goes: `sweep.md` starts as "(in progress ...)" with the table header, a row is added
for each place as the engine logs it and for each failure as it is found, and the summary replaces the file at the end. The
sweep runs in a worker process watched by the command you start: a worker that is killed or panics leaves the rows it had,
a last line "sweep: the sweep ended without its summary (killed by signal N)" in the report and on stderr, and exit status 2.

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
systemd scope in `nv-games.slice` with a hard `MemoryMax` and `MemoryHigh`, `MemorySwapMax` 0 and an out-of-memory score that
has the kernel kill the run before the desktop. The slice itself has no limit; the limit is on each scope.

What a start really uses (the scope's `memory.peak` and its anonymous memory, read every 200 ms while it runs and printed
at the end of each start as `memory NAME: peak ... MiB ..., ... MiB anonymous at most`, with the same text in
`OUT/NAME/memory.txt`): the plain game, five Mojave places at 1920x1080, peak 1700 MiB, 1340 MiB anonymous; a Tale of Two
Wastelands folder, four Capital places, peak 1934 MiB, 1827 MiB anonymous; a bench (quick suite, three places, 3000 timed frames each), peak 1490 MiB, 1363 MiB anonymous. The peak counts the page cache of the files
the run read, which the kernel gives back under pressure.

The default is a total budget for the invocation: 12 GiB, shared evenly by the starts that run together (`run` starts up
to `--jobs` variants at once, `soak` and `bench` one), never below 4 GiB (twice the largest peak) and never above 6 GiB
(three times) for one start. So one start gets 6 GiB, two get 6 GiB each, three get 4 GiB each (12 GiB in all). The tool
prints the numbers first (`memory cap: 4096M per engine start (the budget of 12288 MiB for the whole invocation shared by 3
start(s) at once, between 4096 and 6144 MiB each; ...)`). `--mem 8G` sets the limit of each start, `--label NAME` names the
scope (`NAME-<variant>` for a run; default `scene-<kind>-...-<pid>`), `--who NAME` says who started it, `--no-cap` starts the
engine with no cap and says so at the start; `--wrap` words that already hold `gamerun` keep their own cap. The tool
refuses to start when `~/.local/bin/gamerun` is missing and `--no-cap` is not given.

The machine here has 46 GiB of memory (about 38 GiB available with the desktop up) and a 24 GiB card; at most three games run
at once by the lane rule, so three starts under the default hold at most 12 GiB, which leaves the rest (about 34 GiB) to
the desktop, and a second invocation under the shared lock another 12 GiB at most. The old harness's memory guard (host,
VRAM and GTT checks every 100 ms, halt file) is not ported: the scope limit is the cap and the VRAM and GTT checks are not
there.

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
  take the player to the new cells, which is what #474 reports (the land of the target never loads). Measured with
  the per-call debug line (`scene: setpos AXIS ...`, `NV_LOG_TERM=scene=debug`): after each of the three calls the player's
  position is exactly the target's coordinate in turn (53811, then 38382, then 3215) and its parent cell is still the
  start cell; the first traced frame after the third call shows the player at -72152 643 8291, about 2,000 units from the
  start in x and y, and it then falls to 8268 and stays. So the position is taken back within one frame, before any
  cell is asked for: something that holds the old position (the character controller, `005c0860` decides whether
  `SetPosBase` moves it) overwrites the reference's. Not traced further. So the soak
  fails while the defect is there and is the test for its fix. The task queue path (setting above 1) is not
  exercised.

## A placement that does not stick (nv-rs#486): the border rule

`scenes/soak-rivet-ne.toml` is the case: on a TTW load order the second place, 12000 east and 9000 north of the Rivet City
marker, was not reached. With `NV_SCENE_TRACE=1` the player was at the target in the first main frame after the placement
(89583 -60073 10873; the land is there) and in the second at the spot it stood at before (84583 -69073 12578), where it
stayed (the 150 s limit that first looked like a hang counted the placement call, which holds the game's own loading
screen, 64 s here). The same on every route: the exterior placement, `move = "request"` (the queued
`RequestPositionPlayer`, the route fast travel and `moveto` take) and with the character controller's height set after.

Cause: `PlayerCharacter` has a border rule. `LastKnownGoodPosition` (+0x7a0, three floats), `bReturnToLastKnownGoodPosition`
(+0x79d), `bInBorderContainedCell` (+0x79c) and `pBorderRegions` (+0x7b0): outside the world's border regions (the playable
area) the game puts the player back at the last good position. `NV_SCENE_TRACE=1` now scans the player, its process and its
character controller for words that hold the position the player left (`scan N: player+0x7a0 holds the old position`) and
prints `lkg x y z return N` every frame: the one holder of the old position was `player+0x7a0`. A place outside the border,
here across the Potomac from the marker (the other places and the working ones are inside), is undone one frame later.
The runner now makes the position the player was placed at the last good one and clears the return flag (in the placement and
every frame of a frozen world, `accept_position_as_good`): both places of the case are reached and soaked, exit status 0
(`rivet-north-east` 5.7 ms a frame). It is not an engine defect: the game's rule is what it is, and a scene run turns it off.
(The case places a player where a player cannot go in the game.)

## The far `setpos` (nv-rs#474) is a different thing

`move = "setpos"` is not the same defect: the return flag stays 0 and the border rule does not fire. The position set by
`SetPosBase` is undone within the same frame (the player at -72152 643, 2,000 units from the start, in the old cell, while
`LastKnownGoodPosition` shows the target for that one frame and then follows the player), by something in the game's own
frame after the runner's hook, which keeps the old cell and the character controller. `PositionPlayerExterior` (what `run`
uses; what `moveto` and fast travel end in, through `RequestPositionPlayer`) is the game's route for a move to another
cell: it asks for the target's cells, and it works for every place of the far-moves soak (ten places, Lake Mead among them).
The console's `setpos` only moves the reference inside the cell it is in. So for a place file, `setpos` is the wrong tool and
the default placement is the right one; `setpos` stays only as the test for nv-rs#474.

## The owner's list against what exists

| asked for | command | missing |
| --- | --- | --- |
| engine benchmark in Rust | `nv-rs-scene bench --suite quick` (real clock, exclusive game lock, warm-up and median of passes, `summary.json`, suite table with the change against the previous suite and a baseline, `--save-baseline`), `bench-compare A.json B.json`, a walking place (`path`) | see "The old Python harness is gone" below; the Contract v1 file (nv-rs#231) is not published yet, so the summary follows the old harness's keys plus `schema_version`; one machine's numbers only |
| spawning in specific locations | `NV_SCENE` place file: world, position, heading, pitch, or a reference with an offset | interiors (no cell by name), places by a worldspace that is not in the load order |
| loading into places | the runner's placement through the game's own exterior loader; one start for any number of places | no menu or intro is shown; the start is 6 s of data load (12 s on TTW) |
| no main menu needed | done: the runner sends the player from the menu in frame 121 | the start screens (intro videos) are not skipped, only passed |
| stability tests | `nv-rs-scene soak` (places, N frames each, counts stops, errors, warnings, exits non-zero) | no long-run memory growth check; #455 and #474 cases are place files (#474 fails until fixed) |
| pictures of areas, a frozen frame | `nv-rs-scene run` (variants, same place file, per-view difference) | cloud/brush phase differs by one frame between rare runs (nv-rs#484); `compare --noise` is the workaround |
| all in Rust, no shell/Python | `fork-scene` and `fork-scene-run` crates, the `nv-rs-scene` tool, `nv-rs-dev scene <build\|run\|soak\|bench\|compare\|bench-compare>` | done: the Python harness is deleted (see "The old Python harness is gone") |

## The old Python harness (`scripts/fork/bench/`) is gone

For nv-rs#328 and #348. Every piece is in the Rust tool or retired:

| old piece | here |
| --- | --- |
| bench lock (`lock.py`) | `--game-lock FILE` (shared) and `bench --lock FILE` (exclusive), the file `~/Games/"Fallout New Vegas RS"/game.lock` |
| suites, summary, baselines, compare (`suite.py`, `report.py`) | `bench --suite`, `summary.json`, `--save-baseline`, `--baseline`, `bench-compare` |
| host memory cap (the guard's systemd scope) | `gamerun` scope, per start (`--mem`, `--no-cap`) |
| start check (VRAM and GTT, the engine's budget, the halt file) | `crates/fork-scene/src/guard.rs` (`start_check`) |
| the 100 ms sampler, the session check, writing the halt file | `guard.rs`: `Watch` (read the card every 100 ms inside every engine start, stop at 90% of the way to the cap; once a second the kernel log and the desktop's compositor and the engine's own budget refusal), `halt`; `nv-rs-scene guard status \| clear-halt` |
| `nv-rs-dev bench` | forwards to `scene bench`; `bench clear-halt` is `guard clear-halt`, `bench compare` is `bench-compare` |
| `nv-rs-dev matrix`, `compare`, `clean` (`matrix.py`) | `nv-rs-scene matrix`, `clean` (`crates/fork-scene/src/matrix.rs`); remote hosts, the old harness's configuration axes and the HTML page are not carried over |
| `nv-rs-dev replay` (`d3d9replay.py`) | `nv-rs-scene replay` (`crates/fork-scene/src/replay.rs`) |
| `nv-rs-dev smoke`'s checklist (`smoke_report.py`) | `nv-rs-scene smoke-report` (`crates/fork-scene/src/smoke.rs`) |
| nested window session (`nested.py`), input driver (`vinput.py`, `driver.py`, `drivers.py`), views (`views.py`, `menu_view.py`) | not needed: the offscreen run has no window and no input; the runner places the player itself |
| noisy-run scan | `noisy` in `summary.json` (another `nv-viewer` running at the start or end) |
| scenarios that play the game with real input: new game, character creation, Doc Mitchell's house from the first start, combat, save and continue, the Ctrl (sneak) check, the 20-minute walk (`scenarios.py`) | gone, not ported: the tool tests places, not the opening. Release test steps 3 and 5 are done by hand (RELEASE-TEST.md) |
| log parsing of old run folders (`parse.py`, `timeline.py`, `report.py`, `watch.py`, `record.py`, `replay.py`) | gone; `nv-rs-dev hitches`, `runs` and `logs` read the engine's own logs |

The differential test of nv-rs#328 ("on the same recorded run files the Rust and Python versions give equal `summary.json`") cannot
be done as written: the Rust bench reads the engine's own log, not recorded run files. The Python self-tests were run once
before the deletion: `python3 -I -m unittest discover -s scripts/fork/tests` 410 tests OK (the 212 of the eight deleted
modules among them), `test_wif.py`, `check_data_gates.py`, `public_text.py` (and `--selftest`), `setup-anthropic-wif.py --selftest`
and `create-app.py --selftest` exit 0; after the deletion 198 tests OK.

Python and shell that stay on the game path, and why: `scripts/fork/local/nv-rs-dev` and `lib.sh` (the dev loop: build, `run` and
`smoke` start the viewer in a window on the real display for a person, `test`, `check`, `ci-lite`; they only set the environment and
call cargo or the tool); `scripts/fork/hitches.py`, `runs.py`, `logs.py` (read the engine's logs after a run, no game is started);
`distribution/unix/play.sh`, `packaging/*` (the shipped launcher and installers).

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

`bench --suite NAME` takes a built-in place file instead of `--places`: `quick` (Goodsprings, Novac and the Strip, then Doc Mitchell's house and the Tops casino floor,
the old quick suite; `scenes/bench-quick.toml`), `mojave` (`mojave-far.toml`), `soak` (the far
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
