#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""One long-lived build process owns cargo for the fork's parallel workers.

Workers call the client command instead of starting cargo themselves:

    python3 scripts/fork/build_daemon.py run -- test --test typed_fade
    python3 scripts/fork/build_daemon.py status
    python3 scripts/fork/build_daemon.py stop

The first call starts the daemon if needed. The daemon keeps one FIFO queue and one
worker thread, so two requests at once never overlap, and builds the cargo command
itself: two jobs, a memory cap, a shared lock and a shared target directory. The
limits are read from NV_BUILD_* variables at daemon start (docs/fork/LOCAL.md); no
machine's paths are built into this file. sccache is the rustc wrapper when it is
installed, or the path given by NV_BUILD_WRAPPER. The client prints cargo's output
and returns cargo's exit status.

The transport is a loopback socket with a per-daemon token, so it works with the
same code on Windows, Linux and macOS.
"""
import json
import os
import queue
import secrets
import shlex
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

STATE = "build-daemon.json"
# The exact environment cargo, rustc, this project's build scripts and the typed
# test targets read. A worker's other variables (including NV_* secrets) never
# reach a build.
PASS_ENV = frozenset({
    "PATH", "HOME", "USERPROFILE", "LOCALAPPDATA", "APPDATA", "SYSTEMROOT",
    "COMSPEC", "TEMP", "TMP", "TMPDIR", "XDG_RUNTIME_DIR",
    "CARGO_HOME", "RUSTUP_HOME", "RUSTUP_TOOLCHAIN",
    "RUSTC", "RUSTDOC", "RUSTFLAGS", "RUSTDOCFLAGS", "RUST_BACKTRACE",
    "CC", "CXX", "AR", "LIBCLANG_PATH", "PKG_CONFIG_PATH", "OPENSSL_DIR",
    "SSL_CERT_FILE", "SSL_CERT_DIR",
    "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY",
    "http_proxy", "https_proxy", "all_proxy", "no_proxy",
    "NV_CALL_RESERVE_MB",  # read by research/nv-oracle/nv-call-engine/build.rs
    # Read by the typed test targets the daemon runs (crates/engine/tests/typed_*).
    "NV_DATA", "NV_SCRIPTS", "NV_EXPRESSIONS",
    "NV_ACCEPTANCE_FRAME", "NV_ACCEPTANCE_KEPT", "NV_ACCEPTANCE_OBJECTS",
    "NV_ACCEPTANCE_OUT", "NV_ACCEPTANCE_REPORT", "NV_ACCEPTANCE_SET",
    "NV_ACCEPTANCE_WHO",
})


def expand(value):
    return os.path.expandvars(os.path.expanduser(str(value)))


def prefix(value):
    """A command prefix as shell words, with ~ and environment variables expanded."""
    return [expand(part) for part in shlex.split(value, posix=os.name != "nt")]


def runtime_dir(env):
    if env.get("NV_BUILD_DAEMON_DIR"):
        return Path(expand(env["NV_BUILD_DAEMON_DIR"]))
    if os.name == "nt":
        return Path(env.get("LOCALAPPDATA") or tempfile.gettempdir()) / "nv-rs" / "build-daemon"
    return Path(env.get("XDG_RUNTIME_DIR") or tempfile.gettempdir()) / "nv-rs-build-daemon"


def read_state(runtime):
    try:
        data = json.loads((runtime / STATE).read_text(encoding="utf-8"))
        return data if isinstance(data.get("port"), int) and data.get("token") else None
    except (OSError, ValueError, TypeError):
        return None


def write_state(runtime, state):
    tmp = runtime / (STATE + ".tmp")
    tmp.write_text(json.dumps(state), encoding="utf-8")
    if os.name != "nt":
        tmp.chmod(0o600)
    os.replace(tmp, runtime / STATE)


class FileLock:
    """A short-lived lock around the daemon's startup, on POSIX and Windows."""

    def __init__(self, path):
        self.path = Path(path)
        self.handle = None

    def __enter__(self):
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.handle = self.path.open("a+b")
        try:
            if os.name == "nt":
                import msvcrt
                if self.handle.seek(0, os.SEEK_END) == 0:
                    self.handle.write(b"0")
                    self.handle.flush()
                self.handle.seek(0)
                msvcrt.locking(self.handle.fileno(), msvcrt.LK_LOCK, 1)
            else:
                import fcntl
                fcntl.flock(self.handle.fileno(), fcntl.LOCK_EX)
        except Exception:
            self.handle.close()
            raise
        return self

    def __exit__(self, *_):
        if os.name == "nt":
            import msvcrt
            self.handle.seek(0)
            msvcrt.locking(self.handle.fileno(), msvcrt.LK_UNLCK, 1)
        self.handle.close()
        return False


def send_frame(stream, message):
    stream.write(json.dumps(message) + "\n")
    stream.flush()


def recv_frame(stream):
    """One JSON object per line; json escapes the newlines in cargo's output."""
    line = stream.readline()
    return json.loads(line) if line else None


def default_path(env, leaf):
    if os.name == "nt":
        return str(Path(env.get("LOCALAPPDATA") or tempfile.gettempdir()) / leaf)
    return str(Path.home() / ".cache" / leaf)


class Config:
    """The machine's build limits, supplied by the environment, never by a worker."""

    def __init__(self, env):
        self.jobs = env.get("NV_BUILD_JOBS") or "2"
        self.mem = env.get("NV_BUILD_MEM") or "10G"
        self.target_dir = expand(env.get("NV_BUILD_TARGET_DIR")
                                 or default_path(env, "cargo-targets/nv-rs-gw"))
        self.lock = expand(env.get("NV_BUILD_LOCK")
                           or default_path(env, "nv-workers/cargo.lock"))
        Path(self.lock).parent.mkdir(parents=True, exist_ok=True)
        self.wrapper = env.get("NV_BUILD_WRAPPER", "sccache")
        self.wrapper_set = "NV_BUILD_WRAPPER" in env
        self.sccache_dir = expand(env["NV_BUILD_SCCACHE_DIR"]) if env.get("NV_BUILD_SCCACHE_DIR") else ""
        self.cargo = prefix(env.get("NV_BUILD_CARGO") or "cargo")
        self.flock = prefix(env["NV_BUILD_FLOCK"]) if env.get("NV_BUILD_FLOCK") else (
            prefix("flock") if os.name != "nt" and shutil.which("flock") else []
        )
        self.capped = prefix(env["NV_BUILD_CAPPED"]) if env.get("NV_BUILD_CAPPED") else (
            prefix("capped") if shutil.which("capped") else []
        )
        self.warnings = []
        if not self.capped:
            self.warnings.append(
                "build daemon: warning: no memory-cap wrapper found; "
                "set NV_BUILD_CAPPED to its path to cap build memory"
            )
        # Number of parallel build slots (default 3, 1 behaves as before)
        try:
            self.slots = int(env.get("NV_BUILD_SLOTS", "3"))
        except ValueError:
            self.slots = 3
        if self.slots < 1:
            self.slots = 1

    def rustc_wrapper(self):
        """sccache when installed, an explicit override always, nothing when disabled."""
        if not self.wrapper:
            return ""
        if self.wrapper_set or shutil.which(self.wrapper):
            return self.wrapper
        return ""

    def command(self, argv, slot_index=0):
        command = list(self.flock)
        if self.flock:
            if self.slots > 1:
                command.append(self.lock + f".{slot_index}")
            else:
                command.append(self.lock)
        command += self.capped
        if self.capped:
            command.append(self.mem)
        return command + self.cargo + [str(arg) for arg in argv]

    def target_dir_for_slot(self, slot_index):
        if self.slots > 1:
            return self.target_dir + f"-slot{slot_index}"
        return self.target_dir

    def build_env(self, base, passed, slot_index=0):
        env = {key: value for key, value in base.items() if key in PASS_ENV}
        for key, value in passed.items():
            if key in PASS_ENV:
                env[key] = value
        env["CARGO_BUILD_JOBS"] = self.jobs
        env["CARGO_TARGET_DIR"] = self.target_dir_for_slot(slot_index)
        env.pop("RUSTC_WRAPPER", None)
        if self.rustc_wrapper():
            env["RUSTC_WRAPPER"] = self.rustc_wrapper()
        if self.sccache_dir:
            env["SCCACHE_DIR"] = self.sccache_dir
        return env

    def describe(self):
        return {
            "jobs": self.jobs, "mem": self.mem, "target_dir": self.target_dir,
            "lock": self.lock, "wrapper": self.rustc_wrapper(),
            "sccache_dir": self.sccache_dir, "cargo": self.cargo,
            "flock": bool(self.flock), "capped": bool(self.capped),
            "warnings": list(self.warnings),
            "slots": self.slots,
        }


def pass_env(env):
    return {key: value for key, value in env.items() if key in PASS_ENV}


STATS_LOCK = threading.Lock()


def new_stats():
    """The counters the daemon keeps, shared by its slot threads under STATS_LOCK."""
    return {"active": 0, "max_active": 0, "completed": 0, "failed": 0, "served": [],
            "slot_active": {}, "slot_max_active": {}, "job_starts": [], "job_ends": [], "queued": 0}


def handle(stream, req, config, stats, stop, slot_index=0):
    """Run one queued request; a worker thread calls this for its slot."""
    kind = req.get("kind")
    if kind == "status":
        with STATS_LOCK:
            snapshot = json.loads(json.dumps(dict(stats, config=config.describe())))
        send_frame(stream, {"kind": "done", "exit": 0, "status": snapshot})
        return
    if kind == "stop":
        stop.set()
        send_frame(stream, {"kind": "done", "exit": 0})
        return
    label = str(req.get("label") or "client")
    with STATS_LOCK:
        stats["served"] = stats["served"][-99:] + [label]
        stats["active"] += 1
        stats["max_active"] = max(stats["max_active"], stats["active"])
        running = stats["slot_active"].get(slot_index, 0) + 1
        stats["slot_active"][slot_index] = running
        stats["slot_max_active"][slot_index] = max(stats["slot_max_active"].get(slot_index, 0), running)
        stats["job_starts"] = stats["job_starts"][-199:] + [
            {"label": label, "slot": slot_index, "start": time.time()}]
    for warning in config.warnings:
        send_frame(stream, {"kind": "out", "data": warning + "\n"})
    code = 2
    try:
        argv = req.get("argv") or []
        cwd = str(req.get("cwd") or Path.cwd())
        env = config.build_env(os.environ, req.get("env") or {}, slot_index)
        proc = subprocess.Popen(config.command(argv, slot_index), cwd=cwd, env=env,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                text=True, errors="replace", bufsize=1)
        for line in proc.stdout:
            send_frame(stream, {"kind": "out", "data": line})
        code = proc.wait()
    except Exception as exc:
        send_frame(stream, {"kind": "out", "data": f"build daemon: {exc}\n"})
    finally:
        with STATS_LOCK:
            stats["job_ends"] = stats["job_ends"][-199:] + [
                {"label": label, "slot": slot_index, "end": time.time()}]
            stats["active"] -= 1
            stats["slot_active"][slot_index] -= 1
            stats["completed"] += 1
            if code != 0:
                stats["failed"] += 1
        send_frame(stream, {"kind": "done", "exit": code})


def serve():
    env = os.environ
    config = Config(env)
    runtime = runtime_dir(env)
    runtime.mkdir(parents=True, exist_ok=True)
    if os.name != "nt":
        os.chmod(runtime, 0o700)
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", 0))
    listener.listen(16)
    listener.settimeout(0.5)
    state = {"port": listener.getsockname()[1], "token": secrets.token_hex(24),
             "pid": os.getpid()}
    write_state(runtime, state)
    jobs = queue.Queue()
    stop = threading.Event()
    stats = new_stats()

    def reader(conn):
        try:
            conn.settimeout(None)
            stream = conn.makefile("rw", encoding="utf-8", newline="\n")
            req = recv_frame(stream)
            if req is None or req.get("token") != state["token"]:
                if req is not None:
                    send_frame(stream, {"kind": "done", "exit": 2,
                                        "error": "build daemon token rejected"})
                stream.close()
                conn.close()
                return
            with STATS_LOCK:
                stats["queued"] += 1
            jobs.put((conn, stream, req))  # the worker closes both when done
        except (OSError, ValueError):
            conn.close()

    def worker(slot_index):
        while not stop.is_set() or not jobs.empty():
            try:
                conn, stream, req = jobs.get(timeout=0.2)
            except queue.Empty:
                continue
            with STATS_LOCK:
                stats["queued"] -= 1
            try:
                handle(stream, req, config, stats, stop, slot_index)
            except OSError:
                pass
            finally:
                jobs.task_done()
                stream.close()
                conn.close()

    worker_threads = []
    for i in range(config.slots):
        t = threading.Thread(target=worker, args=(i,), daemon=True)
        t.start()
        worker_threads.append(t)
    try:
        while not stop.is_set():
            try:
                conn, _ = listener.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            threading.Thread(target=reader, args=(conn,), daemon=True).start()
    finally:
        listener.close()
        for t in worker_threads:
            t.join(timeout=30)
        (runtime / STATE).unlink(missing_ok=True)


def reachable(state):
    try:
        with socket.create_connection(("127.0.0.1", state["port"]), timeout=1):
            return True
    except OSError:
        return False


def ensure_daemon(env=None):
    """Start one daemon if none answers, under a startup lock shared by the clients."""
    env = os.environ if env is None else env
    runtime = runtime_dir(env)
    runtime.mkdir(parents=True, exist_ok=True)
    if os.name != "nt":
        os.chmod(runtime, 0o700)
    with FileLock(runtime / "startup.lock"):
        state = read_state(runtime)
        if state and reachable(state):
            return state
        (runtime / STATE).unlink(missing_ok=True)
        spawn = {"stdin": subprocess.DEVNULL, "cwd": str(runtime), "env": dict(env)}
        if os.name == "nt":
            spawn["creationflags"] = subprocess.DETACHED_PROCESS
        else:
            spawn["start_new_session"] = True
        with (runtime / "daemon.log").open("ab") as log:
            spawn["stdout"] = log
            spawn["stderr"] = subprocess.STDOUT
            process = subprocess.Popen(
                [sys.executable, str(Path(__file__).resolve()), "serve"], **spawn)
            # The state file owns the detached daemon's lifetime. Mark the local
            # handle as finished so CPython does not warn about an unreaped child.
            process.returncode = 0
        for _ in range(200):
            time.sleep(0.05)
            state = read_state(runtime)
            if state and reachable(state):
                return state
    raise RuntimeError("build daemon did not start; see daemon.log in the runtime directory")


def request(state, message):
    with socket.create_connection(("127.0.0.1", state["port"]), timeout=30) as sock:
        sock.settimeout(None)  # a build may be quiet for longer than the connect timeout
        with sock.makefile("rw", encoding="utf-8", newline="\n") as stream:
            return exchange(stream, message)


def exchange(stream, message):
    send_frame(stream, message)
    while True:
        reply = recv_frame(stream)
        if reply is None:
            raise RuntimeError("build daemon closed the connection")
        if reply.get("kind") == "out":
            print(reply.get("data", ""), end="", flush=True)
        elif reply.get("kind") == "done":
            if reply.get("error"):
                print(reply["error"], file=sys.stderr)
            return reply


def connection(env=None):
    env = os.environ if env is None else env
    state = read_state(runtime_dir(env))
    return state if state and reachable(state) else ensure_daemon(env)


def submit(argv, cwd=None, label=None, env=None):
    """The client entry point: queue one build and return cargo's exit status."""
    env = os.environ if env is None else env
    state = connection(env)
    message = {
        "kind": "build", "token": state["token"], "argv": list(argv),
        "cwd": str(cwd or Path.cwd()),
        "label": str(label or env.get("NV_BUILD_LABEL", "client")),
        "env": pass_env(env),
    }
    return int(request(state, message).get("exit", 1))


def daemon_status(env=None):
    env = os.environ if env is None else env
    state = read_state(runtime_dir(env))
    if not state or not reachable(state):
        return {"running": False}
    return request(state, {"kind": "status", "token": state["token"]})["status"]


def stop_daemon(env=None):
    env = os.environ if env is None else env
    runtime = runtime_dir(env)
    state = read_state(runtime)
    if not state or not reachable(state):
        return False
    request(state, {"kind": "stop", "token": state["token"]})
    for _ in range(100):
        if not (runtime / STATE).exists():
            return True
        time.sleep(0.05)
    return False


def main(argv):
    command = argv[1] if len(argv) > 1 else ""
    if command == "serve":
        serve()
        return 0
    if command == "status":
        print(json.dumps(daemon_status(), indent=2))
        return 0
    if command == "stop":
        return 0 if stop_daemon() else 1
    if command in ("run", "build"):
        args = list(argv[2:])
        if args and args[0] == "--":
            args.pop(0)
        if not args:
            print("a cargo command is required", file=sys.stderr)
            return 2
        return submit(args)
    print(__doc__.strip(), file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
