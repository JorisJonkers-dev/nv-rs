"""Finding the optional tools of watch, record and replay (wf-recorder, grim, wayvnc, ffmpeg, mpv), with an install hint."""
import os
import shutil

HINT = "nix shell nixpkgs#wayvnc nixpkgs#wf-recorder nixpkgs#grim nixpkgs#ffmpeg nixpkgs#mpv"


def find(name):
    """Path of a tool: NV_BENCH_<NAME> first (name upper-cased, dashes to underscores), then PATH; None when absent."""
    env = os.environ.get("NV_BENCH_" + name.upper().replace("-", "_"))
    if env and os.access(env, os.X_OK):
        return env
    return shutil.which(name)


def need(name, why):
    """Path of a tool or SystemExit with the install hint."""
    p = find(name)
    if not p:
        raise SystemExit(f"{name} is not installed (needed to {why}): {HINT}")
    return p
