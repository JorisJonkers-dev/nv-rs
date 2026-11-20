"""Watching a running bench session without disturbing it, and recording any nested session.

A run writes `session.json` (the nested compositor's socket, the shots folder, the pids). `bench watch` attaches
read-only clients to that compositor's own socket: they never touch the maintainer's session, take no focus (the nested
sway has no seat of ours) and no input (wayvnc runs with --disable-input; the web view only reads frames). A run that had
a watcher attached is marked `watched` in summary.json: a watcher costs a little GPU/CPU, so such numbers are not baselines.

  web (default)  one low-rate grim capture shared by all browsers, served as MJPEG on http://127.0.0.1:PORT/
  vnc            wayvnc on localhost (rate limited, input disabled): connect any VNC client
  offscreen runs (no compositor) show the newest file of the run's --shots folder in the web view.
"""
import json
import os
import signal
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import tools


# ---------------------------------------------------------------------------------------------
# the session file and the markers a run carries


def write_session(run_dir, comp, opts, bench_pid=None):
    """session.json: how to reach the run's screen. `comp` is nested.Nested or None."""
    info = {
        "bench_pid": bench_pid or os.getpid(), "run": Path(run_dir).name, "started_unix": time.time(),
        "socket": comp.socket if comp else None, "runtime": comp.runtime if comp else None,
        "compositor_pid": comp.proc.pid if comp and comp.proc else None,
        "mode": opts.get("nested") or ("window" if opts.get("windowed") else "offscreen"),
        "resolution": opts.get("resolution"), "shots": (opts.get("shots") or "").split(",")[0] or None,
    }
    (Path(run_dir) / "session.json").write_text(json.dumps(info, indent=1))


def write_steps(run_dir, plan):
    """steps.json: the scenario's scripted input as labelled steps by main frame (labels are key codes and counts)."""
    steps = [{"frame": f, "name": f"key {d:X} x{h}"} for f, d, h, *_ in plan.keys]
    steps += [{"frame": f, "name": f"mouse {dx:+d},{dy:+d}"} for f, dx, dy, *_ in plan.mouse]
    steps.sort(key=lambda s: s["frame"])
    (Path(run_dir) / "steps.json").write_text(json.dumps({"scenario": plan.name, "steps": steps}))


def mark_watched(run_dir, how):
    """Appends an attach to watch.json; summary.json reads it back as settings.watched."""
    p = Path(run_dir) / "watch.json"
    try:
        data = json.loads(p.read_text())
    except (OSError, ValueError):
        data = {"attached": []}
    data["attached"].append({"how": how, "unix": time.time()})
    p.write_text(json.dumps(data))


def was_watched(run_dir):
    return (Path(run_dir) / "watch.json").exists()


def alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except (OSError, TypeError):
        return False


def load_session(run_dir):
    try:
        return json.loads((Path(run_dir) / "session.json").read_text())
    except (OSError, ValueError):
        return None


def find_session(target, state, resolve):
    """(run_dir, session) for `latest`, a run name or path, or a pid of a bench run or of its compositor."""
    if target.isdigit():
        for d in sorted(Path(state).glob("*-*T*"), reverse=True):
            s = load_session(d)
            if s and int(target) in (s.get("bench_pid"), s.get("compositor_pid")):
                return d, s
        raise SystemExit(f"no bench session with pid {target}")
    if target == "latest":
        runs = sorted((d for d in Path(state).glob("*-*T*") if load_session(d)), key=lambda d: d.name.rsplit("-", 1)[1])
        live = [d for d in runs if alive(load_session(d)["bench_pid"])]
        pick = (live or runs or [None])[-1]
        if pick is None:
            raise SystemExit("no bench session found (start one with bench run ... --nested headless)")
        return pick, load_session(pick)
    d = resolve(target)
    s = load_session(d)
    if not s:
        raise SystemExit(f"{d} has no session.json (a run from before watch existed)")
    return d, s


def wayland_env(s):
    e = dict(os.environ)
    e.pop("DISPLAY", None)
    e.update(WAYLAND_DISPLAY=s["socket"], XDG_RUNTIME_DIR=s["runtime"])
    return e


# ---------------------------------------------------------------------------------------------
# the web view


def newest_shot(folder):
    try:
        files = [p for p in Path(folder).iterdir() if p.suffix.lower() in (".png", ".jpg", ".jpeg")]
        return max(files, key=lambda p: p.stat().st_mtime) if files else None
    except OSError:
        return None


class Source:
    """The latest frame (jpeg or png bytes), captured at `fps` only while a browser is connected."""

    def __init__(self, s, fps, scale):
        self.s, self.fps, self.scale = s, fps, scale
        self.frame, self.kind, self.clients = None, "jpeg", 0
        self.lock = threading.Lock()
        grim = tools.find("grim") if s.get("socket") else None
        if s.get("socket") and not grim:
            raise SystemExit(f"grim is not installed (needed to watch a nested session): {tools.HINT}")
        self.grim = grim

    def grab(self):
        if self.grim:
            r = subprocess.run([self.grim, "-t", "jpeg", "-q", "55", "-s", str(self.scale), "-"], capture_output=True, env=wayland_env(self.s), timeout=10)
            return (r.stdout, "jpeg") if r.returncode == 0 and r.stdout else (None, "jpeg")
        shot = newest_shot(self.s.get("shots") or "")
        if shot:
            try:
                return shot.read_bytes(), "png" if shot.suffix.lower() == ".png" else "jpeg"
            except OSError:
                pass
        return None, "jpeg"

    def loop(self):
        while True:
            t = time.monotonic()
            if self.clients > 0:
                try:
                    f, k = self.grab()
                    if f:
                        self.frame, self.kind = f, k
                except (OSError, subprocess.SubprocessError):
                    pass
            time.sleep(max(0.0, 1.0 / self.fps - (time.monotonic() - t)))


PAGE = b"""<!doctype html><meta charset=utf-8><meta name=viewport content="width=device-width,initial-scale=1">
<title>bench watch</title><style>body{margin:0;background:#000;color:#999;font:13px system-ui}img{width:100%;height:auto;display:block}</style>
<img src=/stream alt="the bench session's screen (read-only)">"""


def handler(src):
    class H(BaseHTTPRequestHandler):
        def log_message(self, *a):
            pass

        def do_GET(self):
            if self.path == "/frame":
                f = src.frame
                self.send_response(200 if f else 503)
                self.send_header("Content-Type", f"image/{src.kind}")
                self.end_headers()
                if f:
                    self.wfile.write(f)
            elif self.path == "/stream":
                self.send_response(200)
                self.send_header("Content-Type", "multipart/x-mixed-replace; boundary=f")
                self.end_headers()
                src.clients += 1
                last = None
                try:
                    while True:
                        f = src.frame
                        if f is not None and f is not last:
                            last = f
                            self.wfile.write(b"--f\r\nContent-Type: image/" + src.kind.encode() + b"\r\nContent-Length: " + str(len(f)).encode() + b"\r\n\r\n" + f + b"\r\n")
                        time.sleep(0.05)
                except OSError:
                    pass
                finally:
                    src.clients -= 1
            else:
                self.send_response(200)
                self.send_header("Content-Type", "text/html")
                self.end_headers()
                self.wfile.write(PAGE)

    return H


def serve_web(s, run_dir, a):
    src = Source(s, a.fps, a.scale)
    threading.Thread(target=src.loop, daemon=True).start()
    srv = ThreadingHTTPServer((a.bind, a.port), handler(src))
    srv.daemon_threads = True
    mark_watched(run_dir, "web")
    host = "localhost" if a.bind in ("127.0.0.1", "localhost") else a.bind
    print(f"watching {Path(run_dir).name}: http://{host}:{srv.server_address[1]}/  ({a.fps} fps at {a.scale}x, read-only; Ctrl-C stops the view, not the run)")
    if a.bind not in ("127.0.0.1", "localhost"):
        print("warning: bound beyond localhost: anyone who can reach this port sees the game screen", file=sys.stderr)
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        pass
    return 0


def serve_vnc(s, run_dir, a):
    if not s.get("socket"):
        raise SystemExit("vnc needs a nested session (--nested headless); an offscreen run has only --shots frames: use the web view")
    wv = tools.need("wayvnc", "serve the screen over VNC")
    mark_watched(run_dir, "vnc")
    cmd = [wv, "--disable-input", "-f", str(a.fps), "--log-level=warning", a.bind, str(a.port)]
    print(f"wayvnc on {a.bind}:{a.port} (input disabled, rate {a.fps} fps): connect with  vncviewer {a.bind}:{a.port}   (nix shell nixpkgs#tigervnc)")
    try:
        return subprocess.run(cmd, env=wayland_env(s)).returncode
    except KeyboardInterrupt:
        return 0


def cmd_watch(a, state, resolve):
    d, s = find_session(a.target, state, resolve)
    if s.get("mode") == "window":
        print("this session is a window in your own session: look at it there (nothing to attach)")
        return 0
    if not alive(s["bench_pid"]):
        print(f"note: {d.name} has finished; showing what is left of it", file=sys.stderr)
    return (serve_vnc if a.mode == "vnc" else serve_web)(s, d, a)


def cmd_record(a, state, resolve):
    """Records any nested session until Ctrl-C (or until the run ends) into its run directory."""
    import record
    d, s = find_session(a.target, state, resolve)
    if not s.get("socket"):
        raise SystemExit("this run has no nested compositor to record")
    rec = record.Recorder(d, s["socket"], s["runtime"], fps=a.fps, fmt=a.format, cap=int(a.cap_mb * 1024 * 1024))
    if not rec.start():
        raise SystemExit(f"could not record: {rec.note}")
    mark_watched(d, "record")
    print(f"recording {d.name} to {rec.path} ({rec.codec}, {rec.fps} fps); Ctrl-C stops")
    first = None
    try:
        while rec.proc.poll() is None and alive(s["bench_pid"]):
            if first is None and (d / "frames.tsv").exists() and (d / "frames.tsv").stat().st_size > 400:
                first = time.time()
            time.sleep(1)
    except KeyboardInterrupt:
        pass
    rec.stop(first)
    print(f"stopped; replay with: nv-rs-dev bench replay {d.name}")
    return 0


def register(sub, state, resolve, replay):
    w = sub.add_parser("watch", help="show a running nested bench session's screen without disturbing it")
    w.add_argument("target", nargs="?", default="latest", help="latest (default), a run name or directory, or the pid of a bench run")
    w.add_argument("--mode", choices=["web", "vnc"], default="web")
    w.add_argument("--fps", type=int, default=5, help="capture rate (low: a watcher costs the run something)")
    w.add_argument("--scale", type=float, default=0.5, help="web view: downscale factor of the frames")
    w.add_argument("--bind", default="127.0.0.1", help="address to serve on (localhost; another address shows the game screen to the network)")
    w.add_argument("--port", type=int, default=0, help="port (web: any free one by default; vnc: 5900)")
    w.set_defaults(fn=lambda a: cmd_watch(fix_port(a), state, resolve))
    r = sub.add_parser("record", help="record a running nested bench session into its run directory")
    r.add_argument("target", nargs="?", default="latest")
    r.add_argument("--fps", type=int, default=30)
    r.add_argument("--format", choices=["mp4", "webm"], default="mp4")
    r.add_argument("--cap-mb", type=int, default=2048, help="stop recording at this size")
    r.set_defaults(fn=lambda a: cmd_record(a, state, resolve))
    p = sub.add_parser("replay", help="build and show the replay page (video with a frame-time timeline) of a recorded run")
    p.add_argument("run", nargs="?", default="latest")
    p.add_argument("--burn", action="store_true", help="also burn the timeline into a copy of the video (ffmpeg)")
    p.add_argument("--play", action="store_true", help="play it with mpv now")
    p.set_defaults(fn=lambda a: replay.cmd_replay(a, resolve))


def fix_port(a):
    if a.mode == "vnc" and not a.port:
        a.port = 5900
    return a
