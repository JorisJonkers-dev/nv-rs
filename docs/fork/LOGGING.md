# Logging, the JSON log and soft stops

Fork-owned (`crates/fork-log`). The crate has no dependency and starts no thread.

## How logging worked before

- No logging framework: the workspace has no `tracing`, `log` or `env_logger` use. About 70 files print with `eprintln!` and
  `println!` (the window layer, `d3d9-wgpu`, the platform layer's sound and Direct3D calls, the host).
- No levels, no targets, no filter: `RUST_LOG` is read by nothing. Each message is one text line; a leading word (`NV_D3D9 wgpu:`,
  `stop:`) is the only structure.
- The viewer prints the status line (`Game's movement: main N frames; ...`) to stdout every 5 s. Launchers and
  `scripts/fork/bench/parse.py` read it as a heartbeat.
- A stop is main's coroutine ending on a panic (a function not translated, a fault, a platform call that "is not provided"):
  `host::player_move` prints `Game's main stopped in frame N: <why> (in <exe addresses>)` and the process ends with status 3.
  `--movement-missing FILE` writes `FILE` (untranslated addresses), `FILE.stops.tsv` and `FILE.static.tsv`.
- `NV_MEMLOG=<seconds>` (`fork-memlog`, MEMLOG.md) prints one `key=value` line of memory numbers; it stays as it is.

## Levels and targets

`fork_log::{error,warn,info,debug,trace}!(target: "files", "...")`. Targets name the area: `window`, `d3d9`, `sound`, `files`,
`script`, `loader`, `stop`, `status`, `host`. Rules: a stop or a recoverable failure is `error` or `warn`; one-time facts
(adapter, present mode, thread mode) are `info`; anything per frame or per call is `debug` or `trace`.

Terminal text goes to stderr (the status heartbeat to stdout). It keeps the old shape so scripts still match: `info` lines are bare, `warn` and `error` get a
`warning: ` / `error: ` prefix, `debug` and `trace` get `[debug target] `. The stop line and the status line are written whole.
`NV_LOG_FORMAT=full` adds the time, level and target to every line.

| Variable | Meaning |
| --- | --- |
| `NV_LOG_TERM` | What the terminal shows: `quiet` (default), `normal`, `verbose`, `off`, or a filter in `RUST_LOG` syntax (`warn,d3d9=debug`). See "The terminal". |
| `NV_LOG` | Falls back to `RUST_LOG`. The filter of the terminal when `NV_LOG_TERM` is not set (as before), and always widens the files' filter (`NV_LOG=game=trace` puts the trace in the files). |
| `NV_LOG_FORMAT` | `plain` (default) or `full`. |
| `NV_STATUS_SECS` | The status interval; 5 by default, `0` for none. It drives the one-line live status on a terminal (below) and the `Game's movement` heartbeat lines (`info` on target `status`: file only unless the terminal shows `info`, as `NV_LOG_TERM=verbose` or a bench run's `RUST_LOG=info` does). |
| `NV_LOG_JSON` | `auto` (default), `off`, or a path. |
| `NV_LOG_JSON_FILTER` | The files' own filter; `debug` by default, so the files hold more than the screen does. |
| `NV_LOG_TEXT` | `off` stops the plain-text log (below); on by default with the JSON file. |
| `NV_LOG_JSON_MAX_MB` | Size bound of the file, 64 by default. |
| `NV_STOP_POLICY` | `soft` (default off Windows) or `halt`. |

## The terminal

The terminal is quiet by default (`NV_LOG_TERM=quiet`). What it shows, in this order:

1. A start header of 3 to 6 lines (build, GPU and driver, resolution and display, audio and threads, the log paths), printed when the first
   frame has gone. The lines come from the run's facts (`fork_log::fact`; `fork_hitch::set_meta` sets them too).
2. Warnings and errors, the same message (digits ignored) at most three times, then counted: at the end `warning: ... (x repeated 40 times)`.
3. Stops with their reason and exe address chain (the `Game's main stopped in frame N: ... (in ...)` line), soft stops (`error` on
   target `stop`, throttled already), and a panic (the previous panic hook prints it).
4. On a terminal, one self-overwriting status line `f 123456  143 fps  p99 7 ms  hitches 0` every `NV_STATUS_SECS` (5 s), drawn on
   stderr with carriage returns only (no escape codes, so a Windows console behaves the same). Off when stderr is a pipe or a file,
   with `NV_STATUS_SECS=0`, and when the terminal already shows the `status` lines.
5. At the end the final summary: the verdict (`OK:`, `FLAGGED:` or `FAILED:`), fps, p99, 1% and 0.1% low, frames, play and loading time,
   hitch count and the worst three, peaks, soft stops, errors, warnings, flags, and the paths of the log and the run summary.

Fatal failures always print, at any `NV_LOG_TERM` (even `off`): `fork_log::fatal!` (a stop line, a graphics device lost, no game data,
a failed start of the host); they are errors, so they are also in both files.

| `NV_LOG_TERM` | Shows |
| --- | --- |
| `quiet` | the above |
| `normal` | quiet plus the one-time facts (`info`), without the periodic lines (`status`, `hitch`, `game`) |
| `verbose` | everything at `debug`, every repeat, no limits (the files' detail on the screen) |
| `off` | the fatal failures only |
| a filter | `warn,d3d9=debug`, as `RUST_LOG` writes it; no repeat limit |

Precedence: `NV_LOG_TERM` for the terminal, else `NV_LOG`, else `RUST_LOG` (as the filter, no repeat limit: what they did before), else
`quiet`. The files' filter is `NV_LOG_JSON_FILTER`, else `debug` widened by `NV_LOG`/`RUST_LOG`. The per-present stats, the pipeline
reports, `Game's movement`, the preset's fix list, the shader start report and the one-time facts are `info` or `debug`: they are in
the files and not on a quiet terminal. Messages of a level no sink shows are not formatted (a comparison and a return).

The files: `nv-rs-<unix>-<pid>.jsonl` (JSON lines) and `nv-rs-<unix>-<pid>.log` beside it, the same records as plain text
(`2026-11-16T09:40:19.586Z INFO  d3d9 f123: message`; frame `-` before the first), with the same bound and rotation (`.1`). The newest
nine runs are kept, all their files together. `nv-rs-dev logs` reads them (below).

A local wrapper may pipe the viewer through `tee` into its own log; it should let the terminal output through as it is
(it is short now) and, after the run, print `nv-rs-dev logs`, which says what happened in under 40 lines. It no longer needs to keep
the viewer's output as the record: the two files hold everything. A pipe is not a terminal, so the live status line is off there.

## JSON log

One object per line: `ts` (UTC, ms), `level`, `target`, `message`, `fields` (strings), `frame` (the game's main frame, `null`
before the first), `thread`. `auto` writes `$XDG_STATE_HOME/nv-rs/logs/nv-rs-<unix seconds>-<pid>.jsonl`
(`~/.local/state/nv-rs/logs/` without XDG; `%LOCALAPPDATA%\nv-rs\logs` on Windows) and keeps the newest nine files. At the size
bound the file becomes `<file>.1` (one older copy is dropped) and a new one starts. Warnings and errors are flushed at once, other
lines within a second, and everything at exit.

The log is on by default, so a launcher needs nothing. To choose the place, export `NV_LOG_JSON=/path/run.jsonl` (the text log goes
beside it, `.log`); to see more on screen, `NV_LOG_TERM=verbose`.

Final records (`"event"` set) carry `reason`, `frame`, `thread`, counts of `soft_stops`, `errors` and `warnings`, and `last_lines`
(the last 200 lines either sink took):

- `stop`: the game's main stopped. Also `class` (`untranslated`, `memory`, `platform`, `other`) and `addresses` (the exe address
  chain of `(in ...)`).
- `panic`: a panic anywhere outside the game's own stops (the panic hook, then the previous hook runs), with `location`.
- `exit`: the process ends, with the summary line.

No game data goes into a record: only names of calls, counts, paths of the machine's own files and the engine's messages.

## The `game` event trace

For diagnosing a run from its log (the loading-screen bug): every event is on target `game`, with the main frame and a monotonic
millisecond clock (`[f123 +4567ms] movie.open: ...`; JSON fields `kind`, `ms`, `frame`). It is in the JSON file always, and on the
screen with `NV_LOG=game=debug` (or `game=info`). Repeating kinds are throttled (the first five, then every thousandth) and their
counts are in the exit summary.

| Kind | When |
| --- | --- |
| `movie.open`, `movie.start`, `movie.frame`, `movie.pause`, `movie.close` | Bink: name, size, frame count, rate, sound; close says whether it played to its end or was closed early (skipped) |
| `input.key`, `input.mouse` | a change of state that reaches the game's DirectInput: Escape, Enter, Space and keypad Enter at info, other keys throttled |
| `audio.create`, `audio.play`, `audio.stop`, `audio.volume`, `audio.stream.play`, `audio.stream.stop`, `audio.stream.volume` | DirectSound buffers (id = the buffer's guest address) and the movie/music streams |
| `gpu.texture.create`, `gpu.texture.destroy`, `gpu.surface.create`, `gpu.render_target` | textures and surfaces at screen scale (512x384 or more) and every render target |
| `draw.fixed_quad` | a draw without shaders (the movie quad): viewport, target size, texture size and format |
| `present.source`, `present` | which surface the present shows, when it changes; each present throttled |
| `loading.begin`, `loading.end` | the start's loading screen (the d3d9 layer's own) |

From the game's own state (`crates/host/src/fork_trace.rs`, read-only, once a frame, changes only; the words are those
`loadingmenu.rs` names): `menu.loading` (the loading menu is up or gone, and whether it is the game's first load), `load.thread`
(the loading thread object exists: a load runs), `load.music` and `load.movie` (the movie player word set or cleared), and
`menu.stack` (the interface manager's menu stack, `+0x114` ten words of menu class ids, and its menu mode `+0x0c`: how many menus are
open, their class ids bottom to top in hex, the mode; logged when either changes, so a message box, the pause menu or the Reflectron
(`0x40c`) shows when it opens and closes).

`load.kind` (what began a load: a new game or a saved game, from the script extender's own co-save moments
(`nvse::saveload`, the seams at the new game and `PreLoadGame`), told when the moment comes; a fast travel, by the game's
fast-travel flag read when the loading menu opens, or a door or another cell change, told when the loading menu closes
and no new game or save began in it). A new game or save whose loading menu never opens leaves its moment to be taken for
the next load's, which is then not told as a door.

## Soft stops

A stop is classified by its reason (`fork_log::classify`). `untranslated` and `memory` always halt: the port itself is wrong and
going on runs the game on garbage. `platform` (a Win32 or COM call the port does not provide for these arguments) is recoverable
when the game handles that call's documented failure.

`fork_log::soft_stop(area, call, format_args!(...))` at such a site returns `true` under `NV_STOP_POLICY=soft`, after logging an
`error` on target `stop` (the first five of a call, then every thousandth) and counting it; the site then returns the failure
result. Under `halt` it returns `false` and the site panics as before:

```rust
if fork_log::soft_stop("d3d9", "IDirect3DQuery9::Issue", format_args!("flags {flags:#x}")) {
    return D3DERR_INVALIDCALL;
}
panic!("IDirect3DQuery9::Issue({flags:#x}) is not provided");
```

The exit summary (`Log summary: N errors, M warnings, K soft stops (calls...)`, `info`, so in the files; the terminal shows the final summary lines) is written by `engine_platform::end_process`, with a
last record in the JSON file.

Sites that use it (`git grep soft_stop`), each with the failure the call documents:

| Call | Result when soft |
| --- | --- |
| `BinkOpen` (flags, a file the decoder cannot read, an unmeasured revision) | `0`, no movie |
| `IDirect3DQuery9::Issue` (other flags), `GetData` of an occlusion query without `NV_D3D9=real` | `D3DERR_INVALIDCALL` |
| `DirectSoundCreate8` (a device GUID or outer object) | `DSERR_INVALIDPARAM`, a null object |
| `IDirectSoundBuffer::Lock` (other flags) | `DSERR_INVALIDPARAM` |

A site is converted only when the game's own code checks that call's result. `IDirect3D9::CreateDevice` stays a halt: the game cannot
run without a device, and the stop names why. The file layer (`FindFirstFileA`, short names, directory listings) is converted by
the change that fixes it, with the same helper. `fork_log::stop::with_policy` forces a policy on one thread for tests.

## Converted and not yet

Converted to levels: the window layer (`window`), `d3d9-wgpu`'s device (`d3d9`: per-present reports and pipeline timings are
`debug`; one-time facts `info`; refusals `warn`; device loss `error`), the host's start-up lines, and the runtime messages of
sound. Not yet: the remaining `eprintln!` of the platform layer's Direct3D recorder and `d3dx`, the thread dump of
`engine-core` on a stop, and `loading.rs`; they still reach stderr as text and not the JSON file.
