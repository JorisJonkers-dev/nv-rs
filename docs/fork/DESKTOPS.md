# Desktops: window, pointer and screen on Linux

What the fork's window layer (`crates/fork-window`, winit 0.30.13 underneath) does on each desktop, what was checked and
how, and what was not. The layer is one binary for X11 and Wayland: it takes Wayland when the session is one and X11
otherwise (`WINIT_UNIX_BACKEND=x11` forces X11). On start it logs one line:

```
fork-window: window 1280x720 px at scale 1.5 on wayland, pointer locked, idle inhibit wayland
```

Size is in physical pixels (the swap chain is that size), scale is the compositor's, `pointer` is `locked`, `confined` or
`none`, `idle inhibit` is `wayland`, `xdg-screensaver` or `none`. The line is repeated when the size or scale changes.
Read it first when a desktop misbehaves.

## Matrix

Legend: **V** verified by a run (CI or by hand, said in the Evidence column), **R** by reading winit's or the compositor's
source and documentation only, **-** not supported, **?** not known.

| | X11 (any WM) | GNOME Wayland (Mutter) | KDE Wayland (KWin) | Hyprland | sway |
| --- | --- | --- | --- | --- | --- |
| Borderless full screen | R: `_NET_WM_STATE_FULLSCREEN` on the pick | V on the owner's machine (PR 69) | R | R | V (CI, headless) |
| Which output | The pick (`NV_MONITOR`, last used, primary, first) | The compositor: where the pointer or focus is; `NV_MONITOR` names one | The compositor places it; `NV_MONITOR` names one | The compositor: the focused monitor; the launcher can name it | The focused output; the launcher can name it |
| Monitor detection | RandR via winit | `wl_output` via winit | same | same | same |
| Pointer lock | - (X11 has no lock) | R: `zwp_pointer_constraints_v1` | R | R | winit asks; accepted (log says `locked`) |
| Pointer confine | R: `XGrabPointer` | R | R | R | R |
| Relative motion | R: XInput2 raw | R: `zwp_relative_pointer_v1` | R | R | R |
| Cursor hide | R | R | R | R | R |
| Focus loss and regain | Held keys and buttons are released on loss; the pointer is held again and fullscreen restored on regain (unit-tested events) | same | same | same | same |
| Alt-Tab | The WM's; the game gets `Focused(false)` | Mutter's; Super also | KWin's | Binds are the user's | Binds are the user's |
| Fractional scale | R: no scaling (X11 `Xft.dpi` is not read) | R: `wp_fractional_scale_v1` | R | R | V (CI, scale 1.5: window 1280x720 px on a 1280x720 output, swap chain the same) |
| Native-resolution swap chain | n/a | R: winit sets `wp_viewporter`'s destination | R | R | V (above) |
| Server-side decorations | WM | `xdg-decoration` is not offered by Mutter: winit draws its own frame when decorated; full screen has none | R: offered | R | V: offered |
| Idle inhibit | `xdg-screensaver suspend` if xdg-utils is installed | `zwp_idle_inhibit_manager_v1`: Mutter implements it (R) | R | R | V: sway reports `inhibit_idle` true while the window is up |
| VRR | Driver and WM setting, not the game's | The experimental `variable-refresh-rate` feature, per monitor | Per output (Display settings) | `misc:vrr` | `output <name> adaptive_sync on` |
| Keyboard layout | xkbcommon via winit; the game takes physical keys (scan codes), so layout does not change what a key does | same | same | same | same |
| Gamepad | The engine's own input path (not this layer) | same | same | same | same |

Notes behind the cells:

- **Output pick.** A Wayland client cannot place its window; the compositor does. With no `NV_MONITOR` the layer asks for
  none and logs where the window landed (`fork-window: the window is on monitor ...`, kept for the next run). Naming an
  output (`NV_MONITOR=DP-1`, or the index) asks for that output in the full screen request.
- **Hyprland and sway** can say which output has the focus: `scripts/fork/desktop-hint.sh` (below) does, so the launcher sets
  `NV_MONITOR` from it. Mutter and KWin have no such query; there the compositor's own placement is the better answer and
  the launcher leaves `NV_MONITOR` alone.
- **Scale.** winit 0.30 uses `wp_fractional_scale_v1` and `wp_viewporter` itself: `inner_size()` is physical pixels and the
  surface's destination size is the logical one, so the swap chain made at `inner_size()` is shown 1:1. The layer needs
  nothing more; the log line and the smoke make a regression visible.
- **Idle inhibit.** winit has none, so `platform/idle.rs` binds `zwp_idle_inhibit_manager_v1` on the window's own
  connection (a queue of its own, `wayland-client` and `wayland-protocols` at the versions winit uses). The inhibitor lives
  as long as the window; compositors honour it while the surface is visible. On X11 it runs `xdg-screensaver suspend <id>`
  (and `resume` at the end); without xdg-utils, nothing. `NV_IDLE_INHIBIT=0` turns it off. `org.freedesktop.ScreenSaver`
  over D-Bus is not used (it needs a D-Bus client in the tree for one call; `xdg-screensaver` does it per desktop).
- **A window that stops getting frame callbacks** (GNOME stops them for a window the compositor thinks hidden, and a stalled
  game stops committing). The window runs on its own thread with `ControlFlow::Wait`: it sleeps, never spins, and still
  answers the compositor's `xdg_wm_base` pings, so GNOME does not call the game unresponsive because of the window. The
  present loop (`d3d9-wgpu`) backs off 100 ms doubling to 2 s while the surface gives no frame. If the game itself is stuck
  and never takes a close request, a second close within the same window after 5 s ends the process.

## Launcher: `scripts/fork/desktop-hint.sh`

`nv-rs-run` (owner-local) can source it and call `nv_desktop_export`. It detects the desktop from
`HYPRLAND_INSTANCE_SIGNATURE`, `SWAYSOCK`, `XDG_CURRENT_DESKTOP` and `XDG_SESSION_TYPE` and sets `NV_MONITOR` only when it is
unset or empty. Hyprland: `hyprctl -j monitors` (focused). sway: `swaymsg -t get_outputs -r` (focused). X11: the output under
the pointer (`xdotool`, `xrandr`), else the primary. KDE (`kscreen-doctor -o`) and GNOME (Mutter DisplayConfig over `gdbus`)
give only the primary, so nothing is exported there unless `NV_DESKTOP_HINT_PRIMARY=1`. Every branch is silent when its tool is
missing. Tests: `scripts/fork/tests/test_desktop_hint.py` (stub tools). Run by hand: `scripts/fork/desktop-hint.sh`.

## CI

`Window smoke (desktops)` (`.github/workflows/fork-window-desktops.yml`, optional, not part of Pipeline Complete) runs
`scripts/fork/headless-smoke.sh` under headless weston, cage, sway and labwc with a software renderer and lavapipe: a
full screen window, 30 frames, the swap chain the output's size in physical pixels. On sway it also checks the idle inhibitor
is seen by the compositor, and once more at scale 1.5. weston offers no idle inhibit manager (the log says `none` there); labwc, sway and cage's wlroots do. `Window smoke` runs the X11 case under Xvfb.

Documented gaps, not run in CI:

- **GNOME (Mutter)** and **KDE (KWin)**: no headless mode that runs without a GPU session on a hosted runner.
  `mutter --headless --wayland` and `kwin_wayland --virtual` exist; wiring them in is a follow-up.
- **Hyprland**: packaged in Nix and tried headless; it did not start without a seat and a render node (aquamarine could not
  create its backend), so it is unverified.
- **Pointer constraints and relative motion** need synthetic input; none of the headless runs sends any.

## What to try by hand

GNOME Wayland (owner): start the game; the log line should read `on wayland`, `pointer locked`, `idle inhibit wayland`, and
the window should be on the monitor with the pointer. Leave it idle for longer than the blank timeout: the screen must not
blank (`gsettings get org.gnome.desktop.session idle-delay`; set a short one to test). Alt-Tab away and back: the pointer is
released and held again. On Hyprland: the same, plus `hyprctl monitors` and a window on the focused monitor
(`desktop-hint.sh` should name it).
