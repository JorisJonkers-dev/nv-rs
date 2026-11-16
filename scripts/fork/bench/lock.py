"""A machine-wide exclusive lock for bench runs, and a scan for other viewers (docs/fork/BENCH.md).

Frame-time numbers are only valid when nothing else uses the CPU and the card, so one `nv-rs-dev bench run` holds
the lock for its whole run (the nested compositor session included) and the others wait. The lock is a flock on a
file, so it goes away with its holder, however it dies. The file names the holder (pid, command, start) for the
waiting runs' message.
"""
import fcntl
import json
import os
import sys
import time
from pathlib import Path


def lock_path():
    env = os.environ.get("NV_BENCH_LOCK")
    if env:
        return Path(env)
    return Path.home() / ".cache/nv-rs/bench.lock"


def describe(info, now=None):
    now = now or time.time()
    try:
        age = int(now - float(info["start"]))
    except (KeyError, TypeError, ValueError):
        age = -1
    return f"pid {info.get('pid', '?')} ({info.get('cmd', '?')}), {age} s ago" if age >= 0 else f"pid {info.get('pid', '?')}"


class BenchLock:
    def __init__(self, path=None, wait_timeout=None, out=print, poll=2.0):
        self.path, self.timeout, self.out, self.poll = Path(path or lock_path()), wait_timeout, out, poll
        self.fd = None

    def read_holder(self):
        try:
            return json.loads(self.path.read_text() or "{}")
        except (OSError, ValueError):
            return {}

    def acquire(self):
        """True when held; False when the wait timed out. Prints who holds it every 30 s."""
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.fd = os.open(self.path, os.O_RDWR | os.O_CREAT, 0o644)
        start = time.monotonic()
        said = None
        while True:
            try:
                fcntl.flock(self.fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except OSError:
                waited = time.monotonic() - start
                if self.timeout is not None and waited >= self.timeout:
                    os.close(self.fd)
                    self.fd = None
                    return False
                if said is None or waited - said >= 30:
                    self.out(f"waiting for bench lock held by {describe(self.read_holder())} (waited {int(waited)} s)")
                    said = waited
                time.sleep(self.poll)
        os.ftruncate(self.fd, 0)
        os.write(self.fd, json.dumps({"pid": os.getpid(), "cmd": " ".join(sys.argv[1:4]), "start": time.time()}).encode())
        return True

    def release(self):
        if self.fd is not None:
            try:
                os.ftruncate(self.fd, 0)
                fcntl.flock(self.fd, fcntl.LOCK_UN)
            finally:
                os.close(self.fd)
                self.fd = None


def other_viewers(own_pids=(), proc="/proc"):
    """The number of nv-viewer processes that are not `own_pids` (or their children): other runs on the machine
    whose load disturbs the numbers."""
    own = {int(p) for p in own_pids}
    n = 0
    for d in Path(proc).iterdir():
        if not d.name.isdigit() or int(d.name) in own:
            continue
        try:
            comm = (d / "comm").read_text().strip()
            if comm.startswith("nv-viewer") or comm == "nv-viewer":
                ppid = int((d / "stat").read_text().rsplit(")", 1)[1].split()[1])
                if ppid in own:
                    continue
                n += 1
        except (OSError, ValueError, IndexError):
            continue
    return n
