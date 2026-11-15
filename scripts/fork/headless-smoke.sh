#!/usr/bin/env bash
# Runs the window layer's smoke (the `clear` example, --smoke) inside a headless compositor, so a
# desktop's protocol path is exercised without a screen (docs/fork/DESKTOPS.md, "CI").
#
#   headless-smoke.sh <weston|cage|sway|labwc|x11> [path/to/clear]
#
# Needs the compositor on PATH, a Vulkan driver (lavapipe: VK_ICD_FILENAMES) and libwayland / libxkbcommon.
# Exits 0 when the example shows its frames; 77 (skipped) when the compositor is not installed;
# anything else is a failure. Prints what ran so the log says which backend was exercised.
set -u
which_c="${1:-}"
clear_bin="${2:-target/debug/examples/clear}"
[ -x "$clear_bin" ] || { echo "no example at $clear_bin" >&2; exit 2; }
case "$which_c" in
  weston|cage|sway|labwc) command -v "$which_c" >/dev/null 2>&1 || { echo "skip: $which_c is not installed"; exit 77; };;
  x11) command -v Xvfb >/dev/null 2>&1 || { echo "skip: Xvfb is not installed"; exit 77; };;
  *) echo "usage: $0 <weston|cage|sway|labwc|x11> [clear]" >&2; exit 2;;
esac

dir="$(mktemp -d)"
export XDG_RUNTIME_DIR="$dir"
chmod 700 "$dir"
pids=()
cleanup() {
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null; done
  wait 2>/dev/null
  rm -rf "$dir"
}
trap cleanup EXIT

# wlroots compositors: no seat, no GPU, a software renderer.
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman
export WLR_HEADLESS_OUTPUTS=1
unset DISPLAY WAYLAND_DISPLAY SWAYSOCK HYPRLAND_INSTANCE_SIGNATURE WINIT_UNIX_BACKEND

wait_socket() {
  for _ in $(seq 1 100); do
    [ -S "$dir/$1" ] && return 0
    sleep 0.1
  done
  echo "the compositor made no socket $1" >&2
  return 1
}

out="$dir/smoke.txt"
case "$which_c" in
  weston)
    weston --backend=headless --renderer=pixman --socket=nvsmoke --width=1280 --height=720 >"$dir/comp.log" 2>&1 &
    pids+=($!)
    wait_socket nvsmoke || { cat "$dir/comp.log"; exit 1; }
    export WAYLAND_DISPLAY=nvsmoke XDG_CURRENT_DESKTOP=weston XDG_SESSION_TYPE=wayland
    ;;
  sway|labwc)
    if [ "$which_c" = sway ]; then
      printf 'output * resolution 1280x720\n' >"$dir/config"
      sway --unsupported-gpu -c "$dir/config" >"$dir/comp.log" 2>&1 &
    else
      labwc >"$dir/comp.log" 2>&1 &
    fi
    pids+=($!)
    # sway and labwc take the first free name (wayland-0, wayland-1, ...).
    for _ in $(seq 1 100); do
      sock=""
      for f in "$dir"/wayland-[0-9]*; do
        case "$f" in *.lock) continue;; esac
        [ -S "$f" ] && sock="${f##*/}" && break
      done
      [ -n "$sock" ] && break
      sleep 0.1
    done
    [ -n "${sock:-}" ] || { echo "the compositor made no socket" >&2; cat "$dir/comp.log"; exit 1; }
    export WAYLAND_DISPLAY="$sock" XDG_CURRENT_DESKTOP="$which_c" XDG_SESSION_TYPE=wayland
    if [ "$which_c" = sway ]; then
      for f in "$dir"/sway-ipc.*.sock; do SWAYSOCK="$f"; done
      export SWAYSOCK
    fi
    ;;
  cage)
    # cage runs one client full screen and exits with it.
    export XDG_CURRENT_DESKTOP=cage XDG_SESSION_TYPE=wayland
    cage -- bash -c "'$clear_bin' --smoke" >"$out" 2>"$dir/comp.log"
    rc=$?
    echo "compositor: cage (wlroots, headless)"; cat "$out"
    grep -q 'smoke: 30 frames shown' "$out" || { echo "FAIL (rc=$rc)"; cat "$dir/comp.log"; exit 1; }
    exit 0
    ;;
  x11)
    Xvfb :77 -screen 0 1280x720x24 >"$dir/comp.log" 2>&1 &
    pids+=($!)
    sleep 1
    export DISPLAY=:77 XDG_SESSION_TYPE=x11 WINIT_UNIX_BACKEND=x11
    ;;
esac

echo "compositor: $which_c (display ${WAYLAND_DISPLAY:-$DISPLAY})"
timeout 120 "$clear_bin" --smoke >"$out" 2>&1
rc=$?
cat "$out"
if [ "$rc" -ne 0 ] || ! grep -q 'smoke: 30 frames shown' "$out"; then
  echo "FAIL (rc=$rc)"; cat "$dir/comp.log"; exit 1
fi
