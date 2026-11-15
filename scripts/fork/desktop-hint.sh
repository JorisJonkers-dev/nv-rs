#!/usr/bin/env bash
# Which desktop is this, and which output has the focus (docs/fork/DESKTOPS.md).
#
# Source it from a launcher (nv-rs-run does):
#     . /path/to/scripts/fork/desktop-hint.sh
#     nv_desktop_export            # sets and exports NV_MONITOR only when it is unset or empty
# or run it:  desktop-hint.sh [kind | output | export]      (default: both lines, `kind` and `output`)
#
# Every branch needs a command line tool of its compositor and degrades silently without it: no
# output, no error, no NV_MONITOR. The tools are run with a short timeout so a wedged compositor
# cannot hold the launch. Nothing is read from or written to a game folder.
#
# What each desktop can tell:
#   Hyprland  the monitor with the focus (hyprctl -j monitors, "focused")
#   sway      the output with the focus (swaymsg -t get_outputs, "focused")
#   X11       the output under the pointer (xdotool + xrandr), else the primary
#   KDE       the primary output (kscreen-doctor -o, "priority 1"): KWin has no query for the focus
#   GNOME     the primary monitor (Mutter DisplayConfig over gdbus): Mutter has no query for the focus
# For KDE and GNOME the compositor already puts a full screen window on the current output, which is
# better than the primary, so `nv_desktop_export` leaves NV_MONITOR alone there unless
# NV_DESKTOP_HINT_PRIMARY=1 asks for the primary.

_nv_have() { command -v "$1" >/dev/null 2>&1; }

# Runs a command for at most 3 s, stderr dropped; just runs it where there is no `timeout`.
_nv_run() {
  if _nv_have timeout; then timeout 3 "$@" 2>/dev/null; else "$@" 2>/dev/null; fi
}

# hyprland, sway, kde, gnome, x11, or nothing.
nv_desktop_kind() {
  local desk="${XDG_CURRENT_DESKTOP:-}"
  desk="$(printf '%s' "$desk" | tr '[:upper:]' '[:lower:]')"
  if [ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ]; then echo hyprland
  elif [ -n "${SWAYSOCK:-}" ]; then echo sway
  else
    case "$desk" in
      *hyprland*) echo hyprland; return;;
      *sway*) echo sway; return;;
      *kde*|*plasma*) echo kde; return;;
      *gnome*|*ubuntu*|*unity*) echo gnome; return;;
    esac
    if [ "${XDG_SESSION_TYPE:-}" = x11 ] || { [ -z "${WAYLAND_DISPLAY:-}" ] && [ -n "${DISPLAY:-}" ]; }; then
      echo x11
    fi
  fi
}

# The name in the first object of a JSON array where `focused` is true.
_nv_json_focused() {
  if _nv_have python3; then
    python3 -I -c '
import json, sys
try:
    for o in json.load(sys.stdin):
        if o.get("focused"):
            print(o.get("name", "")); break
except Exception:
    pass'
  elif _nv_have jq; then
    jq -r 'map(select(.focused))[0].name // empty'
  fi
}

_nv_out_hyprland() { _nv_have hyprctl && _nv_run hyprctl -j monitors | _nv_json_focused; }
_nv_out_sway() { _nv_have swaymsg && _nv_run swaymsg -t get_outputs -r | _nv_json_focused; }

# `Output: 1 DP-1 enabled connected priority 1 ...` : the one with priority 1, else the first enabled.
_nv_out_kde() {
  _nv_have kscreen-doctor || return 0
  _nv_run kscreen-doctor -o | sed 's/\x1b\[[0-9;]*m//g' | awk '
    $1 == "Output:" && $0 ~ /enabled/ {
      if (first == "") first = $3
      if ($0 ~ /priority 1( |$)/ && prim == "") prim = $3
    }
    END { if (prim != "") print prim; else if (first != "") print first }'
}

# GetCurrentState prints the logical monitors as (x, y, scale, transform, primary, [(connector, ...)], ...).
_nv_out_gnome() {
  _nv_have gdbus || return 0
  _nv_run gdbus call --session --dest org.gnome.Mutter.DisplayConfig \
    --object-path /org/gnome/Mutter/DisplayConfig \
    --method org.gnome.Mutter.DisplayConfig.GetCurrentState |
    sed -n "s/.*, true, \[('\([^']*\)'.*/\1/p" | head -n 1
}

# The output holding the pointer (xdotool + xrandr), else xrandr's primary.
_nv_out_x11() {
  _nv_have xrandr || return 0
  local q px="" py=""
  q="$(_nv_run xrandr --query)" || return 0
  if _nv_have xdotool; then
    eval "$(_nv_run xdotool getmouselocation --shell | grep -E '^(X|Y)=[0-9-]+$' | sed 's/^X=/px=/;s/^Y=/py=/')"
  fi
  printf '%s\n' "$q" | awk -v px="$px" -v py="$py" '
    $2 == "connected" {
      geo = ""
      for (i = 3; i <= NF; i++) if ($i ~ /^[0-9]+x[0-9]+\+-?[0-9]+\+-?[0-9]+$/) { geo = $i; break }
      if (geo == "") next
      if (prim == "" && $3 == "primary") prim = $1
      if (first == "") first = $1
      split(geo, a, /[x+]/)
      if (px != "" && py != "" && px >= a[3] && px < a[3] + a[1] && py >= a[4] && py < a[4] + a[2] && hit == "") hit = $1
    }
    END { if (hit != "") print hit; else if (prim != "") print prim; else print first }'
}

# The output name of the focused (or, for KDE, GNOME and X11 without a pointer, primary) output.
nv_focused_output() {
  local out=""
  case "$(nv_desktop_kind)" in
    hyprland) out="$(_nv_out_hyprland)";;
    sway) out="$(_nv_out_sway)";;
    kde) out="$(_nv_out_kde)";;
    gnome) out="$(_nv_out_gnome)";;
    x11) out="$(_nv_out_x11)";;
  esac
  [ -n "$out" ] && printf '%s\n' "$out"
  return 0
}

# Exports NV_MONITOR only where it is unset or empty and the desktop can say (see the header).
nv_desktop_export() {
  [ -n "${NV_MONITOR:-}" ] && return 0
  local kind out
  kind="$(nv_desktop_kind)"
  case "$kind" in
    kde|gnome) [ "${NV_DESKTOP_HINT_PRIMARY:-0}" = 1 ] || return 0;;
  esac
  out="$(nv_focused_output)"
  if [ -n "$out" ]; then
    NV_MONITOR="$out"
    export NV_MONITOR
  fi
  return 0
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  case "${1:-}" in
    kind) nv_desktop_kind;;
    output) nv_focused_output;;
    export) nv_desktop_export; if [ -n "${NV_MONITOR:-}" ]; then echo "NV_MONITOR=$NV_MONITOR"; fi;;
    "") echo "kind: $(nv_desktop_kind)"; echo "output: $(nv_focused_output)";;
    *) echo "usage: $0 [kind|output|export]" >&2; exit 2;;
  esac
fi
