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

Options of `run`: `--windowed --yes-windowed` (a real window; refused without both), `--threads real|stepped`,
`--preset fixes|vanilla`, `--resolution WxH`, `--sound silent|out|none`, `--vram-limit-pct N`, `--mem 12G`, `--scale F`
(shorten or lengthen a scenario), `--keys` and `--mouse` (extra scripted input, `NV_MOVE_KEYS` syntax), `--shots DIR[,ms]`
(debugging: the game's frames saved for you to look at, never commit them).

Runs go to `~/.local/state/nv-rs/bench/<scenario>-<time>/` (`NV_BENCH_DIR` changes it): `viewer.log`, `frames.tsv`,
`samples.tsv`, `summary.json`, `report.md`, `report.html` (inline SVG charts, no script). Numbers only: no game data.

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

## Modes and safety

- `capped` (default): 144 frames a second limit, mailbox present: pacing and 1% lows. `uncapped`: no limit,
  `NV_PRESENT=immediate`: peak fps (offscreen, the swap chain does not exist, so it measures the work).
- Offscreen (`NV_D3D9_OFFSCREEN=1`) by default: no window, no compositor involved. Sound runs but the speakers get
  silence (`NV_SOUND=out` with a hidden window); the game's voice lines and movies need that clock.
- The watchdog samples VRAM and GTT every second and stops the viewer (SIGTERM, then SIGKILL) at `--vram-limit-pct`
  (default 70, or `NV_VRAM_LIMIT_PCT`). The viewer also runs in a memory-capped systemd scope (`--mem`, default 12G), with
  no CPU quota or nice level, which would distort the numbers.
- Thresholds of `compare` (percent worse, with a noise floor): fps 5, 1% low 10, p99 10, max 50, RSS 10, VRAM 10, GTT 10,
  skipped draws 20; wgpu errors, stops and untranslated addresses regress on any increase. `--threshold fps=3` changes one.

## Scenarios

| Scenario | What it does | State |
| --- | --- | --- |
| `boot` | Skips the start-up movies, sits on the main menu for 50 s | works |
| `newgame` | Menu, New, intro movie, the pack messages, to the first room (Doc Mitchell's house), 20 s there. Capped mode only | works (see below) |

Planned: `goodsprings`, `novac`, `strip`, `soak` (a long looped walk watching memory growth), each its own change.

### How the input is driven, and its limits

The script is by main-loop frame, open loop. Findings that shaped it (2026-11-15, offscreen, 144 Hz cap):

- A key tap shorter than about 20 frames is dropped by the menus; scenarios hold keys for 30 frames.
- The first arrow press after the menu appears only wakes its keyboard mode and moves nothing. The script presses twice.
- The intro movie plays on the loading screen at about 800 frames a second and is skipped with Esc; every frame count
  during a loading screen is wall-time dependent, which is why `newgame` is timed in frames and valid only capped.
- The game's console works through the keys (`~`, text, Enter; `Script.console()` in `scenarios.py`), so teleports and
  state queries (`coc`, `player.getpos`) are available to scenarios.
- The mouse script (`NV_MOVE_MOUSE`) moved no menu cursor in the main menu; menus are driven by keys.

### What the engine lacks for the rest of the opening and for outdoors

- Doc Mitchell's opening dialogue never starts: after the first room is shown the player stays seated and E, Enter, Space
  and a click do nothing for 60 s+. Character creation and the tutorial therefore cannot be reached by input alone. Either
  the opening's quest script does not run, or the input does not reach the first-person controls in that state. Not yet
  diagnosed.
- A console teleport to the town (`coc goodsprings`) stops the game in the first outdoor frames with
  `IDirect3DDevice9::CreateQuery(9) is not in the machine table` (the adapter table has no query lines on this machine;
  `d3d9-capture device queries` is a Windows tool). Until that is solved on Linux, scenarios that go outdoors will report
  that stop, which is what the verdict is for.
- The script cannot wait for an event. A scripted step that depends on a screen change (a load that takes longer on a cold
  disk) can miss. An engine-side `NV_MOVE_KEYS` that counts only non-loading frames, or a key that waits for a named
  screen, would make every scenario closed loop.
