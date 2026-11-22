# Run labels: telling game runs apart in a system monitor

Several game runs can be on one machine at once (bench runs, picture runs, start checks, gate runs). Each carries a label
of the form `<kind>-<scenario or row>-<variant>`, for example `bench-views-lake-mead-lod0151`, `gate14-mojave-ttw`,
`smoke-0201-plain`.

## What each tool shows

| Tool | Shows |
| --- | --- |
| Mission Center (Processes list) | the label as the process name, with that process's CPU and GPU. It lists the file name of `/proc/PID/exe`, not argv[0], not a symlink's name (a symlink is resolved to its target) |
| `ps`, `top`, `htop`, `amdgpu_top` | the label cut to 15 characters (the kernel's `comm`), the whole label in the command line |
| `systemd-cgtop`, `systemctl --user status 'nvgame-*'` | a scope `nvgame-<label>-<n>.scope` in `nv-games.slice`, with CPU, memory and IO accounting and a description naming the label and who started it |

Mission Center does not show cgroups or user units; only the executable's file name works there. So the viewer is started
through a hard link named after the label, next to the real file (libraries and data found beside the program are found),
or a copy where a link cannot be made. The link is removed when the run ends.

## The scene tool

`nv-rs-scene` starts every engine as a `gamerun` scope labelled `<--label>-<variant>` (`scene-<kind>-<variant>-<pid>` without
`--label`), with `--who` as the owner and a memory limit (docs/fork/SCENE.md, "What caps a run's memory").

## Other runs: `gamerun`

For a run the bench does not start (a start check, a picture run, a raul window) use the wrapper
`~/.local/bin/gamerun <label> [--mem 10G] [--who NAME] [--rename PATH] -- <command...>`. It gives the run the same scope,
slice, description and process name, keeps a memory cap (default 10G, no swap), passes the exit status through and does not
take the game lock: take it yourself with `flock -s` (or `-x` for a time measurement) around it.
