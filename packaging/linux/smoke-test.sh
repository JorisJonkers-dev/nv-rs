#!/bin/sh
# Runs the built artifact: nv-viewer --check on folders that are not a game (no game data is needed or read).
#   smoke-test.sh <tree>    <tree> holds bin/nv-viewer and bin/nv-rs-launch (an unpacked tarball, a Nix result, an install prefix)
# What it proves: the binary starts on a clean machine (its libraries resolve), parses its command line, and --check
# refuses a missing, empty and wrong installation with exit status 1 and a message that names the problem, without
# crashing. The success path (exit 0) needs the player's own FalloutNV.exe and is not run here.
set -eu
tree="${1:?usage: smoke-test.sh <tree>}"
viewer="$tree/bin/nv-viewer"
launch="$tree/bin/nv-rs-launch"
[ -x "$viewer" ] || { echo "smoke: $viewer is not an executable" >&2; exit 1; }
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
fail=0
out="$work/out"

# expect <name> <status> <text the output must hold> -- <command...>
expect() {
    name="$1"; want="$2"; text="$3"; shift 4
    set +e
    "$@" >"$out" 2>&1
    got=$?
    set -e
    if [ "$got" -ne "$want" ]; then
        echo "FAIL $name: exit $got, wanted $want"; sed 's/^/    /' "$out"; fail=1
    elif ! grep -qF -- "$text" "$out"; then
        echo "FAIL $name: output lacks '$text'"; sed 's/^/    /' "$out"; fail=1
    else
        echo "ok   $name"
    fi
}

if command -v ldd >/dev/null 2>&1; then
    if ldd "$viewer" 2>&1 | grep -q 'not found'; then
        echo "FAIL libraries: $(ldd "$viewer" 2>&1 | grep 'not found')"; fail=1
    else
        echo "ok   libraries resolve"
    fi
fi

expect "help" 0 "nv-viewer - run Fallout" -- "$viewer" --help
expect "no arguments" 2 "the game's Data folder is missing" -- "$viewer"

mkdir -p "$work/empty"
expect "check: not an installation" 1 "FalloutNV.esm" -- "$viewer" "$work/empty" --check

# Empty stand-ins for the master file; they are not game data.
mkdir -p "$work/noexe/Data"
: >"$work/noexe/Data/FalloutNV.esm"
expect "check: no FalloutNV.exe" 1 "FalloutNV.exe: not found" -- "$viewer" "$work/noexe/Data" --check

mkdir -p "$work/wrong/Data"
: >"$work/wrong/Data/FalloutNV.esm"
printf 'MZ but not a PE file' >"$work/wrong/FalloutNV.exe"
expect "check: wrong executable" 1 "1.4.0.525" -- "$viewer" "$work/wrong" --check

if [ -x "$launch" ]; then
    expect "launcher: help" 0 "usage: nv-rs-launch" -- "$launch" --help
    expect "launcher: no folder" 2 "no data folder" -- env -u NV_RS_DATA XDG_CONFIG_HOME="$work/config" "$launch" --check
    expect "launcher: folder from the environment" 1 "FalloutNV.exe: not found" -- env NV_RS_DATA="$work/noexe" "$launch" --check
    mkdir -p "$work/config"
    expect "launcher: save a folder" 0 "Saved:" -- env XDG_CONFIG_HOME="$work/config" "$launch" --set-data "$work/wrong"
    expect "launcher: saved folder" 1 "1.4.0.525" -- env -u NV_RS_DATA XDG_CONFIG_HOME="$work/config" "$launch" --check
fi

if [ "$fail" -ne 0 ]; then echo "smoke test FAILED"; exit 1; fi
echo "smoke test passed"
