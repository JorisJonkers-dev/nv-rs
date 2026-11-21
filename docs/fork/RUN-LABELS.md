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

## The bench

`nv-rs-dev bench run` labels the viewer by default: `bench-<scenario>-<variant>`, the variant being the game folder's name
(its parent's when the folder is called `game`) and the preset when it is not `fixes`. To choose the label, set
`NV_BENCH_LABEL=<label>` in the environment. The label is written to `label.txt` and a `# label:` line in `viewer.log` of the
run directory and to `label` and `scope` in the run's record, so results and monitor rows can be matched. The nested
compositor's scope is `nvsway-<label>-<n>.scope` in the same slice. The check for other viewers during a measurement
counts a labelled viewer (the first process of an `nvgame-*` scope) as well as a process called `nv-viewer*`.

## Other runs: `gamerun`

For a run the bench does not start (a start check, a picture run, a raul window) use the wrapper
`~/.local/bin/gamerun <label> [--mem 10G] [--who NAME] [--rename PATH] -- <command...>`. It gives the run the same scope,
slice, description and process name, keeps a memory cap (default 10G, no swap), passes the exit status through and does not
take the game lock: take it yourself with `flock -s` (or `-x` for a time measurement) around it.
