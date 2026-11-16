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

Sites that use it: see `git grep soft_stop`. A site is converted only when the game's own code checks that call's result.
