# Play mode: running the viewer as a child process

A fork-only addition. `fork-play` (`crates/fork-play`) lets a program that links the engine run the game as a child
process, so a crash of the engine stays out of the program. The Toolbox (raul) is the consumer this is built for.

## What it is

`fork-play::play_main() -> !` runs the viewer's loop, the same loop the standalone `nv-viewer` runs
(`viewer/src/main.rs`, included by `fork-play` so there is one copy). The program calls it in the child, and the
child is the game. An engine crash, a fault or an out-of-memory kill ends the child, not the program.

## Arguments

The program starts itself again as:

```
<program> --play-mode <DATA> [viewer arguments]
```

- `--play-mode` is a reserved dashed word. The viewer skips it and writes one warning line to the log.
- `<DATA>` is the first word without a dash: the game's data folder.
- The viewer's own flags work unchanged: `--check`, `--precompile-shaders` (the shader precompile runs in the play
  child, not in the program), `--movement-missing FILE`, `--movement-probe` and `--help`.
- `--find` and `--build-mods` are not available through `--play-mode`: the viewer recognises them only as the first argument of the process.

Differences from the standalone program:

- `viewer/build.rs` (the Windows icon) is not run, so the program that links the crate carries its own icon.
- `CARGO_PKG_VERSION` in the viewer's code is the version of `fork-play`.
- Do not call `fork_log::init` before `play_main`. It is not guarded against a second call, and the viewer's code
  starts the log.

## Environment

A launcher may scrub the environment, so the caller passes these on purpose:

| Variable | Meaning |
| --- | --- |
| `NV_D3D9` | the Direct3D 9 backend (`wgpu`, or `real` for the real `d3d9.dll`) |
| `NV_SOUND` | the sound output; the sound layer reads `NV_SOUND_BACKEND` and `NV_SOUND_LAYOUT` |
| `NV_LOG_JSON` | the JSON log: `auto`, `off`, or a path in the instance's log folder |
| `NV_LOG_TERM` | the terminal filter; `normal` keeps stdout quiet; `off` silences it entirely; see Logs for the status line |
| `NV_STATUS_SECS` | the status interval in seconds (default 5) |
| `NV_WGPU_CACHE` | the folder that keeps the driver's compiled pipelines between runs |
| `NV_STOP_FILE` | the stop file (see Stop below) |

On Linux the caller also passes `HOME`, `XDG_*`, `DISPLAY`, `WAYLAND_DISPLAY` and `XAUTHORITY`, so the window opens.

The current directory of the child is the game folder.

The meanings of the `NV_LOG*` variables, and the default log locations, are in [LOGGING.md](LOGGING.md).

## Exit codes

From the module documentation of `crates/fork-play/src/lib.rs`:

| Code | Meaning |
|---|---|
| `0..` | the game's own code (its return or its `ExitProcess`); 0 also for `--help` and a successful `--check` |
| `1` (`EXIT_START`) | a start-up error: no data, the executable not accepted, a failed `--check`; also the window layer's close path that skips the exit hooks |
| `2` (`EXIT_ARGUMENTS`) | an argument error |
| `3` (`EXIT_GAME_STOPPED`) | the game's code stopped (a fault, a function not translated, a call the platform does not provide) |
| `69` (`EXIT_SYSTEM_REFUSED`) | the system refused the start (host start, shader precompile) |
| `143` (`EXIT_STOP_REQUESTED`) | ended by the stop file |
| a signal, or `101` or `134` | the process crashed (the system's) |

The game's own code is any value, so it can equal 1, 2, 3, 69 or 143. A caller trusts a code only together with the
log (`Game's main stopped in frame N` for 3) and with whether it asked for the stop. This is a limit of the existing
contract: the game's own status is passed through by code that the upstream project owns. It is documented here, not
changed.

`fork_play::ExitClass` names these classes for a caller that has not read the log. `ExitClass::of(code)` returns
`Finished`, `StartFailed`, `BadArguments`, `GameStopped`, `SystemRefused`, `StopRequested`, or `Other(code)` for any
other value. A signal has no code, so the caller classifies a signal as a crash before it calls `of`.

## Stop

A signal or a kill skips the exit hooks, and the pipeline cache and the save flush are lost. So the stop is a file:

- A signal would look like a crash. `fork-hitch` handles SIGTERM by writing its summary and then re-raising it, so
  SIGTERM ends the process as a crash to the caller.
- A Windows GUI-subsystem child has no console, so there is no CTRL_BREAK to send.

How it works:

1. The caller sets `NV_STOP_FILE` to a path and starts the child.
2. To stop the game, the caller creates that file. A file left from an earlier run stops the game at once, so the
   caller removes the file before the start.
3. A thread started by `play_main` checks for the file once a second (`STOP_POLL`). When it exists, the thread logs
   a line on target `host` and calls `fork_log::request_stop`.
4. The game's own thread sees the request between two frames, runs the exit hooks, and ends the process with status 143.
5. The caller waits for the exit. It kills the process by process id only after a grace period longer than the save
   and cache flush. Five seconds is the minimum. A kill after the grace period may have lost the cache flush.

A frame in which the game's code does not return (a loading screen) delays the stop until that frame returns. A stop
does not write the `--movement-missing` lists.

An unset or empty `NV_STOP_FILE` means that nothing is watched.

## Logs

`fork-log` writes two files: a JSON file and a text file beside it, both under the folder set by `NV_LOG_JSON`
(default `auto`, see [LOGGING.md](LOGGING.md)). The terminal is quiet with `NV_LOG_TERM=normal`.

- The status line `Game's movement ...` (target `status`) is the heartbeat, every `NV_STATUS_SECS` seconds. It is
  always written to the JSON file. It reaches stdout only when the terminal filter shows `info` on `status`, for
  example `NV_LOG_TERM=warn,status=info`.
- The live status line that overwrites itself is drawn only on a terminal. A child whose output is a pipe has none.
- The game sends no structured events in this version. The caller reads stdout and stderr line by line and makes its
  own start, heartbeat, stop and end events.

## What the child can do to the program

Nothing beyond itself.

- **Memory.** The engine reserves a 4 GiB guest address window (about 3.2 GiB of heap address space, committed on
  demand). An out-of-memory kill, a failed reservation or a stack overflow kills only the child.
- **GPU or driver abort.** Only the child ends.
- **Panic.** `fork_log` installs a panic hook in the child, and the child then exits. The translated C runtime `abort`
  is a panic, not a process abort.
- **Orphan.** If the program dies, the game keeps running in its own window. There is no parent-death signal, on
  purpose. The caller's lock should name the child's process id, so that the next start finds the child.
- **Display (Windows).** The child is made DPI-unaware at start. A program whose own executable carries a DPI
  manifest, or that sets DPI awareness itself before the start, makes that call fail, and the window then scales
  differently. One executable has one manifest.
- **Shared files.** The child writes saves, caches and logs. The caller must not run a write job, such as an install or
  an update of the game folder, while the child runs.
