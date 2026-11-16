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

Text goes to stderr. It keeps the old shape so scripts still match: `info` lines are bare, `warn` and `error` get a
`warning: ` / `error: ` prefix, `debug` and `trace` get `[debug target] `. The stop line and the status line are written whole.
`NV_LOG_FORMAT=full` adds the time, level and target to every line.

| Variable | Meaning |
| --- | --- |
| `NV_LOG` | Text filter, `RUST_LOG` syntax: `info` (default), `warn,d3d9=debug,sound=off`. Falls back to `RUST_LOG`. |
| `NV_LOG_FORMAT` | `plain` (default) or `full`. |
| `NV_STATUS_SECS` | The status line's interval; 5 by default, `0` for none. It is `info` on target `status`, so `NV_LOG=warn` also hides it. |
| `NV_LOG_JSON` | `auto` (default), `off`, or a path. |
| `NV_LOG_JSON_FILTER` | The file's own filter; `debug` by default, so the file holds more than the screen. |
| `NV_LOG_JSON_MAX_MB` | Size bound of the file, 64 by default. |
| `NV_STOP_POLICY` | `soft` (default off Windows) or `halt`. |

## JSON log

One object per line: `ts` (UTC, ms), `level`, `target`, `message`, `fields` (strings), `frame` (the game's main frame, `null`
before the first), `thread`. `auto` writes `$XDG_STATE_HOME/nv-rs/logs/nv-rs-<unix seconds>-<pid>.jsonl`
(`~/.local/state/nv-rs/logs/` without XDG; `%LOCALAPPDATA%\nv-rs\logs` on Windows) and keeps the newest nine files. At the size
bound the file becomes `<file>.1` (one older copy is dropped) and a new one starts. Warnings and errors are flushed at once, other
lines within a second, and everything at exit.

The owner-local launcher `~/.local/bin/nv-rs-run` is not changed by the fork: the log is on by default, so it needs nothing. To
choose the place, export `NV_LOG_JSON=/path/run.jsonl` there; to see more on screen, `NV_LOG=debug`.

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

Not yet traced: the game's menus (the menu stack, main, loading and pause menus), which are the game's own translated code and have
no intercepted seam, the cause of a music stop beyond "the game stopped it", the kind of load (new game, save, cell), and cell or
world loads. Those need hooks in the engine crates; the trace is ready for them (`fork_log::game_info!("menu.open", ...)`).

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

The exit summary (`Log summary: N errors, M warnings, K soft stops (calls...)`) is printed by `engine_platform::end_process`, with a
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
