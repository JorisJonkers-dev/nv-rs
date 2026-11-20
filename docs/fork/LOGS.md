# Reading a run's logs: `nv-rs-dev logs`

Fork-owned (`scripts/fork/logs.py`; `nv-rs-dev logs`, or `nv-rs-logs` linked next to `nv-rs-dev`:
`ln -s <worktree>/scripts/fork/local/nv-rs-logs ~/.local/bin/nv-rs-logs`). Standard library only.

The terminal of a run is short (docs/fork/LOGGING.md); the whole record is in the run's files in `~/.local/state/nv-rs/logs`
(`NV_LOG_DIR`, `$XDG_STATE_HOME`, `%LOCALAPPDATA%\nv-rs\logs` on Windows): `nv-rs-<unix>-<pid>.jsonl` (JSON lines),
`nv-rs-<unix>-<pid>.log` (the same lines as text) and `run-summary-<unix>-<pid>.json` (schema `nv-rs-run-summary/1`, HITCHES.md).
This tool reads them and prints the relevant part. It reads the JSON lines and the summary, and falls back to the text log when
the JSON file is gone, and to the log alone when the run has no summary (a run that never got to write one).

| Command | Prints |
| --- | --- |
| `logs` | the latest run: verdict, fps, p99, 1% and 0.1% low, times, hitch counts by cause, errors, warnings, soft stops, flags, peaks, the key facts (version, GPU, driver, resolution, session), the stops with their address chain, the warnings and errors by kind (counted), the worst three hitches, and the file paths. Under 40 lines for a normal run, under 80 for a bad one |
| `logs --errors` | every kind of warning and error with its count and frame range, and the stops |
| `logs --hitches` | hitch counts and time by cause, and the worst ten |
| `logs --tail N [--level warn] [--target window,d3d9,sound,files,game]` | the last N lines, filtered (`--level` is the most verbose level shown) |
| `logs --grep PATTERN` | lines whose text or target match (regular expression, case-insensitive); `--limit N` (60) |
| `logs --around 1234` / `f1234` / `5000ms` `[--window N]` | the lines near a frame (default 30 frames each side) or a run time (default 2000 ms); also takes `--level`, `--target` |
| `logs --run ID` | another run: its id (`<unix>-<pid>`, a prefix is enough), `latest`, `latest-bad` (the newest run that was flagged, stopped or logged an error) or a path to a `.jsonl`, `.log` or summary file |
| `logs --diff A B` | the summary change from run A to run B and the regressions (fps down 5%, p99 or loading up 10%, hitches a minute up by 0.5, more errors, warnings or soft stops, a peak up 10%, a new flag); exit status 1 when there is one |
| `logs --json` | the same view as one JSON document |
| `logs --bundle [FILE]` | one small text file to send after a bad run (default `nv-rs-bundle-<id>.txt` here): the summary, stops, all kinds of errors and warnings, the worst hitches, the environment and the last lines before the end. No game data; the home folder, user names, Windows user paths and the game's folder are replaced (`~`, `<game folder>`) |

Colors on a terminal (`--color auto|always|never`; `NO_COLOR` is honoured). Exit status is 2 when no run is found.

## What a local wrapper should do

The maintainer's local wrapper is not part of this repository and is not changed by it. With the quiet terminal it should print the viewer's output as
it is, without keeping it as the record, and afterwards run `nv-rs-dev logs` (or `nv-rs-logs`), so a run ends with the short report. A
bad run (`logs --run latest-bad --bundle`) gives a file to send. The viewer's files already hold everything; the wrapper's own `tee`
log is only needed for the lines the viewer does not log (the wrapper's header, the watchdog, the exit status).

## Tests

`scripts/fork/tests/test_logs.py` runs the tool on synthetic runs (`fixtures/logs`: a normal run and one that stopped, each with its
JSON lines, text log and summary) and compares the compact outputs with golden files (`fixtures/logs-golden`; `UPDATE_GOLDEN=1` rewrites
them when a change is meant), and checks the fallbacks, the run selection, the diff, the JSON output, the colors and the bundle's
scrubbing.
