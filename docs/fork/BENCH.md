# Scripted playthrough and benchmark runs

`nv-rs-dev bench` runs the viewer on the maintainer's game folder with scripted input and records frame times, memory and
stability numbers, so the engine can be exercised far more than by hand. It uses what the engine already has: the
scripted keys and mouse of `crates/host/src/player_move.rs` (`NV_MOVE_KEYS`, `NV_MOVE_MOUSE`, by main-loop frame), the
per-frame log `NV_FRAME_LOG` (clock, player position, screen), the game's own console, and the viewer's log counters. No
engine change is needed. The scripts are `scripts/fork/bench/` (Python, standard library only). Real scenarios need the
game folder and run only locally; CI runs the parsing, report and compare code against synthetic fixtures
(`scripts/fork/tests/test_bench.py`, part of `task scripts`).

## Commands

| Command | What it does |
| --- | --- |
| `nv-rs-dev bench list` | The scenarios |
| `nv-rs-dev bench run <scenario> [--cap N] [--mode unlimited\|capped\|peak] [options]` | Runs one and writes the run directory and report. Unlimited by default |
| `nv-rs-dev bench suite [--scenarios a,b] [--cap N] [--save-baseline NAME] [--baseline NAME]` | The scenarios back to back (the lock is held one scenario at a time), one headline table with the change against the previous suite and a baseline |
| `nv-rs-dev bench report <run>` | Prints a run's report (`<run>` is a directory, a summary.json, a name, or `latest[:scenario]`) |
| `nv-rs-dev bench compare <baseline> <candidate> [--threshold key=pct]` | Regression check; exit status 1 on a regression |
| `nv-rs-dev bench analyze <viewer.log> [--frames F] [--samples S]` | Parses an existing log |

Options of `run`: `--nested headless|window` (the real window path, see "Real window path" below), `--windowed --yes-windowed` (the viewer's own window on your compositor; refused without both), `--threads real|stepped`,
`--preset fixes|vanilla`, `--resolution WxH`, `--sound silent|out|none`, `--vram-limit-pct N`, `--mem 12G`, `--scale F`
(shorten or lengthen a scenario), `--keys` and `--mouse` (extra scripted input, `NV_MOVE_KEYS` syntax), `--shots DIR[,ms]`
(debugging: the game's frames saved for you to look at, never commit them). The `views` scenario has its own: `--views`, `--views-file`, `--hour`, `--weather`, `--views-settle` (see "The views scenario").

Runs go to `~/.local/state/nv-rs/bench/<scenario>-<time>/` (`NV_BENCH_DIR` changes it): `viewer.log`, `frames.tsv`,
`samples.tsv`, `summary.json`, `report.md`, `report.html` (inline SVG charts, no script). Numbers only: no game data.

## Across refs, configurations and machines

`nv-rs-dev matrix` and `nv-rs-dev compare` drive this runner for several refs (commit, branch, tag, PR), several
configurations (session, mode, resolution, preset, threads, sound) and several hosts, and compare each ref with a baseline
using the thresholds below; see [LOCAL.md](LOCAL.md#matrix-runs). `--json` gives CI a machine-readable summary.

## What is recorded

- Frame times from `NV_FRAME_LOG`: average, p50, p95, p99, max, fps, 1% and 0.1% lows, overall and for the play screen; an
  fps timeline in 1 s buckets. Loading-screen frames (the main loop spins at about 1 ms a frame there) are left out of the
  headline numbers and reported apart.
- A one-second sampler: process RSS, CPU, and the card's VRAM and GTT (amdgpu sysfs; every process on the card counts, so
  close other GPU work for clean numbers). The growth of each over the second half of the run is reported in MiB a minute.
  `NV_MEMLOG=1` is set too: once the build has it (#70) its lines are parsed (heap use) as well.
- From the viewer log: stop lines, wgpu errors, skipped draws, draws, pipelines, presents over 20 ms, panics, memory
  failures; from `--movement-missing`: the untranslated addresses and their hits.
- Hitches next to the game trace's menu and load events (`menu.stack`, `menu.loading`, `load.kind`, `load.thread`, `load.movie`): each hitch of the recorder's `hitches.csv` is counted under the nearest such event within three frames of the viewer log (`metrics.hitch_events`, one line in the report's Hitches section), else under none.
- A verdict: the stability checks plus the scenario's own (screens reached, player placed, minutes of play).

## One run at a time (the bench lock)

Frame times are only valid when nothing else loads the CPU and the card, so `nv-rs-dev bench run` takes a machine-wide
exclusive lock (a flock on `~/.cache/nv-rs/bench.lock`, `NV_BENCH_LOCK` changes it) for the whole run, the nested
compositor session included, and releases it when the run ends however it ends. Waiters take it in the order they came (a ticket file each in `bench.lock.queue/`; a waiter that died loses its place), so
no run is starved by newer ones. A second run prints
`waiting for bench lock held by pid N (bench run X), S s ago` every 30 s and waits; `--wait-timeout SECONDS` gives up
with exit status 3. `--no-lock` skips it for a debugging run that gives no numbers. `nv-rs-dev matrix` and `compare` run
their benchmarks through `bench run`, so they take the same lock one cell at a time.

**Every worker and every person measures through `nv-rs-dev bench run`; nobody launches `nv-viewer` directly for numbers.**
The run also scans /proc for other `nv-viewer` processes every 5 s; if any ran, `summary.json` has `run.noisy: true`
and `run.other_viewers_max`, the report says NOISY, and `bench compare` says which side was noisy: such timing rows
are not to be trusted (a run by hand on the side, a session of a local wrapper, a worker that skipped the lock).

## The memory guard (every run)

A bench run once started while another program held almost all of the card's memory; the driver then refused the
desktop's own command submissions (`Not enough memory for command submission`) and the session crashed. Every
`nv-rs-dev bench run` and `bench suite`, and the replay-bench's `run`, therefore goes through `scripts/fork/bench/guard.py`.
No option switches it off.

| What | How |
| --- | --- |
| Host memory | The viewer and the nested compositor run in transient systemd scopes with `MemoryMax`, `MemoryHigh` (90% of it) and `MemorySwapMax=0`. Together they get half the memory available at start, 12 GiB at most; the compositor's scope is a fifth of that (2 GiB at most) and the viewer's the rest. Without `systemd-run` there is no run |
| GPU memory, before | The card's VRAM and GTT use is read from amdgpu sysfs. The run is refused when the free VRAM cannot hold its cap (half the card's VRAM) plus 2048 MiB kept for the desktop, or the free GTT its cap (a quarter of the GTT) plus 1024 MiB |
| GPU memory, during | Every 100 ms. At 90% of the way to the lower of the run's cap (over the use at start) and the card's total less the desktop's margin, the viewer's whole process group gets TERM, then KILL two seconds later. `--vram-limit-pct` (70 at most) stays as a second, card-wide limit, checked every second |
| Engine | `NV_GPU_BUDGET_MB` is set to the run's VRAM cap (a lower value in the environment stays). The Direct3D layer refuses allocations past it with a `stop:` line, and the guard ends the run at that line |
| Unreadable card | A card whose memory can no longer be read during the run stops it: an unguarded run does not go on. A card with no amdgpu sysfs memory files is refused at the start |
| Session health | Once a second: an amdgpu line in the kernel log with `Not enough memory for command submission`, a GPU reset, a ring timeout or a lost device, or a desktop compositor (`gnome-shell`, `kwin_wayland`, `mutter`) that ran at the start and is gone |
| Halt | When the guard stops a run, or the session is in trouble, it writes `~/.cache/nv-rs/bench.halt`. Every later run is refused (exit status 4), and a suite stops at once and starts no further scenario. `nv-rs-dev bench clear-halt` removes it, after the cause is understood |

The caps are the defaults and can only be lowered: `--mem`, `--vram-cap-mib`, `--gtt-cap-mib` and `--vram-limit-pct`
refuse a higher value. Every run logs its caps and the card's use at start (`# memory caps:` in `viewer.log`), writes
`caps.json`, and keeps the caps, the card's use at start, its peak during the run and any guard stop in
`summary.json` (`run.caps`, `run.card_at_start_mib`, `run.card_peak_mib`, `run.guard`). The verdict has a
`memory guard quiet` check. When `journalctl -k` cannot be read, the log says so (`# memory guard warning:`) and only the
card's use and the desktop's compositor are watched.

The card's use is the whole card's, every program on it: another program that fills the card during a run also stops
the run and halts the bench, on purpose. The desktop is what is protected.

## Modes and safety

- **Unlimited is the default, as real play is.** `--mode unlimited`: `NV_FRAME_LIMIT=0` and `NV_FPS_LIMIT=0` in play and
  in loading, `NV_PRESENT=mailbox`, no vsync pacing, so fps is what the game and the card can do. Only the menus keep
  `NV_MENU_FRAME_LIMIT=120` (the menus run on time and drop keys at thousands of frames a second). `--cap N` (or
  `--mode capped`) is the one way to a limit, for pacing tests: `NV_FRAME_LIMIT=N`. `--mode peak` presents `Immediate`.
  A run whose fps is not a headline says so: the report and the suite table show **CAPPED** (verdict) when a limiter
  variable is set, when `--cap` was asked, or when the measured play fps sits within 2% of a usual cap value (24, 30, 48,
  50, 60, 72, 75, 90, 100, 120, 144, 165, 240: something else limits it, such as a display, a compositor or the driver).
  Compare such a run only with capped ones.
- Offscreen (`NV_D3D9_OFFSCREEN=1`) by default: no window, no compositor involved. Sound runs on the game's clock into
  the null sink (`NV_SOUND=out`, `NV_SOUND_BACKEND=null`), so the speakers get nothing. The game's voice lines and movies
  need that clock: with `NV_D3D9_HIDDEN=1` (which mutes the device, and was the bench's way until 2026-11-16) Doc
  Mitchell's opening lines never ended and the opening stalled at his second line, which was taken for an engine fault.
  With the null sink Doc's dialogue runs on its own to the name box and the face menu (mirror) draws its head.
- The watchdog samples VRAM and GTT every second and stops the viewer (SIGTERM, then SIGKILL) at `--vram-limit-pct`
  (default 70, or `NV_VRAM_LIMIT_PCT`). The viewer also runs in a memory-capped systemd scope (`--mem`, default 12G), with
  no CPU quota or nice level, which would distort the numbers.
- Thresholds of `compare` (percent worse, with a noise floor): fps 5, 1% low 10, p99 10, max 50, RSS 10, VRAM 10, GTT 10,
  skipped draws 20; wgpu errors, stops and untranslated addresses regress on any increase. `--threshold fps=3` changes one.

## Real window path (`--nested`)

`--nested headless` starts sway on wlroots' headless backend (own Wayland socket, one output of `--resolution`, no
Xwayland, memory-capped scope) and runs the game in it as a normal borderless window: a real Wayland surface, a real
wgpu swap chain with Mailbox present, real window focus and pointer lock, 2560x1440 if you ask for it. Your session
(GNOME) is never involved, nothing appears on your screen, no focus is taken, and the watchdog (`--vram-limit-pct`,
use 50 for window runs) and the memory scope stay on. Needs `sway` (`nix shell nixpkgs#sway`, or `NV_BENCH_SWAY=path`).
The runner also borrows the Nix store's Wayland, xkbcommon, Vulkan and pulse-simple libraries as a local wrapper does;
without them a windowed run fell back to off screen silently (the log line "the real window could not be made").

`--nested window` shows the nested compositor as one window in your session (`--yes-windowed`; the compositor is a
client of yours, and a covered or minimised window stops frame callbacks, so fps and pacing numbers are not valid
there). `--windowed --yes-windowed` is the viewer's own window on your compositor (`NV_D3D9_BACKGROUND=1`: behind the
others, not activated). Which gives valid numbers: only `--nested headless` (the nested output always has a frame
callback) for fps and pacing; offscreen for work and stability without a swap chain; the other two only for stability, with
the card's memory watchdog at 50%.

Measured 2026-11-16 (AMD RDNA 3 (RADV), `newgame` and `chargen`, capped, 2560x1440, build of the day), offscreen against nested:

| | offscreen | nested headless |
| --- | --- | --- |
| play-screen fps, newgame | 119 | 119 |
| VRAM peak, chargen (MiB) | 1618 | 1837 |
| GTT peak, chargen (MiB) | 201 | 179 |
| wgpu errors, skipped draws | 0, 0 | 0, 0 |
| swap chain configured | none (no swap chain) | 2 (0 refused) |
| screens reached, face menu drawn | yes | yes |

VRAM and GTT were flat after the load in every run (a few MiB a minute); nothing grew without bound, so the GPU
out-of-memory that once took gdm down did not reproduce in either path with today's build. A windowed run by the maintainer
of 2026-11-16 07:34 on GNOME (`~/.local/state/nv-rs/logs/run-*.log.gpumem`) also stayed under 2.2 GiB VRAM and 340 MiB GTT
with 0 wgpu errors; there the swap chain was configured 24 times in three minutes (the nested one twice): the compositor
reconfigures it (monitor 1920x1080 at 144 Hz, window 2560x1440). What to capture if it happens again: the run's `.gpumem`
file, `NV_LOG=game=debug`, the "swap chain configured N times" line, and `dmesg` for `amdgpu` lines.

## Watching, recording and replaying a run

Needs the tools of a nested run plus `grim` (web view), `wayvnc` (VNC view), `wf-recorder` (recording), `ffmpeg` and `mpv` (optional):
`nix shell nixpkgs#sway nixpkgs#grim nixpkgs#wayvnc nixpkgs#wf-recorder nixpkgs#ffmpeg nixpkgs#mpv`. A missing tool stops only
its own feature, with that hint. Every run writes `session.json` (the nested socket, the `--shots` folder), `steps.json`
(the scripted input by main frame) and, through the viewer's hitch recorder, `hitches.csv` into its run directory.

Live view, from a second terminal while the run goes (read-only: it attaches to the nested compositor's own socket, never
to your session, takes no focus and no input):

    nv-rs-dev bench watch                 # newest session; opens http://localhost:PORT/ (MJPEG, 5 fps at half size)
    nv-rs-dev bench watch PID|RUN --mode vnc   # wayvnc on localhost:5900, input disabled; any VNC client
    nv-rs-dev bench watch --bind 0.0.0.0  # for another device (shows the game screen to your network: your call)

An offscreen run (no nested compositor) shows the newest file of its `--shots` folder instead. There is no tap in the viewer's
render path, and none is planned: a read-back there would be the cost the numbers exclude. A run that had a watcher or a
recorder attached carries `watched: true` in `summary.json` settings (and `recorded: true`): treat its numbers as not baselines.

Recording: `bench run <scenario> --nested headless --record [--record-fps 30|60] [--record-format mp4|webm]`, or `bench record
[latest|RUN|PID]` for a session already running. wf-recorder encodes with VAAPI (`h264_vaapi`, the card's encoder, so the CPU
stays free) when `/dev/dri/renderD128` accepts it, else x264 ultrafast; `NV_BENCH_RECORD_CODEC=x264` forces the latter. The
file is capped at 2 GiB (`bench record --cap-mb`; the recording stops, valid, and `record.json` says `truncated`). `screen.mp4`,
`record.json` and `record.log` stay in the run directory and are never committed: they show game content.

Replay: `nv-rs-dev bench replay [latest|RUN] [--burn] [--play]` writes `timeline.json`, `timeline.tsv` (kind, t_s, frame, value,
label: frame times, hitches, screens, steps on the video's clock), `timeline.ass` and `replay.html` into the run directory and
prints the commands. `replay.html` is one local file (no external scripts, works offline): the video, a frame-time graph with a
moving cursor, a hitch list and prev/next-hitch buttons, step and screen lists; click the graph or a list entry to seek.
`xdg-open RUN/replay.html`, or `mpv --sub-file=RUN/timeline.ass RUN/screen.mp4` (hitches flash red in the corner, the status line
names the screen and step); `--burn` writes `screen.timeline.mp4` with the subtitles burned in. The video clock is the engine
clock from the first frame plus the recording's lead-in; the hitch recorder's wall stamps pin that to a few milliseconds.
Step names are the scripted input's key codes and counts (the scenarios carry no names yet).

## Scenarios

The world scenarios are driven by **wall-clock, closed-loop real input** (`driver.py`, `drivers.py`): a Wayland virtual
keyboard and pointer (`vinput.py`, standard library, the Wayland wire protocol; sway's own, never the session's) send real
key and pointer events into the nested window, timed in seconds and by the game's own screen (the frame log's `screen`
column), so a scenario is valid at any frame rate. The virtual keyboard carries a real xkb keymap, so the game gets true
scancodes (`wtype` gives each key a private keycode and the game reads it as the wrong key), and it exists before the game
starts (a seat without a keyboard when the game binds it is never given one). The run needs `--nested headless`, which
is the default for these scenarios (`--legacy` runs the old frame-counted `NV_MOVE_KEYS` scripts offscreen at a cap).

| Scenario | What it does |
| --- | --- |
| `quick` | **The quick suite, one viewer session, 120 s in all (`TOO_LONG` over that, start-up and loading included):** start-up (shorter fixed waits), the new game's first room, then Doc's house, Goodsprings and the Strip in turn by console teleports, each place's measured script at half length (about 10 s) |
| `doc_house`, `goodsprings`, `strip`, `novac` | Each the same for one place (start-up, new game, then the place): ~3 s warm-up, ~18 s measured |
| `sneak` | Goodsprings, then the sneak check: a step forward standing, real Ctrl, the same step sneaking; fails when sneaking is not slower (see "Ctrl and sneak") |
| `views` | **Fixed views of the world, one picture each (nv-rs#343; "The views scenario" below):** a new game, Goodsprings, the game hour set and stopped, a weather forced, then each view of a list placed by the console, settled, and captured to `views/<name>.png` with a manifest `views.json` |
| `save`, `continue` | The save round trip of the release test ([RELEASE-TEST.md](RELEASE-TEST.md)): a new game saved in the first town with the console, then a new session that loads it with Continue; each notes the player's position |
| `newgame` | Start-up movies skipped, menu, New, the intro movie skipped, the pack boxes, the first room, 20 s there |
| `boot` | The start-up movies skipped, 40 s on the main menu |
| `combat` | Goodsprings, three hostile geckos by form id, a pistol and ammo added and equipped, clicks and turns |
| `soak` | Novac, the Strip, Goodsprings and Doc's house in turn, the measured script eight times in each, repeated (`--scale`) |
| `chargen`, `probe`, `realwalk` | Frame-script tools (`--legacy`): the name and sex by console stage commands to the face menu; free console commands; real input after a teleport |

The measured script of a place (about 18 s, the same everywhere): a yaw sweep right and back (pointer counts, the game's
sensitivity decides the angle), pitch up, down through level and level again, forward 3 s, strafe left and right 1 s each,
forward 3 s turning. Goodneighbor is not in this load order (base game and the DLC: no such
worldspace or cell in FalloutNV.esm or the DLC masters), so there is no `goodneighbor` scenario.

Verdicts: the stability checks; `reached the loading and play screens`; `player moved 150 units a place`; `the camera
turned (<place>)` (the walk's heading changed by 15 degrees or more between the first forward leg and the last seconds of
the turning one, read from the player's position: the view turned in between; it fails when either leg moved under 30
units, so a place where the player did not walk fails it); `game time keeps wall time` (game seconds
over wall seconds 0.85 to 1.15, which fails if the game depends on the frame rate: 500+ fps must not change what the game
does); `the player landed` (height settled); `within the wall-time budget (TOO_LONG)`; and **CAPPED** when the fps is not
a headline (above). Each measured stretch is a phase in the log (`# driver: phase NAME start|end frame=N t=S`), and a
`quick` run reports the fps, p99 and 1% low of each place and the seconds of each phase.

How a place is reached without a save (form ids of the base game, numbers only; found by scanning FalloutNV.esm, the ids
are not game data): the opening quest `104C1C` has the player seated in an animated camera with movement and looking off
(its stage 0 runs `DisablePlayerControls 1 1 1 1 0 0 1` and a script package). Its stage 50 removes the package and stage 55
gives movement and looking back (`EnablePlayerControls 1 0 0 1 1 1 0`), so the driver sets `setstage 104c1c 50`, then 55,
and the player walks; jumping straight to the last stage left the player unable to move for the whole earlier work (path 0
units). Then `player.moveto <marker>`: `1658B7` Goodsprings, `16ACB9` the Strip, `8D0A3` Novac, `107235` Doc's house; the
downloadable packs' message boxes arrive after the opening's stages and with the first exterior, a few seconds after the
load, at no fixed time. The driver never assumes the console's state: it opens the console from the play screen and sees
the screen turn to menu, shuts it as soon as a teleport's load is over (a box that comes over the open console never
gets the keyboard), and dismisses boxes with Enter, Down, Enter in turn (a box's button takes Enter only once an arrow
key has given it the focus; not in the start-up, where Down in the pause menu moves to Save). A menu that outlasts 6 s
gets the console key, again every 6 s. The warm-up of a place is the play screen held 3 s with no box, so a late box is
dismissed before the measured script starts rather than measured as a menu. `168D0B` is the hostile gecko, `E3778` the pistol, `8ED03` the ammo.

Why `NV_MOVE_KEYS` real input needed `NV_MOVE_REAL_INPUT=1`: a run with a key or mouse script shuts the window's keyboard
and mouse out (`real_input_reaches_game`, `crates/host/src/player_move.rs`), so real events never arrived until the
drivers stopped using scripts altogether.

### The views scenario

`nv-rs-dev bench run views` shows the same places, at the same headings and the same time of day, every run, with one
picture per view: for comparing a run with a generated LOD folder against one without it, and for judging distant land and
water from the pictures. It is a driver like the others (nested headless window, the memory guard, the bench lock).

After the new game and a teleport to Goodsprings (which is how the driver gets into the Mojave exterior), the driver sets the
sky once, by the console: `set gamehour to H`, `set timescale to 0` (the hour then stays for the whole run) and, unless
asked not to, `fw <weather>` (a forced weather; the default is the base game's clear day, `0FFC88`). Then, for each view in
order, it opens the console and sets `player.setpos x/y/z` and `player.setangle z <heading>`, waits for the player to be there
and the load to end, holds the play screen `--views-settle` seconds (default 15; boxes dismissed), waits for the frame times
to even out (60 frames none over 100 ms and the player landed), and takes the picture of the nested screen (`grim`, found as
`NV_BENCH_GRIM` or on `PATH`; without it no picture is taken and the verdict fails). Its position is noted in the log as
`# driver: state view.<name> x=.. y=.. z=..` after the picture is written.

| Option | Meaning |
| --- | --- |
| `--views 'name:x:y:z:heading;...'` | The views on the command line (`;` between views). Default: the built-in list below |
| `--views-file FILE` | The views in a file, one a line `name x y z heading`, `#` comments and blank lines ignored. Not together with `--views` |
| `--hour H` | The game hour (0 up to 24, default 12) |
| `--weather FORMID\|none` | The weather form id in hex to force (default `0FFC88`); `none` leaves the game's own weather, and the manifest then says it is not fixed |
| `--views-settle S` | Seconds of play at a view before its picture (default 15) |

A view's name is its file name (letters, digits, `-` and `_`), so two runs compare file by file whatever else is on the list;
the position is in world units of the Mojave Wasteland, the heading in degrees of the game's compass (0 is north, +y; 90 is
east, +x). The heading is commanded, not read back (the frame log has no heading column), so it is exact; the position
is the one the frame log showed after the player landed. The built-in views (map markers of the base game, looking far over
land and water):

| Name | From | Heading |
| --- | --- | --- |
| `goodsprings-valley` | the east edge of Goodsprings | 90 |
| `vegas-from-the-south` | NCR Sharecropper Farms | 0 |
| `lake-mead` | Boulder Beach | 60 |
| `colorado-river` | Cottonwood Cove | 90 |
| `hoover-dam-from-the-west` | west of Hoover Dam | 75 |

The run directory gets `views/<name>.png` and `views.json`: the hour, the clock (stopped), the weather (or that it is not
set), the settle time and, per view, the commanded and the observed position, the heading and the picture's name. The
manifest is rewritten after every view. `nv-rs-dev bench views-compare RUN_A RUN_B` checks two runs against the issue's
acceptance: the same views and sky, positions within 1 unit and headings within 1 degree (exit status 1 and one line per
difference otherwise). The pictures are game content: they stay on the machine, never in a repository.

### Fixed: the player stopped answering movement and the mouse (nv-rs#324)

Until 2026-11-16 the player stood still in most quick runs a few seconds after the opening's stages were set, while
the console kept taking typed text. The cause was not in the input path. The opening runs on its own clock: its quest
script steps the early stages on a timer, and the end of the opening's first spoken line sets stage 5, which puts the
sitting script package back on the player and starts the timer to stage 7, which adds it again. When that line ended
after the bench had set stages 50 and 55, which depended on how long the start-up took, the package held the player
again: movement and looking enabled, the player not restrained, the keys reaching the game, and nothing moving.
`player.removescriptpackage` freed it; `enableplayercontrols`, `setrestrained 0` and the like did not.

What the driver does now (`drivers.py`):

- `free_player` also runs `set VCG01.bRunTimer to 0`, which stops the opening's timer chain.
- `ensure_walks`, after each place's warm-up, takes a 0.4 s step forward, else back. If neither moves the player, it
  runs `player.removescriptpackage` and tries again. In Doc's house it fired in each of the five runs below.
- The frame-log tail drops a line the viewer has not finished writing. Cut after the position, its screen read as an
  empty string, "not play", and the driver tapped Enter and Down into play: the stray keys of the failing runs.
- The first exterior's warm-up waits for one more of the packs' boxes, which comes about 5 s after the load (4.9 to
  5.0 s in four runs), after a 3 s warm-up was already over. A box that still comes during the sweeps of the measured
  script is dismissed before each walk, so the walk walks.
- The runner no longer waits out the viewer's 5 s grace at the end of every run: the viewer, ended by SIGTERM, stayed a
  zombie until waited for, and the check took that for a live process.

Two things tried and dropped: a 1.5 s first-room settle instead of 3 s (in both runs with it the player never walked,
even after the package was taken off; the cause was not looked into) and Down before each Enter on the boxes (a box
then stayed for minutes). The start-up keeps its 3 s.

### Results: the quick suite, 2026-11-16

`nv-rs-dev bench suite --quick --resolution 2560x1440 --vram-limit-pct 50 --nested headless` (the suite turns on
`NV_PERF`), `play-opt` build of `fork` of the day, unlimited (no frame limiter; menus 120), real threads, AMD RDNA 3 (RADV),
16-core machine with no other viewer running (no run flagged noisy). Five runs in a row, all `PASS`: the player walked
and the view turned at every place in each (paths 3699 to 3805 units; heading changes 33 to 69 degrees in Doc's house,
68 to 80 in Goodsprings, 76 to 92 on the Strip). Total wall time, start-up and loading included: 113.0, 112.5, 106.0,
113.0 and 113.5 s (budget 120 s). The table gives the median of the five (each run's own numbers in brackets). fps,
p99 and 1% low are of the play-screen frames of each place's measured script (about 10 s: the yaw and pitch sweeps,
the walks); "busy" is the busiest thread's CPU share over the `NV_PERF` reports logged during that script, as
milliseconds of a mean frame.

| Place | avg fps | p99 ms | 1% low fps | busiest thread ms/frame |
| --- | --- | --- | --- | --- |
| Doc Mitchell's house | 246 (246, 243, 248, 246, 244) | 5.9 (5.6, 6.1, 5.9, 5.7, 6.3) | 153 (156, 149, 153, 159, 141) | 3.8 (3.6, 3.6, 3.8, 4.0, 3.8) |
| Goodsprings | 157 (158, 155, 177, 157, 155) | 8.2 (8.2, 8.2, 7.8, 8.1, 8.2) | 107 (107, 104, 109, 112, 105) | 4.5 (4.5, 4.6, 4.5, 4.5, 3.7) |
| The Strip | 167 (169, 168, 167, 166, 167) | 9.9 (9.9, 9.9, 9.7, 9.9, 9.9) | 94 (95, 90, 94, 94, 94) | 5.9 (5.8, 5.9, 5.9, 5.9, 5.9) |
| Whole session, play screen | 209 (212, 209, 208, 211, 207) | 8.7 (8.9, 8.7, 8.7, 8.6, 9.0) | 92 (92, 92, 91, 93, 92) | 3.3 (3.2, 3.3, 3.3, 3.3, 3.2) |

The earlier table of the day (three runs, two with a standing player) is replaced: its place numbers were mostly of a
view standing still. The run-to-run spread is now small (Strip fps 166 to 169, p99 9.7 to 9.9 ms). The baseline
`quick-20261116` (under `~/.local/state/nv-rs/bench/baselines/`, local) still holds the older first run.

### Ctrl and sneak

Real `Ctrl` taps and holds reach the game in the nested window (`input.key: LeftControl (0x1d) down reaches the game`,
then `up`, in the log), so the reported "Ctrl does nothing on GNOME" is not the game dropping the key once the
compositor delivers it to the window. The `sneak` scenario checks the rest: in Goodsprings, a 0.8 s step forward
standing and back, a real Ctrl tap, the same step sneaking and back, Ctrl again. Sneaking is slower, so the verdict
`Ctrl made the player sneak` passes when Ctrl reached the game between the two steps and the sneaking step got at most
0.8 times as far as the standing one (the farthest point from where the step began, read from the frame log).

Result, 2026-11-16: the standing step went 270 units, the sneaking one 153 (ratio 0.57): **pass**. The scenario first
failed, rightly: Ctrl reached the game and the player did not sneak (ratio 1.01, no sneak indicator on screen). The
opening's stage 0 turns sneaking off with the other controls and stage 55 gives back movement and looking only; the
opening turns sneaking back on later. So the scenario runs `enableplayercontrols` before the check, as a player who has
finished the opening has. A new game stopped before the end of the opening cannot sneak: that is the game's own rule.

### Notes on the old scripts

The frame-counted scripts (`--legacy`, `chargen`, `probe`) and their findings stand: a key tap shorter than about 20 frames
is dropped by the menus at the old cap, the first arrow press only wakes the menu's keyboard mode, `p`-prefixed frames
count only the frames that are not a loading screen. They are valid only capped (`--legacy` implies `--cap 144`).
