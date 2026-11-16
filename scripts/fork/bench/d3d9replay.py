#!/usr/bin/env python3
"""Fork-only. `nv-rs-dev replay`: the replay-bench of the recorded Direct3D 9 stream (docs/fork/REPLAY.md).

  nv-rs-dev replay build                 build d3d9-replay-bench (release) into the loop's target dir
  nv-rs-dev replay streams               list the streams kept under ~/.local/share/nv-rs-research/streams/
  nv-rs-dev replay info|pack|synth ...   the binary's own commands (no measurement, no lock)
  nv-rs-dev replay run STREAM [args]     measure: takes the bench lock (the one `bench run` takes), then runs the binary

STREAM is a path or the name of a stream in the streams folder (`doc-room` is `doc-room.nvrs` there). The streams are
recordings of the game (`NV_D3D9_RECORD`, cut by `pack`): private, they stay on this machine and never go into a
repository. A run waits for the bench lock like `bench run` does (`--wait-timeout SECONDS` gives up with status 3,
`--no-lock` skips it for a run that gives no numbers); the lock holder is named in the waiting message. If another
nv-viewer ran during the measurement the line `NOISY` is printed.
"""
import os
import subprocess
import sys
import threading
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import lock  # noqa: E402

STREAMS = Path(os.environ.get("NV_REPLAY_STREAMS", Path.home() / ".local/share/nv-rs-research/streams"))
BINARY = "d3d9-replay-bench"


def resolve(name, streams=STREAMS):
    """The stream a name stands for: the path itself when it exists, else `<name>.nvrs` in the streams folder."""
    p = Path(name)
    if p.exists():
        return p
    for cand in (streams / name, streams / f"{name}.nvrs"):
        if cand.exists():
            return cand
    return None


def binary(target=None):
    base = Path(target or os.environ.get("NV_DEV_TARGET", Path.home() / ".cache/nv-rs/target"))
    return base / "release" / BINARY


def listing(streams=STREAMS):
    return sorted(p for p in streams.glob("*.nvrs")) if streams.is_dir() else []


def run_locked(argv, wait_timeout=None, use_lock=True, exe=None, other=lock.other_viewers, out=print):
    """Runs the binary with `argv` under the bench lock; the exit status."""
    held = lock.BenchLock(wait_timeout=wait_timeout, out=out)
    if use_lock and not held.acquire():
        out("bench lock not free in time")
        return 3
    try:
        noisy = {"n": 0}
        stop = threading.Event()

        def watch():
            while not stop.wait(5.0):
                noisy["n"] = max(noisy["n"], other())

        t = threading.Thread(target=watch, daemon=True)
        t.start()
        noisy["n"] = other()
        rc = subprocess.call([str(exe or binary()), *argv])
        stop.set()
        if noisy["n"]:
            out(f"NOISY: {noisy['n']} other nv-viewer process(es) ran during the measurement")
        return rc
    finally:
        held.release()


def main(argv):
    if not argv or argv[0] in ("-h", "--help"):
        print(__doc__)
        return 0
    cmd, rest = argv[0], argv[1:]
    if cmd == "streams":
        for p in listing():
            print(f"{p.name}\t{p.stat().st_size} bytes")
        return 0
    if cmd in ("info", "pack", "synth", "run"):
        wait, use_lock, args, i = None, True, [], 0
        while i < len(rest):
            a = rest[i]
            if cmd == "run" and a == "--wait-timeout":
                wait, i = float(rest[i + 1]), i + 1
            elif cmd == "run" and a == "--no-lock":
                use_lock = False
            else:
                args.append(a)
            i += 1
        if cmd in ("info", "run") and args:
            found = resolve(args[0])
            if not found:
                print(f"no stream {args[0]!r} (path, or a name in {STREAMS})", file=sys.stderr)
                return 2
            args[0] = str(found)
        if not binary().exists():
            print(f"no {binary()}: run `nv-rs-dev replay build`", file=sys.stderr)
            return 2
        if cmd == "run":
            return run_locked([cmd, *args], wait, use_lock)
        return subprocess.call([str(binary()), cmd, *args])
    print(f"unknown command {cmd}\n{__doc__}", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
