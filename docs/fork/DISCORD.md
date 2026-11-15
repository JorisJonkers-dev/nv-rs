# Discord Rich Presence

While the game runs under `nv-viewer`, the player's Discord shows "Playing Fallout: New Rust Vegas" with the build
(`Build 0.1.0`), a coarse state (`Starting`, then `In game` from the first frame) and the elapsed time. The activity's
name is the Discord application's name. Nothing else is sent: no player, place, plugin or mod names, no game data.

Upstream (`slaterain/nv-rs`) has no such feature (checked 2026-11-15); it is fork-only and listed in `DIVERGENCE.md`.

## How it behaves

- The crate is `crates/fork-presence`, pure Rust, on the pinned `discord-rich-presence` 1.0.0 (MIT, compatible with
  GPL-3.0-or-later). It speaks Discord's IPC (the unix socket in `XDG_RUNTIME_DIR`, flatpak and snap paths included, or the
  Windows named pipe). The cargo feature `discord` is on by default and built on Linux and Windows; macOS is added later.
- One background thread connects lazily, retries with backoff (5 s doubling to 5 min) and stays silent when Discord is not
  running. The viewer's hook is two calls in `viewer/src/main.rs`; the game never waits on it and it cannot crash the run.
- Opt out with `NV_DISCORD=0` (also `off`, `false`, `no`). This does not depend on `Fallout.ini`.

## The application ID (owner step)

Presence stays off until a Discord application ID is set, and one info line says why. The ID is read from
`NV_DISCORD_APP_ID`, else from `DEFAULT_APP_ID` in `crates/fork-presence/src/lib.rs`, a placeholder marked `TODO(owner)`.
To switch it on for everyone: create an application named "Fallout: New Rust Vegas" at
https://discord.com/developers/applications, copy its Application ID into `DEFAULT_APP_ID` and merge.
