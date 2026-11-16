# Scripted playthrough and benchmark runs

`nv-rs-dev bench` runs the viewer on the owner's game folder with scripted input and records frame times, memory and
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
| `nv-rs-dev bench run <scenario> [--mode capped\|uncapped] [options]` | Runs one and writes the run directory and report |
| `nv-rs-dev bench report <run>` | Prints a run's report (`<run>` is a directory, a summary.json, a name, or `latest[:scenario]`) |
| `nv-rs-dev bench compare <baseline> <candidate> [--threshold key=pct]` | Regression check; exit status 1 on a regression |
| `nv-rs-dev bench analyze <viewer.log> [--frames F] [--samples S]` | Parses an existing log |

Options of `run`: `--nested headless|window` (the real window path, see "Real window path" below), `--windowed --yes-windowed` (the viewer's own window on your compositor; refused without both), `--threads real|stepped`,
`--preset fixes|vanilla`, `--resolution WxH`, `--sound silent|out|none`, `--vram-limit-pct N`, `--mem 12G`, `--scale F`
(shorten or lengthen a scenario), `--keys` and `--mouse` (extra scripted input, `NV_MOVE_KEYS` syntax), `--shots DIR[,ms]`
(debugging: the game's frames saved for you to look at, never commit them).

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
are not to be trusted (a run by hand on the side, an `nv-rs-run` session, a worker that skipped the lock).

## Modes and safety

- `capped` (default): 144 frames a second limit, mailbox present: pacing and 1% lows. `uncapped`: no limit,
  `NV_PRESENT=immediate`: peak fps (offscreen, the swap chain does not exist, so it measures the work).
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
The runner also borrows the Nix store's Wayland, xkbcommon, Vulkan and pulse-simple libraries as `nv-rs-run` does;
without them a windowed run fell back to off screen silently (the log line "the real window could not be made").

`--nested window` shows the nested compositor as one window in your session (`--yes-windowed`; the compositor is a
client of yours, and a covered or minimised window stops frame callbacks, so fps and pacing numbers are not valid
there). `--windowed --yes-windowed` is the viewer's own window on your compositor (`NV_D3D9_BACKGROUND=1`: behind the
others, not activated). Which gives valid numbers: only `--nested headless` (the nested output always has a frame
callback) for fps and pacing; offscreen for work and stability without a swap chain; the other two only for stability, with
the card's memory watchdog at 50%.

Measured 2026-11-16 (RX 7900 XTX, `newgame` and `chargen`, capped, 2560x1440, build of the day), offscreen against nested:

| | offscreen | nested headless |
| --- | --- | --- |
| play-screen fps, newgame | 119 | 119 |
| VRAM peak, chargen (MiB) | 1618 | 1837 |
| GTT peak, chargen (MiB) | 201 | 179 |
| wgpu errors, skipped draws | 0, 0 | 0, 0 |
| swap chain configured | none (no swap chain) | 2 (0 refused) |
| screens reached, face menu drawn | yes | yes |

VRAM and GTT were flat after the load in every run (a few MiB a minute); nothing grew without bound, so the GPU
out-of-memory that once took gdm down did not reproduce in either path with today's build. The owner's own windowed run
of 2026-11-16 07:34 on GNOME (`~/.local/state/nv-rs/logs/run-*.log.gpumem`) also stayed under 2.2 GiB VRAM and 340 MiB GTT
with 0 wgpu errors; there the swap chain was configured 24 times in three minutes (the nested one twice): the compositor
reconfigures it (monitor 1920x1080 at 144 Hz, window 2560x1440). What to capture if it happens again: the run's `.gpumem`
file, `NV_LOG=game=debug`, the "swap chain configured N times" line, and `dmesg` for `amdgpu` lines.

## Scenarios

| Scenario | What it does | State |
| --- | --- | --- |
| `boot` | Skips the start-up movies, sits on the main menu for 50 s | works |
| `newgame` | Menu, New, intro movie, the pack messages, to the first room (Doc Mitchell's house), 20 s there. Capped mode only | works (see below) |
| `chargen` | `newgame`, then the opening quest's name and sex choices (stages set by console), to the face menu. Capped mode only | works to the face menu (see below) |
| `probe` | Debugging: `PROBE="getstage 104c1c;key:enter;wait:2;mouse:10:5:1"` runs console commands and key, mouse and wait steps in the first room | a tool |

| `chargen` | `newgame` then the opening quest's name and sex by console stage commands, to the face menu | works |
| `goodsprings`, `novac`, `strip` | Without a save: finish the opening quest by its last stage, teleport by a placed reference's form id, dismiss the packs' message boxes, a fixed walk loop (`--scale` sets the laps) | teleport works, walking blocked (below) |
| `combat` | In Goodsprings: three hostile geckos placed by form id, pistol and ammo added and equipped, a click script | as above, firing not verified |
| `soak` | Goodsprings, Novac, the Strip, Freeside in turn, repeated (20 min at scale 1) | as above, later teleports untested |

Form ids used (base game, numbers only): quest `104C1C` (stage 200 is the end), markers `1658B7` Goodsprings, `8D0A3`
Novac, `16ACB9` the Strip, `13A669` Freeside, creature `168D0B`, pistol `E3778`, ammo `8ED03`. Found by scanning
FalloutNV.esm for editor ids; the ids are not game data.

What works (2026-11-16): `setstage 104c1c 200` frees the player and ends Doc's lines; `player.moveto 1658B7` loads the
Goodsprings exterior (it renders; the old `CreateQuery(9)` stop does not appear any more); the downloadable packs'
message boxes then come one after another (`menu` screen; Enter dismisses; the first one closes the console, so no
console key follows). Goodsprings exterior ran at about 105 fps (capped, 2560x1440) with 0 wgpu errors and VRAM 2.4 GiB.

What blocks: after the teleport the scripted `W` and the mouse do not move or turn the player (path 0 units, the same
view after minutes, offscreen and nested alike), so the "moved N units" verdicts fail honestly. `enableplayercontrols`,
`player.setrestrained 0` and `player.setav paralysis 0` did not change it. Unknown: whether the opening's quest script
holds the controls (finishing it by stage may skip what gives them back), or the scripted keys do not reach the
player's controls in play. A prepared save (another worker is on save writing) would answer which; a real-input run in
the nested window (`wtype`/`wlrctl` events into it) would answer the other. An open flake: about one run in ten stops
at frame 6001 (the intro movie skip) with `WaitForSingleObject ... semaphore count 0` in stepped threads.

Planned: the same scenarios once a save or the controls are solved; their route lengths and the fps numbers then stand.

### How the input is driven, and its limits

The script is by main-loop frame, open loop. Findings that shaped it (2026-11-15, offscreen, 144 Hz cap):

- A key tap shorter than about 20 frames is dropped by the menus; scenarios hold keys for 30 frames.
- The first arrow press after the menu appears only wakes its keyboard mode and moves nothing. The script presses twice.
- The intro movie plays on the loading screen at about 800 frames a second and is skipped with Esc; every frame count
  during a loading screen is wall-time dependent, which is why `newgame` is timed in frames and valid only capped.
- The game's console works through the keys (`~`, text, Enter; `Script.console()` in `scenarios.py`), so teleports and
  state queries (`coc`, `player.getpos`) are available to scenarios.
- The mouse script (`NV_MOVE_MOUSE`) moved no menu cursor in the main menu; menus are driven by keys.

### The opening quest, diagnosed (2026-11-16)

The input is not at fault. Keys reach the game in every state tried (console, the name box, message boxes). What was found
with the game's console (`getstage`, `sqv`) on the first room:

- The opening quest runs (its timers advance) and stops in the stage in which Doc's second line is due. Doc's first line
  finishes (its result script sets the next stage) and the second never does, with the threads real or stepped and with
  the fork's fixes on or off (so it is not the dialogue topic index). That second line is the first with an idle animation
  attached, so the stop is most likely in the animation the line waits for; not yet confirmed. Upstream-owned code.
- E, Enter, Space and a click do nothing in that state by design: the quest disables every control but looking in its first
  stage and enables them in a late one. It was never an input fault.
- The face menu (the mirror) opens, but its head is black, its right-hand screen is empty and neither Esc, Tab, Enter, the
  arrows nor the scripted mouse do anything in it (Esc opens the pause menu over it). A menu cursor does not follow the
  scripted mouse in any menu tried.

`chargen` therefore sets the stage that each unfinished line would have set, through the console (the console takes form
ids on this port, not editor names; the quest's id is the constant `OPENING_QUEST`), types the name, takes the first sex
choice and opens the face menu, where it stops. Reaching the SPECIAL, tag and trait menus and walking out needs that menu to
work first.

### Script clocks

`NV_MOVE_KEYS` and `NV_MOVE_MOUSE` frames count main-loop frames, which a loading screen spins at about 1 ms each, so a
step after a load depended on how long the load took. A frame written with a `p` before it (`p3399:1C:30`) counts only
frames that are not a loading screen, so it waits for the game's loads: `newgame` and `chargen` take the first room's steps
that way (`Script` items go through `on_play_clock`). The two clocks mix in one script; the movie skip stays on the
main-frame clock because it is pressed during a load. A step that waits for a named screen is still not there.

### What the engine lacks for outdoors

- (Not reproduced on 2026-11-16: `player.moveto` by form id loads the exterior.) A console teleport to the town (`coc goodsprings`) stopped the game in the first outdoor frames with
  `IDirect3DDevice9::CreateQuery(9) is not in the machine table` (the adapter table has no query lines on this machine;
  `d3d9-capture device queries` is a Windows tool). Until that is solved on Linux, scenarios that go outdoors will report
  that stop, which is what the verdict is for.
