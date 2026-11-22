# shellcheck shell=bash
# Shared by nv-rs-dev. Sourced, not run.
DEV_TARGET="${NV_DEV_TARGET:-$HOME/.cache/nv-rs/target}"
DEV_MEM="${NV_DEV_MEM:-12G}"
DEV_PROFILE="${NV_DEV_PROFILE:-play}"
DEV_LOGS="$HOME/.local/state/nv-rs/logs"
DEV_REPO="${NV_DEV_REPO:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
DEV_CAPPED="${CAPPED:-$HOME/.local/bin/capped}"

# One cargo process, memory-capped, shared target dir, run from the repo.
dev_cargo() {
  [ -f "$DEV_REPO/Cargo.toml" ] || { echo "not in an nv-rs checkout (set NV_DEV_REPO)" >&2; return 2; }
  mkdir -p "$DEV_TARGET"
  # The linker choice is the toolchain's default here; set NV_DEV_RUSTFLAGS (encoded, 0x1f separated) to override.
  ( cd "$DEV_REPO" && CARGO_TARGET_DIR="$DEV_TARGET" ${NV_DEV_RUSTFLAGS:+CARGO_ENCODED_RUSTFLAGS="$NV_DEV_RUSTFLAGS"} \
      "$DEV_CAPPED" "$DEV_MEM" cargo "$@" )
}

dev_viewer() { echo "$DEV_TARGET/$DEV_PROFILE/nv-viewer"; }

dev_build() {
  local t0=$SECONDS rc kind=warm args=() a
  for a in "$@"; do
    if [ "$a" = --clean ]; then kind=cold
    elif [ "$a" = --opt ]; then DEV_PROFILE=play-opt   # the fully optimised build the numbers are measured on
    else args+=("$a"); fi
  done
  if [ "$kind" = cold ]; then
    # A reproducible cold build in its own fresh target dir (the shared one may be in use by another worker, so it is
    # never emptied here; `nv-rs-dev clean` removes the cold dir), with no compiler cache in the way.
    DEV_TARGET="$DEV_TARGET-cold"
    case "$DEV_TARGET" in
      "$HOME"/.cache/nv-rs/*|"$HOME"/.local/*|/tmp/*) rm -rf "$DEV_TARGET";;
      *) echo "refusing --clean: $DEV_TARGET is outside the loop's cache folders" >&2; return 2;;
    esac
    export RUSTC_WRAPPER=
  fi
  dev_cargo build --profile "$DEV_PROFILE" -p nv-viewer --bin nv-viewer "${args[@]}"; rc=$?
  echo "# nv-rs-dev build: rc=$rc, $kind, $((SECONDS - t0)) s, $(dev_viewer)"
  mkdir -p "$HOME/.local/state/nv-rs"
  printf '%s\t%s\t%s\t%s\t%s\n' "$(date +%Y-%m-%dT%H:%M:%S)" "$(cd "$DEV_REPO" && git rev-parse --short HEAD 2>/dev/null)" "$kind" "$((SECONDS - t0))" "$rc" \
    >> "$HOME/.local/state/nv-rs/build-times.tsv"
  return $rc
}

# The run environment, as a local wrapper sets it (game folder, wgpu, Nix-store fallbacks, sound).
dev_env() {
  GAME="${NV_GAME:-$HOME/Games/SteamLibrary/steamapps/common/Fallout New Vegas}"
  export NV_D3D9="${NV_D3D9:-wgpu}"
  if ! ldconfig -p 2>/dev/null | grep -q libwayland-client; then
    local d
    for d in /nix/store/*-wayland-1.*/lib /nix/store/*-libxkbcommon-*/lib /nix/store/*-vulkan-loader-*/lib; do
      case "$d" in *-dev/*|*-bin/*) continue;; esac
      [ -d "$d" ] && LD_LIBRARY_PATH="${LD_LIBRARY_PATH:+$LD_LIBRARY_PATH:}$d"
    done
    export LD_LIBRARY_PATH
  fi
  export NV_SOUND="${NV_SOUND:-out}"
  # Scripted and local-loop runs keep their saves in a scratch folder, never in the maintainer's userdata
  # (a real run through a local wrapper keeps the default: saves persist there); the caller may set it.
  export NV_SAVE_OUT="${NV_SAVE_OUT:-$HOME/.local/state/nv-rs/bench/saves-$(date +%Y%m%dT%H%M%S)-$$}"
  if [ -z "${NV_PULSE_SIMPLE:-}" ] && ! ldconfig -p 2>/dev/null | grep -q libpulse-simple; then
    local f
    for f in /nix/store/*-libpulseaudio-*/lib/libpulse-simple.so.0; do [ -e "$f" ] && NV_PULSE_SIMPLE="$f" && break; done
    export NV_PULSE_SIMPLE
  fi
}

# dev_run_viewer <seconds|0> [viewer args]: logs to $LOG; seconds>0 stops it with SIGTERM after that long.
dev_run_viewer() {
  local secs="$1"; shift
  local v; v="$(dev_viewer)"
  [ -x "$v" ] || { echo "no build at $v: run nv-rs-dev build" >&2; return 2; }
  dev_env; mkdir -p "$DEV_LOGS"
  local T; T="$(date +%Y%m%dT%H%M%S)"; LOG="$DEV_LOGS/dev-$T.log"
  {
    echo "# nv-rs-dev $T"
    echo "# build: $v ($(cd "$DEV_REPO" && git rev-parse --short HEAD 2>/dev/null))"
    echo "# game:  $GAME"
    echo "# host:  $(uname -srm); session: ${XDG_SESSION_TYPE:-unknown}; display: ${WAYLAND_DISPLAY:-${DISPLAY:-none}}"
    echo "# NV_D3D9=$NV_D3D9 NV_SOUND=$NV_SOUND NV_PULSE_SIMPLE=${NV_PULSE_SIMPLE:-} LD_LIBRARY_PATH=${LD_LIBRARY_PATH:-}"
  } > "$LOG"
  local pre=(); [ "$secs" -gt 0 ] && pre=(timeout -s TERM -k 10 "$secs")
  RUST_BACKTRACE=1 RUST_LOG="${RUST_LOG:-info}" "${pre[@]}" "$v" "$GAME" --movement-missing "$DEV_LOGS/missing-$T.tsv" "$@" >> "$LOG" 2>&1 &
  VIEWER_PID=$!
  if [ "$secs" -eq 0 ]; then tail -f --pid=$VIEWER_PID "$LOG" & fi
  wait $VIEWER_PID; RC=$?
  echo "# exit status: $RC" >> "$LOG"
  return 0
}

dev_run() { dev_run_viewer 0 "$@"; echo "log: $LOG (exit $RC)"; exit "$RC"; }

dev_smoke() {
  local secs=60 shot=0
  while [ $# -gt 0 ]; do case "$1" in
    --seconds) secs="$2"; shift 2;; --shot) shot=1; shift;; *) echo "unknown option $1" >&2; return 2;; esac; done
  local v; v="$(dev_viewer)"; [ -x "$v" ] || { echo "no build at $v: run nv-rs-dev build" >&2; return 2; }
  if [ "$shot" = 1 ]; then
    ( sleep $((secs - 5)); mkdir -p "$HOME/.local/state/nv-rs/shots"
      f="$HOME/.local/state/nv-rs/shots/smoke-$(date +%Y%m%dT%H%M%S).png"
      if command -v grim >/dev/null; then grim "$f"; elif command -v spectacle >/dev/null; then spectacle -b -n -o "$f"; fi ) &
  fi
  dev_run_viewer "$secs"
  local tool="$DEV_TARGET/debug/nv-rs-scene"
  [ -x "$tool" ] || { echo "no $tool: run 'nv-rs-dev scene build' first" >&2; return 2; }
  "$tool" smoke-report "$LOG" "$RC" "$secs"
}

dev_check() {
  local crates=("$@")
  if [ ${#crates[@]} -eq 0 ]; then
    mapfile -t crates < <(cd "$DEV_REPO" && git diff --name-only origin/fork...HEAD 2>/dev/null; cd "$DEV_REPO" && git diff --name-only HEAD 2>/dev/null)
    mapfile -t crates < <(printf '%s\n' "${crates[@]}" | sed -n 's#^\(crates\|vendor\)/\([^/]*\)/.*#\2#p; s#^viewer/.*#nv-viewer#p' | sort -u)
  fi
  [ ${#crates[@]} -gt 0 ] || { echo "no affected crates"; return 0; }
  local pk=() c; for c in "${crates[@]}"; do pk+=(-p "$c"); done
  echo "# crates: ${crates[*]}"
  dev_cargo fmt "${pk[@]}" -- --check || return 1
  dev_cargo clippy "${pk[@]}" --all-targets -- -D warnings
}

# What a diff reaches, by the same map the pull request's CI uses (.fork/components.toml, scripts/fork/components.py).
dev_affected() {
  ( cd "$DEV_REPO" && python3 -I scripts/fork/components.py affected --base "${1:-origin/fork}" )
}

# The pull request's selection, run here: fmt, clippy of the changed crates, then the Linux commands of the affected
# components (Windows-only components, the typed integration jobs among them, are left to CI). Stops at the first failure.
dev_ci_lite() {
  local base="${1:-origin/fork}" t0=$SECONDS line cmds
  dev_affected "$base" || return 1
  dev_cargo fmt --all -- --check || return 1
  local out; out="$(cd "$DEV_REPO" && python3 -I scripts/fork/components.py affected --base "$base" --commands)" \
    || { echo "# ci-lite: could not compute the selection against $base (fetch it, or pass another base)" >&2; return 1; }
  mapfile -t cmds <<<"$out"
  for line in "${cmds[@]}"; do
    [ -n "$line" ] || continue
    echo "# ci-lite: $line"
    local -a words; read -r -a words <<<"$line"
    [ "${words[0]}" = cargo ] || { echo "skipped (not cargo)"; continue; }
    dev_cargo "${words[@]:1}" || { echo "# ci-lite: FAILED after $((SECONDS - t0)) s"; return 1; }
  done
  echo "# ci-lite: ok, $((SECONDS - t0)) s"
}
