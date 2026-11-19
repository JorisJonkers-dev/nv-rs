# ADR-0017: The engine is linked into another program, started as a child process

> This is a fork decision record kept under docs/fork/adr/ because docs/adr/ is upstream-owned; it uses the next free number of docs/adr (0016 is the last there).

**Status:** Accepted (maintainer decision on the tracking issue, 2026-11-18)
**Date:** 2026-11-18

## Context

The Toolbox program, raul, must check data, read saves, precompile shaders and start the game without shipping or finding a separate engine program.

## Decision

1. Two fork-owned crates, `fork-api` (a typed in-process facade, with `API_VERSION`) and `fork-play` (`play_main`), both `publish = false`. The Toolbox consumes them as path dependencies through its submodule. The release tag is the version; there is no registry.
2. The game always runs as a child process. The program re-executes itself with the reserved first argument `--play-mode <DATA>`, so engine crashes, out-of-memory kills and GPU driver aborts stay out of the program.
3. Shader precompile runs in that child (`--precompile-shaders`), not in process, because it opens a GPU device whose driver may abort or hang. So there is no in-process precompile step.
4. Stop is a file named by `NV_STOP_FILE`, polled once a second and observed on the game's own thread, with exit status 143. Signals are not the stop.
5. Exit codes overlap the game's own and are documented, not changed.
6. The viewer's loop is included by `#[path]` into `fork-play`, with a minimal edit of `viewer/src/main.rs` (`pub fn run`, one stop poll), so there is one loop.
7. One binary behind a feature `engine` (on in release builds, off in fast loops), subject to a three-system spike of the Windows DPI manifest. If that spike fails, the fallback is a sidecar program beside the program.

## Consequences

- The profile and patch lines do not travel; see [CONSUMING.md](../CONSUMING.md).
- Build time and memory grow in the consumer.
- The play-mode contract is [PLAY-MODE.md](../PLAY-MODE.md).
- Upstream edits to `viewer/src/main.rs` can conflict at replay, and a person merges them.
